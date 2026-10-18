use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{finish, has_billable_tokens, i64_field, text_field};
use crate::domain::{Source, UsageRecord};
use crate::ingest::PathOverrides;

/// Amp 把每个线程写成一份 JSON：`threads/<id>.json`。
///
/// `$AMP_DATA_DIR` 覆盖 Amp 数据根并拼 `threads/`（与 ccusage 一致）。
/// 未覆盖时扫 XDG `~/.local/share/amp/threads`，以及 macOS / Windows 的
/// Application Support / AppData 同名目录。Amp CLI 在各平台默认都写
/// `~/.local/share/amp`（不跟 OS 惯例分叉）。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides
        .get("AMP_DATA_DIR")
        .cloned()
        .map(|roots| roots.into_iter().map(|root| root.join("threads")).collect())
        .unwrap_or_else(|| {
            vec![
                home.join(".local/share/amp/threads"),
                home.join("Library/Application Support/amp/threads"),
                home.join("AppData/Roaming/amp/threads"),
            ]
        })
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::discover_suffix(roots, ".json")
}

pub fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    crate::adapters::parse_whole_json(path, parse_thread)
}

pub(crate) fn parse_thread(content: &str, source_file: &str) -> Vec<UsageRecord> {
    let Ok(root) = serde_json::from_str::<Value>(content) else {
        return Vec::new();
    };
    let session_id = text_field(&root, &["id"]);
    if session_id.is_empty() {
        return Vec::new();
    }
    let project = project_from_env(&root);
    let messages = root
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let ledger_events = root
        .get("usageLedger")
        .and_then(Value::as_object)
        .and_then(|ledger| ledger.get("events"))
        .and_then(Value::as_array);
    if let Some(events) = ledger_events {
        return parse_ledger_events(events, &messages, &session_id, &project, source_file);
    }
    parse_message_usage(&messages, &session_id, &project, source_file)
}

fn parse_ledger_events(
    events: &[Value],
    messages: &[Value],
    session_id: &str,
    project: &str,
    source_file: &str,
) -> Vec<UsageRecord> {
    let cache = cache_tokens_by_message_id(messages);
    let mut records = Vec::new();
    for event in events {
        if !event.is_object() {
            continue;
        }
        let model = text_field(event, &["model"]);
        let occurred_at = parse_occurred_at(&text_field(event, &["timestamp"]));
        if model.is_empty() || occurred_at.is_empty() {
            continue;
        }
        let tokens = event.get("tokens").cloned().unwrap_or(Value::Null);
        let mut parts = AmpTokens {
            input: i64_field(&tokens, &["input"]),
            output: i64_field(&tokens, &["output"]),
            cache_creation: 0,
            cache_read: 0,
        };
        if let Some((cache_creation, cache_read)) =
            json_id(event.get("toMessageId")).and_then(|id| cache.get(&id).copied())
        {
            parts.cache_creation = cache_creation;
            parts.cache_read = cache_read;
        }
        parts.apply_total_only_fallback(i64_field(&tokens, &["total"]));
        if let Some(record) =
            finish_record(occurred_at, model, project, session_id, source_file, parts)
        {
            records.push(record);
        }
    }
    records
}

fn parse_message_usage(
    messages: &[Value],
    session_id: &str,
    project: &str,
    source_file: &str,
) -> Vec<UsageRecord> {
    let mut records = Vec::new();
    for message in messages {
        if text_field(message, &["role"]) != "assistant" {
            continue;
        }
        let usage = match message.get("usage") {
            Some(value) if value.is_object() => value,
            _ => continue,
        };
        let model = non_empty(text_field(usage, &["model"]))
            .unwrap_or_else(|| text_field(message, &["model"]));
        let occurred_at = parse_occurred_at(
            &non_empty(text_field(usage, &["timestamp"]))
                .unwrap_or_else(|| text_field(message, &["timestamp"])),
        );
        if model.is_empty() || occurred_at.is_empty() {
            continue;
        }
        let mut parts = AmpTokens {
            input: i64_field(usage, &["inputTokens"]),
            output: i64_field(usage, &["outputTokens"]),
            cache_creation: i64_field(usage, &["cacheCreationInputTokens"]),
            cache_read: i64_field(usage, &["cacheReadInputTokens"]),
        };
        parts.apply_total_only_fallback(i64_field(usage, &["totalTokens"]));
        if let Some(record) =
            finish_record(occurred_at, model, project, session_id, source_file, parts)
        {
            records.push(record);
        }
    }
    records
}

struct AmpTokens {
    input: i64,
    output: i64,
    cache_creation: i64,
    cache_read: i64,
}

impl AmpTokens {
    fn apply_total_only_fallback(&mut self, total_only: i64) {
        if self.input <= 0
            && self.output <= 0
            && self.cache_creation <= 0
            && self.cache_read <= 0
            && total_only > 0
        {
            self.output = total_only;
        }
    }
}

fn finish_record(
    occurred_at: String,
    model: String,
    project: &str,
    session_id: &str,
    source_file: &str,
    tokens: AmpTokens,
) -> Option<UsageRecord> {
    let record = finish(UsageRecord {
        occurred_at,
        source: Source::Amp,
        model,
        provider: String::new(),
        project: project.to_string(),
        session_id: session_id.to_string(),
        source_file: source_file.to_string(),
        input_tokens: tokens.input.max(0),
        output_tokens: tokens.output.max(0),
        cache_read_tokens: tokens.cache_read.max(0),
        cache_creation_tokens: tokens.cache_creation.max(0),
        cache_creation_1h_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: 0,
        native_cost: None,
    });
    if has_billable_tokens(&record) {
        Some(record)
    } else {
        None
    }
}

fn cache_tokens_by_message_id(messages: &[Value]) -> HashMap<String, (i64, i64)> {
    let mut cache = HashMap::new();
    for message in messages {
        if text_field(message, &["role"]) != "assistant" {
            continue;
        }
        let Some(id) = json_id(message.get("messageId")) else {
            continue;
        };
        let usage = message.get("usage").cloned().unwrap_or(Value::Null);
        cache.insert(
            id,
            (
                i64_field(&usage, &["cacheCreationInputTokens"]).max(0),
                i64_field(&usage, &["cacheReadInputTokens"]).max(0),
            ),
        );
    }
    cache
}

fn project_from_env(root: &Value) -> String {
    let uri = root
        .pointer("/env/initial/trees/0/uri")
        .and_then(Value::as_str)
        .unwrap_or("");
    file_uri_to_path(uri).unwrap_or_default()
}

pub(crate) fn file_uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let decoded = percent_decode(rest);
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (from_hex(bytes[index + 1]), from_hex(bytes[index + 2]))
            {
                out.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn json_id(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub(crate) fn parse_occurred_at(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return dt.with_timezone(&chrono::Utc).to_rfc3339();
    }
    String::new()
}

pub(crate) fn millis_to_rfc3339(millis: i64) -> String {
    if millis <= 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}
