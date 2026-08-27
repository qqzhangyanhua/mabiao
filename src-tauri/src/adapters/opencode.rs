use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{finish, has_billable_tokens, i64_field, text_field};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(
        overrides,
        home,
        "OPENCODE_DATA_DIR",
        ".local/share/opencode",
        "opencode.db",
    )
}

/// 扫描目录解析出来的就是数据库文件本身；发现退化成「这个文件存在吗」。
pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(roots.iter().filter(|path| path.exists()).cloned().collect())
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    let wal = PathBuf::from(format!("{}-wal", path.to_string_lossy()));
    ingest::metadata_fingerprint(&wal)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let source_db = ingest::open_readonly(path)?;
    let mut stmt = source_db
        .prepare("SELECT session_id, data FROM message")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let loc = path.to_string_lossy();
    let mut messages = Vec::new();
    for row in rows {
        let (session_id, data) = row.map_err(|e| e.to_string())?;
        let data = serde_json::from_str(&data)
            .map_err(|error| format!("OpenCode message JSON 无效：{error}"))?;
        messages.push(OpencodeMessage {
            session_id,
            source_file: loc.to_string(),
            data,
        });
    }
    Ok(parse_opencode_messages(&messages))
}

pub fn parse_opencode_messages(rows: &[OpencodeMessage]) -> Vec<UsageRecord> {
    rows.iter().filter_map(parse_one).collect()
}

#[derive(Debug, Clone)]
pub struct OpencodeMessage {
    pub session_id: String,
    pub source_file: String,
    pub data: Value,
}

fn parse_one(row: &OpencodeMessage) -> Option<UsageRecord> {
    if row.data.get("role").and_then(|v| v.as_str()) != Some("assistant") {
        return None;
    }
    let tokens = row.data.get("tokens").cloned().unwrap_or_default();
    if !tokens.is_object() {
        return None;
    }
    // 进行中的消息只有半截 token，与 cc-switch 一样等 time.completed 再入账。
    row.data.get("time").and_then(|t| t.get("completed"))?;
    let cache = tokens.get("cache").cloned().unwrap_or_default();
    let path = row.data.get("path").cloned().unwrap_or_default();
    let project = text_field(&path, &["root", "cwd"]);
    let occurred = row
        .data
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(|v| v.as_i64())
        .map(millis_to_rfc3339)
        .unwrap_or_default();
    let native_cost = row
        .data
        .get("cost")
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_i64().map(|n| n as f64))
                .or_else(|| v.get("total").and_then(|n| n.as_f64()))
        })
        .filter(|amount| *amount > 0.0);
    let record = finish(UsageRecord {
        occurred_at: occurred,
        source: Source::Opencode,
        model: text_field(&row.data, &["modelID", "modelId"]),
        provider: text_field(&row.data, &["providerID", "providerId"]),
        project,
        session_id: row.session_id.clone(),
        source_file: row.source_file.clone(),
        input_tokens: i64_field(&tokens, &["input"]),
        output_tokens: i64_field(&tokens, &["output"]),
        cache_read_tokens: i64_field(&cache, &["read"]),
        cache_creation_tokens: i64_field(&cache, &["write"]),
        reasoning_tokens: i64_field(&tokens, &["reasoning"]),
        total_tokens: 0,
        native_cost,
    });
    has_billable_tokens(&record).then_some(record)
}

fn millis_to_rfc3339(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}
