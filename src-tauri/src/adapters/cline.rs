use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{
    discover_suffix, finish, has_billable_tokens, i64_field, parse_whole_json, text_field,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// Cline CLI 会话目录：`$CLINE_SESSION_DATA_DIR`，否则 `~/.cline/data/sessions`。
/// 每个会话一个文件夹，主文件是 `<id>.messages.json`（整份重写）。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(
        overrides,
        home,
        "CLINE_SESSION_DATA_DIR",
        ".cline/data/sessions",
        "",
    )
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    discover_suffix(roots, ".messages.json")
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    match manifest_beside(path) {
        Some(manifest) => ingest::metadata_fingerprint(&manifest),
        None => String::new(),
    }
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_whole_json(path, parse_cline_messages)
}

pub fn parse_cline_messages(content: &str, source_file: &str) -> Vec<UsageRecord> {
    let value: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let messages = match value.get("messages").and_then(|v| v.as_array()) {
        Some(messages) => messages,
        None => return Vec::new(),
    };

    let manifest = manifest_beside(Path::new(source_file))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let forked_at = manifest.as_ref().and_then(forked_at_millis);
    let project = manifest
        .as_ref()
        .map(|value| text_field(value, &["cwd"]))
        .unwrap_or_default();
    let session_id = session_id_from_path(source_file);

    let mut records = Vec::new();
    for message in messages {
        if message.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let ts = i64_field(message, &["ts"]);
        if let Some(since) = forked_at {
            if ts < since {
                continue;
            }
        }
        let model_info = message.get("modelInfo").cloned().unwrap_or(Value::Null);
        let model = text_field(&model_info, &["id"]);
        if model.is_empty() {
            continue;
        }
        let metrics = match message.get("metrics") {
            Some(metrics) if !metrics.is_null() => metrics,
            _ => continue,
        };
        let cache_read = i64_field(metrics, &["cacheReadTokens"]);
        let cache_write = i64_field(metrics, &["cacheWriteTokens"]);
        let raw_input = i64_field(metrics, &["inputTokens"]);
        let record = finish(UsageRecord {
            occurred_at: occurred_at_from_ts(ts),
            source: Source::Cline,
            model,
            provider: text_field(&model_info, &["provider"]),
            project: project.clone(),
            session_id: session_id.clone(),
            source_file: source_file.to_string(),
            input_tokens: (raw_input - cache_read - cache_write).max(0),
            output_tokens: i64_field(metrics, &["outputTokens"]),
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_write,
            reasoning_tokens: 0,
            total_tokens: 0,
            native_cost: native_cost_from_metrics(metrics),
        });
        if has_billable_tokens(&record) {
            records.push(record);
        }
    }
    records
}

fn manifest_beside(messages: &Path) -> Option<PathBuf> {
    let name = messages.file_name()?.to_str()?;
    let stem = name.strip_suffix(".messages.json")?;
    Some(messages.parent()?.join(format!("{stem}.json")))
}

fn session_id_from_path(source_file: &str) -> String {
    Path::new(source_file)
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string()
}

fn forked_at_millis(manifest: &Value) -> Option<i64> {
    let raw = manifest.pointer("/metadata/fork/forkedAt")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn occurred_at_from_ts(ts: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ts)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn native_cost_from_metrics(metrics: &Value) -> Option<f64> {
    let amount = metrics.get("cost").and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_i64().map(|n| n as f64))
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })?;
    (amount > 0.0).then_some(amount)
}
