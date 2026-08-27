use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapters::{finish, i64_field, parse_jsonl_values, text_field};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "KIMI_DATA_DIR", ".kimi", "")
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
    ingest::content_fingerprint(&root.join("kimi.json"))
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
    let session_id = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    let project = projects
        .iter()
        .find(|(id, _)| id == &session_id)
        .map(|(_, project)| project.clone())
        .unwrap_or_else(|| {
            path.parent()
                .and_then(|parent| parent.parent())
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_string()
        });
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let content = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
    ingest::validate_jsonl(content)?;
    Ok(parse_kimi_wire(content, &path.to_string_lossy(), &project))
}

fn projects(root: &Path) -> Result<Vec<(String, String)>, String> {
    let path = root.join("kimi.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let value =
        serde_json::from_str::<serde_json::Value>(&text).map_err(|error| error.to_string())?;
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

pub fn parse_kimi_wire(content: &str, source_file: &str, project: &str) -> Vec<UsageRecord> {
    let session_id = std::path::Path::new(source_file)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let mut last_by_message: HashMap<String, UsageRecord> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for value in parse_jsonl_values(content) {
        let message = value.get("message").cloned().unwrap_or_default();
        if message.get("type").and_then(|v| v.as_str()) != Some("StatusUpdate") {
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
            .and_then(|v| v.as_f64())
            .map(|secs| {
                chrono::DateTime::from_timestamp(secs as i64, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if !last_by_message.contains_key(&message_id) {
            order.push(message_id.clone());
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
                reasoning_tokens: 0,
                total_tokens: 0,
                native_cost: None,
            }),
        );
    }

    order
        .into_iter()
        .filter_map(|id| last_by_message.remove(&id))
        .collect()
}
