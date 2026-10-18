use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, parse_streaming_jsonl,
    text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// Windsurf / Devin Desktop 把 ACP 事件写成 `User/acp-events/*.ndjson`。
///
/// 只读 `sessionUpdate=usage_update` 的 `_meta`：
/// `cognition.ai/inputTokens`（累计，含 cache read）、`cachedReadTokens`、
/// `cachedWriteTokens`、`outputTokens`（逐步相加）。无模型名则跳过。
/// `WINDSURF_ACP_HOME` 整体替换，指向 `acp-events` 目录。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides
        .get("WINDSURF_ACP_HOME")
        .cloned()
        .unwrap_or_else(|| {
            ["Devin", "Windsurf"]
                .into_iter()
                .flat_map(|app| {
                    [
                        home.join(format!(".config/{app}/User/acp-events")),
                        home.join(format!("Library/Application Support/{app}/User/acp-events")),
                        home.join(format!("AppData/Roaming/{app}/User/acp-events")),
                    ]
                })
                .collect()
        })
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        if root.is_file() {
            if is_ndjson(root) && !paths.contains(root) {
                paths.push(root.clone());
            }
            continue;
        }
        for path in ingest::walk_files(root, "ndjson")? {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

pub fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_windsurf_ndjson)
}

pub fn parse_windsurf_ndjson(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    let session_id = session_id_from_path(source_file);
    let mut input = 0_i64;
    let mut cache_read = 0_i64;
    let mut cache_write = 0_i64;
    let mut output = 0_i64;
    let mut model = String::new();
    let mut occurred_at = String::new();
    let mut saw_usage = false;
    for value in parse_jsonl_value_lines(lines()) {
        let Some(notification) = notification_of(&value) else {
            continue;
        };
        if text_field(&notification, &["sessionUpdate"]) != "usage_update" {
            continue;
        }
        let Some(meta) = notification.get("_meta") else {
            continue;
        };
        let next_input = optional_tokens(meta, "cognition.ai/inputTokens");
        let next_cache_read = optional_tokens(meta, "cognition.ai/cachedReadTokens");
        let next_cache_write = optional_tokens(meta, "cognition.ai/cachedWriteTokens");
        let next_output = optional_tokens(meta, "cognition.ai/outputTokens");
        if next_input.is_none()
            && next_cache_read.is_none()
            && next_cache_write.is_none()
            && next_output.is_none()
        {
            continue;
        }
        saw_usage = true;
        if let Some(value) = next_input {
            input = value;
        }
        if let Some(value) = next_cache_read {
            cache_read = value;
        }
        if let Some(value) = next_cache_write {
            cache_write = value;
        }
        if let Some(value) = next_output {
            output = output.saturating_add(value);
        }
        if model.is_empty() {
            model = usable_model(&notification, meta);
        }
        if let Some(ts) = event_time(&notification) {
            occurred_at = ts;
        }
    }
    if !saw_usage {
        return Vec::new();
    }
    let Some(model) = usable_model_name(&model) else {
        return Vec::new();
    };
    let uncached_input = input.saturating_sub(cache_read).max(0);
    let record = finish(UsageRecord {
        occurred_at,
        source: Source::Windsurf,
        model,
        provider: "cognition".to_string(),
        project: String::new(),
        session_id,
        source_file: source_file.to_string(),
        input_tokens: uncached_input,
        output_tokens: output.max(0),
        cache_read_tokens: cache_read.max(0),
        cache_creation_tokens: cache_write.max(0),
        cache_creation_1h_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: 0,
        native_cost: None,
    });
    has_billable_tokens(&record)
        .then_some(record)
        .into_iter()
        .collect()
}

fn usable_model(notification: &Value, meta: &Value) -> String {
    let from_meta = text_field(meta, &["cognition.ai/model"]);
    if !from_meta.is_empty() {
        return from_meta;
    }
    let nested = notification
        .pointer("/content/metadata/generation_model")
        .or_else(|| notification.pointer("/metadata/generation_model"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    nested.to_string()
}

fn usable_model_name(model: &str) -> Option<String> {
    let trimmed = model.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("adaptive") {
        return None;
    }
    Some(trimmed.to_string())
}

fn notification_of(value: &Value) -> Option<Value> {
    value.get("notification").cloned()
}

fn optional_tokens(meta: &Value, key: &str) -> Option<i64> {
    meta.get(key).map(|_| i64_field(meta, &[key]).max(0))
}

fn event_time(notification: &Value) -> Option<String> {
    let raw = notification
        .pointer("/content/metadata/created_at")
        .or_else(|| notification.pointer("/metadata/created_at"))
        .or_else(|| notification.get("created_at"))
        .or_else(|| notification.get("timestamp"))
        .and_then(Value::as_str)?
        .trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&chrono::Utc).to_rfc3339());
    }
    None
}

fn session_id_from_path(source_file: &str) -> String {
    Path::new(source_file)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn is_ndjson(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ndjson"))
}
