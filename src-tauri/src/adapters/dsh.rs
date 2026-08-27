use std::path::{Path, PathBuf};

use crate::adapters::{discover_suffix, finish, i64_field, parse_jsonl_values, text_field};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "DSH_HOME", ".dsh", "sessions")
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    discover_suffix(roots, "session.jsonl.zstd")
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    parse_dsh_zstd(&bytes, path.to_string_lossy().as_ref())
}

pub fn parse_dsh_jsonl(content: &str, source_file: &str) -> Vec<UsageRecord> {
    let mut session_id = String::new();
    let mut project = String::new();
    let mut provider = String::new();
    let mut model = String::new();
    let mut records = Vec::new();

    for value in parse_jsonl_values(content) {
        match value.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "session" => {
                session_id = text_field(&value, &["id"]);
                project = text_field(&value, &["cwd"]);
            }
            "request/header" => {
                let header = value
                    .get("data")
                    .and_then(|d| d.get("header"))
                    .and_then(|h| h.get("config"))
                    .cloned()
                    .unwrap_or_default();
                let next_provider = text_field(&header, &["provider"]);
                let next_model = text_field(&header, &["model"]);
                if !next_provider.is_empty() {
                    provider = next_provider;
                }
                if !next_model.is_empty() {
                    model = next_model;
                }
            }
            "assistant/message" => {
                let data = value.get("data").cloned().unwrap_or_default();
                let usage = data.get("usage").cloned().unwrap_or_default();
                if usage.is_null() {
                    continue;
                }
                let source = data
                    .get("message")
                    .and_then(|m| m.get("source"))
                    .cloned()
                    .unwrap_or_default();
                let msg_model = text_field(&source, &["model"]);
                let msg_provider = text_field(&source, &["provider"]);
                let occurred = value
                    .get("time")
                    .and_then(|v| v.as_i64())
                    .map(millis_to_rfc3339)
                    .unwrap_or_default();
                records.push(finish(UsageRecord {
                    occurred_at: occurred,
                    source: Source::Dsh,
                    model: if msg_model.is_empty() {
                        model.clone()
                    } else {
                        msg_model
                    },
                    provider: if msg_provider.is_empty() {
                        provider.clone()
                    } else {
                        msg_provider
                    },
                    project: project.clone(),
                    session_id: session_id.clone(),
                    source_file: source_file.to_string(),
                    input_tokens: i64_field(&usage, &["inputTokens"]),
                    output_tokens: i64_field(&usage, &["outputTokens"]),
                    cache_read_tokens: i64_field(&usage, &["cacheReadTokens"]),
                    cache_creation_tokens: i64_field(
                        &usage,
                        &["cacheWriteTokens", "cacheCreationTokens"],
                    ),
                    reasoning_tokens: i64_field(&usage, &["reasoningTokens"]),
                    total_tokens: 0,
                    native_cost: None,
                }));
            }
            _ => {}
        }
    }

    records
}

pub fn parse_dsh_zstd(bytes: &[u8], source_file: &str) -> Result<Vec<UsageRecord>, String> {
    let decoded = zstd::decode_all(bytes).map_err(|e| e.to_string())?;
    let content = String::from_utf8(decoded).map_err(|e| e.to_string())?;
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        serde_json::from_str::<serde_json::Value>(line)
            .map_err(|error| format!("第 {} 行 JSON 无效：{error}", index + 1))?;
    }
    Ok(parse_dsh_jsonl(&content, source_file))
}

fn millis_to_rfc3339(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}
