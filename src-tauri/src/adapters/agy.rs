//! Antigravity（`agy`）Usage Source。
//!
//! 两个默认根（同一产品、同一套库结构、同一个账号额度池）：
//! CLI `~/.gemini/antigravity-cli` 与 IDE `~/.gemini/antigravity-ide`，
//! 各自再进 `conversations/`。`AGY_DATA_DIR` 整体覆盖两个根（Claude Code 先例）。
//! 与官方额度 provider `antigravity` 分属两个维度，不合并。
//!
//! 一条消耗记录 = `steps.metadata`（`CortexStepMetadata`）里的一次模型用量：
//! 字段 9 是 `ModelUsageStats`（服务端计费六元组 + request id），字段 1 是创建时间。
//! 模型名只从 `gen_metadata.data` 的 `ChatModelMetadata` 字段 19 按 request id 拼接；
//! 那里的字段 4 用量是客户端 prompt 估算，不进消耗记录。
//!
//! 项目取 `trajectory_metadata_blob.data` 会话级元数据里的 workspace URI
//!（字段号来自 agy 二进制内 FileDescriptorProto，与 step/gen 同一权威）：
//! **字段 1 → 嵌套字段 1**（string）。值为该 URI 的最后一段（可先剥 `file://`，
//! 以及 Windows `file:///C:/…` 多出来的前导斜杠）。路径能 `canonicalize` 时
//! 用磁盘真实大小写；失败则原样保留 URI 最后一段。不读 blob 里的 git remote，
//! 也不做小写化。URI 缺失、空串或 blob 畸形时项目留空，用量行仍解析。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::adapters::finish;
use crate::adapters::project::session_id_from_source_file;
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};
use crate::proto_wire;

pub(crate) const PATH_ENV: &str = "AGY_DATA_DIR";
const DEFAULT_ROOTS: [&str; 2] = [".gemini/antigravity-cli", ".gemini/antigravity-ide"];
const CONVERSATIONS: &str = "conversations";

/// `CortexStepMetadata` 字段号（agy 二进制 Go protobuf tag）。
const STEP_CREATED_AT: u32 = 1;
const STEP_USAGE: u32 = 9;

/// `google.protobuf.Timestamp`
const TS_SECONDS: u32 = 1;
const TS_NANOS: u32 = 2;

/// `ModelUsageStats`：2/3/4/5/9/10/11。reasoning 是 output 的子集，不进 total。
const USAGE_INPUT: u32 = 2;
const USAGE_OUTPUT: u32 = 3;
const USAGE_CACHE_WRITE: u32 = 4;
const USAGE_CACHE_READ: u32 = 5;
const USAGE_THINKING: u32 = 9;
const USAGE_REQUEST_ID: u32 = 11;

/// `ChatModelMetadata`：只取模型名与 request id，不用字段 4 的估算用量。
const GEN_USAGE: u32 = 4;
const GEN_MODEL: u32 = 19;

/// `trajectory_metadata_blob.data`：会话级 workspace。
/// FileDescriptorProto：字段 1（嵌套消息）→ 字段 1（workspace URI string）。
const TRAJECTORY_WORKSPACE: u32 = 1;
const WORKSPACE_URI: u32 = 1;

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    let roots = overrides.get(PATH_ENV).cloned().unwrap_or_else(|| {
        DEFAULT_ROOTS
            .iter()
            .map(|relative| home.join(relative))
            .collect()
    });
    roots
        .into_iter()
        .map(|root| root.join(CONVERSATIONS))
        .collect()
}

/// 只白名单 `.db`。同目录旧版加密 `.pb` 必须显式排除：尝试解析失败会计入
/// `files_failed`，进而按既有对账规则拖停整个来源。
pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        paths.extend(ingest::walk_files(root, "db")?);
    }
    Ok(paths)
}

/// 「已检测到」= 任一扫描根（已拼好的 `conversations/`）下至少有一个 `.db`。
/// 目录存在或只有加密 `.pb` 都不算检测到。
pub(crate) fn detected(dirs: &[PathBuf]) -> bool {
    dirs.iter().any(|root| {
        ingest::walk_files(root, "db")
            .map(|files| !files.is_empty())
            .unwrap_or(false)
    })
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let Ok(conn) = ingest::open_readonly(path) else {
        return Ok(Vec::new());
    };
    let models = load_model_names(&conn);
    let project = load_project(&conn);
    let session_id = session_id_from_source_file(&path.to_string_lossy());
    let source_file = path.to_string_lossy().into_owned();
    let mut records: Vec<UsageRecord> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for blob in load_step_blobs(&conn) {
        let Some((request_id, record)) =
            record_from_step(&blob, &session_id, &source_file, &project, &models)
        else {
            continue;
        };
        match index.get(&request_id) {
            Some(&pos) if records[pos].output_tokens >= record.output_tokens => {}
            Some(&pos) => records[pos] = record,
            None => {
                index.insert(request_id, records.len());
                records.push(record);
            }
        }
    }
    Ok(records)
}

fn load_step_blobs(conn: &rusqlite::Connection) -> Vec<Vec<u8>> {
    let Ok(mut stmt) = conn.prepare("SELECT metadata FROM steps WHERE metadata IS NOT NULL") else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, Vec<u8>>(0)) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

fn load_model_names(conn: &rusqlite::Connection) -> HashMap<String, String> {
    let Ok(mut stmt) = conn.prepare("SELECT data FROM gen_metadata WHERE data IS NOT NULL") else {
        return HashMap::new();
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, Vec<u8>>(0)) else {
        return HashMap::new();
    };
    let mut models = HashMap::new();
    for blob in rows.filter_map(Result::ok) {
        collect_model_names(&blob, &mut models);
    }
    models
}

fn load_project(conn: &rusqlite::Connection) -> String {
    let Ok(mut stmt) =
        conn.prepare("SELECT data FROM trajectory_metadata_blob WHERE data IS NOT NULL")
    else {
        return String::new();
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, Vec<u8>>(0)) else {
        return String::new();
    };
    for blob in rows.filter_map(Result::ok) {
        if let Some(uri) = proto_wire::text_at_path(&blob, &[TRAJECTORY_WORKSPACE, WORKSPACE_URI]) {
            return project_from_workspace_uri(uri);
        }
    }
    String::new()
}

/// 剥 `file://`，再处理 Windows `file:///C:/…` 留下的 `/C:/` 前导斜杠。
fn filesystem_path_from_workspace_uri(uri: &str) -> Option<PathBuf> {
    let stripped = uri.strip_prefix("file://").unwrap_or(uri);
    let path = strip_windows_drive_leading_slash(stripped);
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

fn strip_windows_drive_leading_slash(path: &str) -> &str {
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        &path[1..]
    } else {
        path
    }
}

fn last_path_segment(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string()
}

fn project_from_workspace_uri(uri: &str) -> String {
    let Some(fs_path) = filesystem_path_from_workspace_uri(uri) else {
        return String::new();
    };
    if let Ok(canonical) = std::fs::canonicalize(&fs_path) {
        return last_path_segment(&canonical);
    }
    last_path_segment(&fs_path)
}

fn collect_model_names(blob: &[u8], models: &mut HashMap<String, String>) {
    proto_wire::for_each_bytes_field(blob, |_field, inner| {
        let Some(model) = proto_wire::text_at_path(inner, &[GEN_MODEL]) else {
            return;
        };
        let Some(request_id) = proto_wire::text_at_path(inner, &[GEN_USAGE, USAGE_REQUEST_ID])
        else {
            return;
        };
        models.insert(request_id.to_string(), model.to_string());
    });
}

fn record_from_step(
    blob: &[u8],
    session_id: &str,
    source_file: &str,
    project: &str,
    models: &HashMap<String, String>,
) -> Option<(String, UsageRecord)> {
    let usage = proto_wire::bytes_at_path(blob, &[STEP_USAGE])?;
    let request_id = proto_wire::text_at_path(usage, &[USAGE_REQUEST_ID])?.to_string();
    let input_tokens = token(proto_wire::varint_at_path(usage, &[USAGE_INPUT]));
    let output_tokens = token(proto_wire::varint_at_path(usage, &[USAGE_OUTPUT]));
    let cache_creation_tokens = token(proto_wire::varint_at_path(usage, &[USAGE_CACHE_WRITE]));
    let cache_read_tokens = token(proto_wire::varint_at_path(usage, &[USAGE_CACHE_READ]));
    let reasoning_tokens = token(proto_wire::varint_at_path(usage, &[USAGE_THINKING]));
    if input_tokens == 0
        && output_tokens == 0
        && cache_read_tokens == 0
        && cache_creation_tokens == 0
    {
        return None;
    }
    // 必须显式写 total：领域兜底会再加 reasoning，而 thinking 已含在 output 里。
    let total_tokens = input_tokens + output_tokens + cache_read_tokens + cache_creation_tokens;
    let record = finish(UsageRecord {
        occurred_at: timestamp_at(blob, &[STEP_CREATED_AT]),
        source: Source::Agy,
        model: models.get(&request_id).cloned().unwrap_or_default(),
        provider: String::new(),
        project: project.to_string(),
        session_id: session_id.to_string(),
        source_file: source_file.to_string(),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        reasoning_tokens,
        total_tokens,
        native_cost: None,
    });
    Some((request_id, record))
}

fn token(value: Option<u64>) -> i64 {
    value.and_then(|n| i64::try_from(n).ok()).unwrap_or(0)
}

fn timestamp_at(blob: &[u8], path: &[u32]) -> String {
    let Some(raw) = proto_wire::bytes_at_path(blob, path) else {
        return String::new();
    };
    let Some(seconds) = proto_wire::varint_at_path(raw, &[TS_SECONDS]) else {
        return String::new();
    };
    let Ok(seconds) = i64::try_from(seconds) else {
        return String::new();
    };
    if seconds <= 0 {
        return String::new();
    }
    let nanos = proto_wire::varint_at_path(raw, &[TS_NANOS]).unwrap_or(0);
    let nanos = u32::try_from(nanos)
        .ok()
        .filter(|value| *value < 1_000_000_000)
        .unwrap_or(0);
    chrono::DateTime::from_timestamp(seconds, nanos)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}
