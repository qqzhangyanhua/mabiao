use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{finish, i64_field, parse_jsonl_values, text_field};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides
        .get("KIMI_DATA_DIR")
        .cloned()
        .unwrap_or_else(|| vec![home.join(".kimi"), home.join(".kimi-code")])
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        for path in ingest::walk_files(&root.join("sessions"), "jsonl")? {
            if path.file_name().and_then(|name| name.to_str()) == Some("wire.jsonl") {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

pub(crate) fn sidecar_fingerprint(path: &Path, dirs: &[PathBuf]) -> String {
    let root = dirs
        .iter()
        .find(|dir| path.starts_with(dir))
        .cloned()
        .unwrap_or_else(|| path.to_path_buf());
    let mut fingerprint = ingest::content_fingerprint(&root.join("kimi.json"));
    if let Some(state) = state_json_path(path) {
        if state.exists() {
            fingerprint.push(':');
            fingerprint.push_str(&ingest::content_fingerprint(&state));
        }
    }
    fingerprint
}

pub(crate) fn detected(dirs: &[PathBuf]) -> bool {
    dirs.iter().any(|root| root.join("sessions").exists())
}

pub(crate) fn prepare_dir(scan_dir: &Path) -> Result<(), (PathBuf, String)> {
    projects(scan_dir).map(|_| ()).map_err(|error| {
        (
            scan_dir.join("kimi.json"),
            format!("Kimi 项目映射无效：{error}"),
        )
    })
}

pub(crate) fn parse(path: &Path, scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let projects = projects(scan_dir).map_err(|error| format!("Kimi 项目映射无效：{error}"))?;
    let session_id = session_id_from_wire(path);
    let state = load_state(path)?;
    let project = state
        .as_ref()
        .map(cwd_from)
        .filter(|cwd| !cwd.is_empty())
        .or_else(|| {
            projects
                .into_iter()
                .find(|(id, _)| id == &session_id)
                .map(|(_, project)| project)
        })
        .unwrap_or_else(|| directory_project(path));
    let fork = state.as_ref().and_then(fork_from_state);
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let content = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
    ingest::validate_jsonl(content)?;
    Ok(parse_kimi_wire_inner(
        content,
        &path.to_string_lossy(),
        &project,
        fork,
    ))
}

/// 旧布局 `sessions/<hash>/<id>/wire.jsonl` 的会话是 wire 的父目录。
/// 新布局 `sessions/<工作区>/<会话>/agents/<agent>/wire.jsonl` 的会话是 `agents` 的父目录，
/// 子代理文件和 main 共用这个 id。
pub(crate) fn session_id_from_wire(path: &Path) -> String {
    session_dir(path)
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn directory_project(path: &Path) -> String {
    session_dir(path)
        .and_then(|dir| dir.parent())
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string()
}

/// 新布局才有会话级 `state.json`。文件可以不存在，调用方据此决定要不要把它算进指纹。
pub(crate) fn state_json_path(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let grand = parent.parent()?;
    if grand.file_name().and_then(|name| name.to_str()) != Some("agents") {
        return None;
    }
    Some(grand.parent()?.join("state.json"))
}

pub(crate) fn state_cwd(path: &Path) -> Result<Option<String>, String> {
    Ok(load_state(path)?
        .as_ref()
        .map(cwd_from)
        .filter(|cwd| !cwd.is_empty()))
}

fn session_dir(path: &Path) -> Option<&Path> {
    let parent = path.parent()?;
    match parent.parent() {
        Some(grand) if grand.file_name().and_then(|name| name.to_str()) == Some("agents") => {
            grand.parent()
        }
        _ => Some(parent),
    }
}

fn projects(root: &Path) -> Result<Vec<(String, String)>, String> {
    let path = root.join("kimi.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let value = serde_json::from_str::<Value>(&text).map_err(|error| error.to_string())?;
    Ok(value
        .get("work_dirs")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some((
                        item.get("last_session_id")?.as_str()?.to_string(),
                        item.get("path")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default())
}

fn load_state(path: &Path) -> Result<Option<Value>, String> {
    let Some(state_path) = state_json_path(path) else {
        return Ok(None);
    };
    if !state_path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&state_path)
        .map_err(|error| format!("读取 Kimi state.json 失败：{error}"))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| format!("Kimi state.json 无效：{error}"))
}

fn cwd_from(value: &Value) -> String {
    let cwd = text_field(value, &["cwd"]);
    if cwd.is_empty() {
        text_field(value, &["workDir"])
    } else {
        cwd
    }
}

#[derive(Clone, Copy)]
struct KimiFork {
    created_at_ms: Option<i64>,
}

fn fork_from_state(value: &Value) -> Option<KimiFork> {
    let forked = value
        .get("forkedFrom")
        .and_then(|item| item.as_str())
        .is_some_and(|item| !item.is_empty());
    if !forked {
        return None;
    }
    Some(KimiFork {
        created_at_ms: value.get("createdAt").and_then(json_i64).or_else(|| {
            value
                .get("createdAt")
                .and_then(|item| item.as_str())
                .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                .map(|time| time.timestamp_millis())
        }),
    })
}

fn json_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
        .or_else(|| value.as_f64().map(|number| number.round() as i64))
}

pub fn parse_kimi_wire(content: &str, source_file: &str, project: &str) -> Vec<UsageRecord> {
    parse_kimi_wire_inner(content, source_file, project, None)
}

#[derive(Clone, Copy)]
enum KimiUsageCutoff {
    BeforeValue(usize),
    BeforeMillis(i64),
}

fn parse_kimi_wire_inner(
    content: &str,
    source_file: &str,
    project: &str,
    fork: Option<KimiFork>,
) -> Vec<UsageRecord> {
    let session_id = session_id_from_wire(Path::new(source_file));
    let cutoff = usage_cutoff(content, fork);
    let mut last_by_message: HashMap<String, UsageRecord> = HashMap::new();
    let mut slots: Vec<WireSlot> = Vec::new();

    for (index, value) in parse_jsonl_values(content).enumerate() {
        if value.get("type").and_then(|kind| kind.as_str()) == Some("usage.record") {
            if let Some(counted) = counted_usage(&value, source_file, project, &session_id) {
                if !usage_dropped(index, counted.time_ms, cutoff) {
                    slots.push(WireSlot::Usage(counted.record));
                }
            }
            continue;
        }
        let message = value.get("message").cloned().unwrap_or_default();
        if message.get("type").and_then(|kind| kind.as_str()) != Some("StatusUpdate") {
            continue;
        }
        let payload = message.get("payload").cloned().unwrap_or_default();
        let usage = payload.get("token_usage").cloned().unwrap_or_default();
        if usage.is_null() {
            continue;
        }
        let message_id = text_field(&payload, &["message_id"]);
        if message_id.is_empty() {
            continue;
        }
        let occurred = value
            .get("timestamp")
            .and_then(|item| item.as_f64())
            .map(|secs| {
                chrono::DateTime::from_timestamp(secs as i64, 0)
                    .map(|time| time.to_rfc3339())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if !last_by_message.contains_key(&message_id) {
            slots.push(WireSlot::Status(message_id.clone()));
        }
        last_by_message.insert(
            message_id,
            finish(UsageRecord {
                occurred_at: occurred,
                source: Source::Kimi,
                model: String::new(),
                provider: String::new(),
                project: project.to_string(),
                session_id: session_id.clone(),
                source_file: source_file.to_string(),
                input_tokens: i64_field(&usage, &["input_other"]),
                output_tokens: i64_field(&usage, &["output"]),
                cache_read_tokens: i64_field(&usage, &["input_cache_read"]),
                cache_creation_tokens: i64_field(&usage, &["input_cache_creation"]),
                cache_creation_1h_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 0,
                native_cost: None,
            }),
        );
    }

    slots
        .into_iter()
        .filter_map(|slot| match slot {
            WireSlot::Status(id) => last_by_message.remove(&id),
            WireSlot::Usage(record) => Some(record),
        })
        .collect()
}

enum WireSlot {
    Status(String),
    Usage(UsageRecord),
}

struct CountedUsage {
    time_ms: i64,
    record: UsageRecord,
}

fn counted_usage(
    value: &Value,
    source_file: &str,
    project: &str,
    session_id: &str,
) -> Option<CountedUsage> {
    let time_ms = i64_field(value, &["time"]);
    if time_ms <= 0 {
        return None;
    }
    let usage = value.get("usage").cloned().unwrap_or_default();
    let input = i64_field(&usage, &["inputOther"]);
    let output = i64_field(&usage, &["output"]);
    let cache_read = i64_field(&usage, &["inputCacheRead"]);
    let cache_creation = i64_field(&usage, &["inputCacheCreation"]);
    if input < 0 || output < 0 || cache_read < 0 || cache_creation < 0 {
        return None;
    }
    if input == 0 && output == 0 && cache_read == 0 && cache_creation == 0 {
        return None;
    }
    let occurred = chrono::DateTime::from_timestamp_millis(time_ms)
        .map(|time| time.to_rfc3339())
        .unwrap_or_default();
    Some(CountedUsage {
        time_ms,
        record: finish(UsageRecord {
            occurred_at: occurred,
            source: Source::Kimi,
            model: text_field(value, &["model"]),
            provider: String::new(),
            project: project.to_string(),
            session_id: session_id.to_string(),
            source_file: source_file.to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_creation,
            cache_creation_1h_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            native_cost: None,
        }),
    })
}

/// fork 会把父会话的 `usage.record` 原样拷进新文件。`forkedFrom` 非空时，
/// 只留最后一条顶层 `{"type":"forked"}` 之后的记录；没有标记则只留 `createdAt` 之后的。
/// `StatusUpdate` 不走这个截断。
fn usage_cutoff(content: &str, fork: Option<KimiFork>) -> Option<KimiUsageCutoff> {
    let fork = fork?;
    let marker = parse_jsonl_values(content)
        .enumerate()
        .filter(|(_, value)| value.get("type").and_then(|kind| kind.as_str()) == Some("forked"))
        .map(|(index, _)| index)
        .last();
    if let Some(index) = marker {
        return Some(KimiUsageCutoff::BeforeValue(index));
    }
    fork.created_at_ms.map(KimiUsageCutoff::BeforeMillis)
}

fn usage_dropped(index: usize, time_ms: i64, cutoff: Option<KimiUsageCutoff>) -> bool {
    match cutoff {
        Some(KimiUsageCutoff::BeforeValue(marker)) => index < marker,
        Some(KimiUsageCutoff::BeforeMillis(since)) => time_ms < since,
        None => false,
    }
}
