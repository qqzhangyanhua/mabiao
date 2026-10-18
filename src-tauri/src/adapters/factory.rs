use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::project::decode_dashed_dir;
use crate::adapters::{
    discover_suffix, finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, text_field,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(
        overrides,
        home,
        "FACTORY_SESSIONS_DIR",
        ".factory/sessions",
        "",
    )
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    discover_suffix(roots, ".settings.json")
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    ingest::metadata_fingerprint(&jsonl_sibling(path))
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let content = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
    serde_json::from_str::<serde::de::IgnoredAny>(content).map_err(|error| error.to_string())?;
    let custom = load_custom_models(path);
    let jsonl = scan_jsonl(&jsonl_sibling(path));
    Ok(parse_factory_session(
        content,
        path.to_string_lossy().as_ref(),
        custom.as_ref(),
        &jsonl,
    ))
}

pub fn parse_factory_settings(content: &str, source_file: &str) -> Vec<UsageRecord> {
    parse_factory_session(content, source_file, None, &JsonlScan::default())
}

fn parse_factory_session(
    content: &str,
    source_file: &str,
    custom_models: Option<&HashMap<String, String>>,
    jsonl: &JsonlScan,
) -> Vec<UsageRecord> {
    let value: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let file_name = std::path::Path::new(source_file)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let file_session_id = file_name
        .strip_suffix(".settings.json")
        .unwrap_or(file_name)
        .to_string();
    let session_id = match jsonl.calling_session_id.as_deref() {
        Some(parent) if !parent.is_empty() && parent != file_session_id => parent.to_string(),
        _ => file_session_id,
    };
    let parent = std::path::Path::new(source_file)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let project = if parent.starts_with('-') {
        decode_dashed_dir(parent)
    } else {
        String::new()
    };
    let settings_model = resolve_model(&text_field(&value, &["model"]), custom_models);
    let provider = text_field(&value, &["providerLock"]);
    let turns: Vec<UsageRecord> = jsonl
        .turns
        .iter()
        .map(|turn| {
            finish(UsageRecord {
                occurred_at: turn.occurred_at.clone(),
                source: Source::Factory,
                model: if turn.model.is_empty() {
                    settings_model.clone()
                } else {
                    resolve_model(&turn.model, custom_models)
                },
                provider: provider.clone(),
                project: project.clone(),
                session_id: session_id.clone(),
                source_file: source_file.to_string(),
                input_tokens: turn.input_tokens,
                output_tokens: turn.output_tokens,
                cache_read_tokens: turn.cache_read_tokens,
                cache_creation_tokens: turn.cache_creation_tokens,
                cache_creation_1h_tokens: 0,
                reasoning_tokens: turn.reasoning_tokens,
                total_tokens: 0,
                native_cost: None,
            })
        })
        .filter(has_billable_tokens)
        .collect();
    if !turns.is_empty() {
        return turns;
    }
    let usage = match value.get("tokenUsage") {
        Some(v) if !v.is_null() => v,
        _ => return Vec::new(),
    };
    let occurred_at = if jsonl.last_timestamp.is_empty() {
        text_field(&value, &["providerLockTimestamp"])
    } else {
        jsonl.last_timestamp.clone()
    };
    vec![finish(UsageRecord {
        occurred_at,
        source: Source::Factory,
        model: settings_model,
        provider,
        project,
        session_id,
        source_file: source_file.to_string(),
        input_tokens: i64_field(usage, &["inputTokens", "input_tokens"]),
        output_tokens: i64_field(usage, &["outputTokens", "output_tokens"]),
        cache_read_tokens: i64_field(usage, &["cacheReadTokens", "cache_read_tokens"]),
        cache_creation_tokens: i64_field(usage, &["cacheCreationTokens", "cache_creation_tokens"]),
        cache_creation_1h_tokens: 0,
        reasoning_tokens: i64_field(usage, &["thinkingTokens", "reasoning_tokens"]),
        total_tokens: 0,
        native_cost: None,
    })]
}

#[derive(Default)]
struct JsonlScan {
    calling_session_id: Option<String>,
    last_timestamp: String,
    turns: Vec<TurnUsage>,
}

struct TurnUsage {
    occurred_at: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    reasoning_tokens: i64,
}

fn jsonl_sibling(settings_path: &Path) -> PathBuf {
    let stem = settings_path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".settings.json"))
        .unwrap_or("");
    settings_path.with_file_name(format!("{stem}.jsonl"))
}

fn scan_jsonl(path: &Path) -> JsonlScan {
    let mut scan = JsonlScan::default();
    for value in parse_jsonl_value_lines(ingest::open_jsonl_lines(path)) {
        if scan.calling_session_id.is_none()
            && value.get("type").and_then(Value::as_str) == Some("session_start")
        {
            if let Some(calling) = value
                .get("callingSessionId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                scan.calling_session_id = Some(calling.to_string());
            }
        }
        let occurred_at = event_timestamp(&value);
        if !occurred_at.is_empty() {
            scan.last_timestamp = occurred_at.clone();
        }
        let Some(usage) = token_usage_object(&value) else {
            continue;
        };
        scan.turns.push(TurnUsage {
            occurred_at,
            model: event_model(&value),
            input_tokens: i64_field(usage, &["inputTokens", "input_tokens"]),
            output_tokens: i64_field(usage, &["outputTokens", "output_tokens"]),
            cache_read_tokens: i64_field(usage, &["cacheReadTokens", "cache_read_tokens"]),
            cache_creation_tokens: i64_field(
                usage,
                &["cacheCreationTokens", "cache_creation_tokens"],
            ),
            reasoning_tokens: i64_field(usage, &["thinkingTokens", "reasoning_tokens"]),
        });
    }
    scan
}

fn token_usage_object(value: &Value) -> Option<&Value> {
    [
        value.get("tokenUsage"),
        value.get("usage"),
        value.pointer("/message/tokenUsage"),
        value.pointer("/message/usage"),
    ]
    .into_iter()
    .flatten()
    .find(|candidate| is_token_usage(candidate))
}

fn is_token_usage(value: &Value) -> bool {
    value.is_object()
        && [
            "inputTokens",
            "outputTokens",
            "cacheReadTokens",
            "cacheCreationTokens",
            "thinkingTokens",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_creation_tokens",
            "reasoning_tokens",
        ]
        .iter()
        .any(|key| value.get(*key).is_some())
}

fn event_model(value: &Value) -> String {
    let top = text_field(value, &["model"]);
    if !top.is_empty() {
        return top;
    }
    value
        .get("message")
        .map(|message| text_field(message, &["model"]))
        .unwrap_or_default()
}

fn event_timestamp(value: &Value) -> String {
    for candidate in [
        value.get("timestamp"),
        value.get("created_at"),
        value.get("createdAt"),
        value.get("time"),
        value.pointer("/message/timestamp"),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(timestamp) = candidate
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return timestamp.to_string();
        }
        if let Some(milliseconds) = candidate.as_i64() {
            return chrono::DateTime::from_timestamp_millis(milliseconds)
                .map(|timestamp| timestamp.to_rfc3339())
                .unwrap_or_default();
        }
    }
    String::new()
}

/// 会话 `model`：空则回落 `droid`；`custom:<id>` 先查 `~/.factory/settings.json`
/// 的 `customModels`，找不到再剥 Droid 加在 id 尾上的 `-[slot]-N`。
fn resolve_model(raw: &str, custom_models: Option<&HashMap<String, String>>) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "droid".to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("custom:") {
        if let Some(map) = custom_models {
            if let Some(mapped) = map.get(trimmed).or_else(|| map.get(rest)) {
                if !mapped.is_empty() {
                    return mapped.clone();
                }
            }
        }
        return strip_droid_slot(rest);
    }
    trimmed.to_string()
}

/// `Kimi-K2-[Groq]-0` → `Kimi-K2`。不引入 regex crate。
fn strip_droid_slot(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_digit() {
        end -= 1;
    }
    if end == bytes.len() || end == 0 || bytes[end - 1] != b'-' {
        return name.to_string();
    }
    end -= 1;
    if end > 0 && bytes[end - 1] == b']' {
        if let Some(start) = name[..end].rfind("-[") {
            end = start;
        }
    }
    name[..end].to_string()
}

fn factory_settings_path(session_file: &Path) -> Option<PathBuf> {
    for ancestor in session_file.ancestors() {
        if ancestor.file_name().and_then(|name| name.to_str()) == Some("sessions") {
            return Some(ancestor.parent()?.join("settings.json"));
        }
    }
    None
}

fn load_custom_models(session_file: &Path) -> Option<HashMap<String, String>> {
    let path = factory_settings_path(session_file)?;
    let raw = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let models = value.get("customModels")?;
    let mut map = HashMap::new();
    match models {
        Value::Array(items) => {
            for item in items {
                let Some(id) = item.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(model) = item.get("model").and_then(Value::as_str) else {
                    continue;
                };
                if !id.is_empty() && !model.is_empty() {
                    map.insert(id.to_string(), model.to_string());
                }
            }
        }
        Value::Object(obj) => {
            for (id, item) in obj {
                let model = item
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !id.is_empty() && !model.is_empty() {
                    map.insert(id.clone(), model.to_string());
                }
            }
        }
        _ => {}
    }
    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

#[cfg(test)]
mod slot_tests {
    use super::strip_droid_slot;

    #[test]
    fn strips_optional_bracket_slot_and_trailing_index() {
        assert_eq!(strip_droid_slot("Kimi-K2-[Groq]-0"), "Kimi-K2");
        assert_eq!(strip_droid_slot("Kimi-K2-3"), "Kimi-K2");
        assert_eq!(strip_droid_slot("plain"), "plain");
    }
}
