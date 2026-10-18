use std::fs;
use std::path::{Path, PathBuf};

use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, parse_streaming_jsonl,
    text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::PathOverrides;

/// OpenClaw 会话写在 `$OPENCLAW_STATE_DIR/agents/<agent>/sessions/`，
/// 默认 `~/.openclaw`；升级前的 `~/.clawdbot` 一并扫。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides
        .get("OPENCLAW_STATE_DIR")
        .cloned()
        .map(|roots| roots.into_iter().map(|root| root.join("agents")).collect())
        .unwrap_or_else(|| vec![home.join(".openclaw/agents"), home.join(".clawdbot/agents")])
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for agents_dir in roots {
        if !agents_dir.exists() {
            continue;
        }
        let agents = fs::read_dir(agents_dir)
            .map_err(|error| format!("扫描目录 {} 失败：{error}", agents_dir.display()))?;
        for agent in agents {
            let agent = agent
                .map_err(|error| format!("扫描目录 {} 失败：{error}", agents_dir.display()))?
                .path();
            let sessions = agent.join("sessions");
            if !sessions.is_dir() {
                continue;
            }
            let entries = fs::read_dir(&sessions)
                .map_err(|error| format!("扫描目录 {} 失败：{error}", sessions.display()))?;
            for entry in entries {
                let path = entry
                    .map_err(|error| format!("扫描目录 {} 失败：{error}", sessions.display()))?
                    .path();
                if path.is_file() && is_usage_counted_transcript(&path) {
                    paths.push(path);
                }
            }
        }
    }
    paths.sort();
    Ok(paths)
}

pub fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_openclaw_jsonl)
}

pub fn parse_openclaw_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    let mut session_id = session_id_from_path(source_file);
    let mut project = String::new();
    let mut provider = String::new();
    let mut model = String::new();
    let mut records = Vec::new();

    for value in parse_jsonl_value_lines(lines()) {
        match value.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "session" => {
                let next_id = text_field(&value, &["id", "sessionId"]);
                if !next_id.is_empty() {
                    session_id = next_id;
                }
                let cwd = text_field(&value, &["cwd"]);
                if !cwd.is_empty() {
                    project = cwd;
                }
            }
            "model" => {
                let next_provider = text_field(&value, &["provider", "modelProvider"]);
                if !next_provider.is_empty() {
                    provider = next_provider;
                }
                let next_model = text_field(&value, &["model", "modelId"]);
                if !next_model.is_empty() {
                    model = next_model;
                }
            }
            "message" | "" => {
                let message = value.get("message").cloned().unwrap_or_default();
                let role = text_field(&message, &["role"]);
                let role = if role.is_empty() {
                    text_field(&value, &["role"])
                } else {
                    role
                };
                if role != "assistant" {
                    continue;
                }
                let usage = message
                    .get("usage")
                    .cloned()
                    .or_else(|| value.get("usage").cloned())
                    .unwrap_or(serde_json::Value::Null);
                if usage.is_null() {
                    continue;
                }
                let message_provider = text_field(&message, &["provider"]);
                let message_model = text_field(&message, &["model", "modelId"]);
                let resolved_provider = if message_provider.is_empty() {
                    provider.clone()
                } else {
                    message_provider
                };
                let resolved_model = if message_model.is_empty() {
                    model.clone()
                } else {
                    message_model
                };
                if resolved_provider == "openclaw" && resolved_model == "delivery-mirror" {
                    continue;
                }
                let native_cost = usage
                    .get("cost")
                    .and_then(|cost| cost.get("total"))
                    .and_then(|value| value.as_f64())
                    .filter(|value| value.is_finite() && *value > 0.0);
                let occurred_at = text_field(&value, &["timestamp"]);
                if occurred_at.is_empty() {
                    continue;
                }
                let record = finish(UsageRecord {
                    occurred_at,
                    source: Source::OpenClaw,
                    model: resolved_model,
                    provider: resolved_provider,
                    project: project.clone(),
                    session_id: session_id.clone(),
                    source_file: source_file.to_string(),
                    input_tokens: i64_field(&usage, &["input"]),
                    output_tokens: i64_field(&usage, &["output"]),
                    cache_read_tokens: i64_field(&usage, &["cacheRead"]),
                    cache_creation_tokens: i64_field(&usage, &["cacheWrite"]),
                    cache_creation_1h_tokens: 0,
                    reasoning_tokens: i64_field(&usage, &["reasoningTokens", "reasoning"]),
                    total_tokens: i64_field(&usage, &["totalTokens", "total"]),
                    native_cost,
                });
                if !has_billable_tokens(&record) {
                    continue;
                }
                records.push(record);
            }
            _ => {}
        }
    }
    records
}

pub(crate) fn is_usage_counted_transcript(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    is_usage_counted_name(name)
}

fn is_usage_counted_name(name: &str) -> bool {
    let normalized = name.strip_suffix(".zst").unwrap_or(name);
    if normalized == "sessions.json" {
        return false;
    }
    if normalized.contains(".trajectory") {
        return false;
    }
    if is_compaction_checkpoint(normalized) {
        return false;
    }
    if has_archive_suffix(normalized, "bak") {
        return false;
    }
    if has_archive_suffix(normalized, "reset") || has_archive_suffix(normalized, "deleted") {
        return true;
    }
    normalized.ends_with(".jsonl")
}

fn is_compaction_checkpoint(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".jsonl") else {
        return false;
    };
    let Some((prefix, uuid)) = stem.rsplit_once(".checkpoint.") else {
        return false;
    };
    !prefix.is_empty() && is_uuid(uuid)
}

fn has_archive_suffix(name: &str, reason: &str) -> bool {
    let marker = format!(".jsonl.{reason}.");
    let Some(index) = name.rfind(&marker) else {
        return false;
    };
    let suffix = &name[index + marker.len()..];
    archive_suffix_ok(suffix)
}

fn archive_suffix_ok(suffix: &str) -> bool {
    // `2026-01-01T00-00-00Z` 或带毫秒 `2026-01-01T00-00-00.000Z`，可选再跟 32 位 hex。
    let stamp_len = if suffix.len() >= 24 && suffix.as_bytes().get(19) == Some(&b'.') {
        24
    } else if suffix.len() >= 20 {
        20
    } else {
        return false;
    };
    let Some(stamp) = suffix.get(..stamp_len) else {
        return false;
    };
    if !archive_stamp_ok(stamp) {
        return false;
    }
    match suffix.get(stamp_len..) {
        None | Some("") => true,
        Some(rest) if rest.len() == 33 && rest.starts_with('.') => {
            rest[1..].chars().all(|ch| ch.is_ascii_hexdigit())
        }
        _ => false,
    }
}

fn archive_stamp_ok(stamp: &str) -> bool {
    let bytes = stamp.as_bytes();
    matches!(bytes.get(4), Some(&b'-'))
        && matches!(bytes.get(7), Some(&b'-'))
        && matches!(bytes.get(10), Some(&b'T'))
        && matches!(bytes.get(13), Some(&b'-'))
        && matches!(bytes.get(16), Some(&b'-'))
        && stamp.ends_with('Z')
        && (stamp.len() == 20 || stamp.len() == 24)
}

fn is_uuid(value: &str) -> bool {
    let mut parts = value.split('-');
    let expected = [8, 4, 4, 4, 12];
    for len in expected {
        let Some(part) = parts.next() else {
            return false;
        };
        if part.len() != len || !part.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return false;
        }
    }
    parts.next().is_none()
}

fn session_id_from_path(source_file: &str) -> String {
    let Some(name) = Path::new(source_file)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return String::new();
    };
    let normalized = name.strip_suffix(".zst").unwrap_or(name);
    for reason in ["reset", "deleted"] {
        let marker = format!(".jsonl.{reason}.");
        if let Some(index) = normalized.find(&marker) {
            if has_archive_suffix(normalized, reason) {
                return normalized[..index].to_string();
            }
        }
    }
    normalized
        .strip_suffix(".jsonl")
        .unwrap_or(normalized)
        .to_string()
}
