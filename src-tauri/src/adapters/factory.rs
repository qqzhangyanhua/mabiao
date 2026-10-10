use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::project::decode_dashed_dir;
use crate::adapters::{discover_suffix, finish, i64_field, text_field};
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

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let content = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
    serde_json::from_str::<serde::de::IgnoredAny>(content).map_err(|error| error.to_string())?;
    let custom = load_custom_models(path);
    let calling = read_calling_session_id(path);
    Ok(parse_factory_session(
        content,
        path.to_string_lossy().as_ref(),
        custom.as_ref(),
        calling.as_deref(),
    ))
}

pub fn parse_factory_settings(content: &str, source_file: &str) -> Vec<UsageRecord> {
    parse_factory_session(content, source_file, None, None)
}

fn parse_factory_session(
    content: &str,
    source_file: &str,
    custom_models: Option<&HashMap<String, String>>,
    calling: Option<&str>,
) -> Vec<UsageRecord> {
    let value: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let usage = match value.get("tokenUsage") {
        Some(v) if !v.is_null() => v.clone(),
        _ => return Vec::new(),
    };
    let file_name = std::path::Path::new(source_file)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let file_session_id = file_name
        .strip_suffix(".settings.json")
        .unwrap_or(file_name)
        .to_string();
    let session_id = match calling {
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
    vec![finish(UsageRecord {
        occurred_at: text_field(&value, &["providerLockTimestamp"]),
        source: Source::Factory,
        model: resolve_model(&text_field(&value, &["model"]), custom_models),
        provider: text_field(&value, &["providerLock"]),
        project,
        session_id,
        source_file: source_file.to_string(),
        input_tokens: i64_field(&usage, &["inputTokens"]),
        output_tokens: i64_field(&usage, &["outputTokens"]),
        cache_read_tokens: i64_field(&usage, &["cacheReadTokens"]),
        cache_creation_tokens: i64_field(&usage, &["cacheCreationTokens"]),
        reasoning_tokens: i64_field(&usage, &["thinkingTokens"]),
        total_tokens: 0,
        native_cost: None,
    })]
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

fn read_calling_session_id(settings_path: &Path) -> Option<String> {
    let name = settings_path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".settings.json")?;
    let jsonl = settings_path.with_file_name(format!("{stem}.jsonl"));
    let file = std::fs::File::open(jsonl).ok()?;
    let first = std::io::BufReader::new(file).lines().next()?.ok()?;
    let value: Value = serde_json::from_str(first.trim()).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("session_start") {
        return None;
    }
    let calling = value
        .get("callingSessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    Some(calling.to_string())
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
