use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, parse_streaming_jsonl,
    text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// CodeBuddy CLI 会话：`$CODEBUDDY_DIR`，否则 `~/.codebuddy`，扫 `projects/**/*.jsonl`。
/// 目录布局像 Claude Code，行格式是 OpenAI Responses 形态，用量在
/// `providerData.rawUsage` / `providerData.usage`（`prompt_tokens` 含 cache）。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides
        .get("CODEBUDDY_DIR")
        .cloned()
        .unwrap_or_else(|| vec![home.join(".codebuddy")])
        .into_iter()
        .map(|root| root.join("projects"))
        .collect()
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    match meta_beside(path) {
        Some(meta) if meta.exists() => ingest::metadata_fingerprint(&meta),
        _ => String::new(),
    }
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_codebuddy_jsonl)
}

pub fn parse_codebuddy_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    let meta_project = read_meta_cwd(source_file);
    let session_fallback = Path::new(source_file)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();

    let mut order = Vec::new();
    let mut by_id: HashMap<String, UsageRecord> = HashMap::new();
    let mut anonymous = 0usize;
    for value in parse_jsonl_value_lines(lines()) {
        let Some(record) = record_from_line(&value, source_file, &meta_project, &session_fallback)
        else {
            continue;
        };
        let key = {
            let provider = value.get("providerData").cloned().unwrap_or(Value::Null);
            let id = text_field(&provider, &["messageId", "traceId"]);
            if id.is_empty() {
                let id = text_field(&value, &["id"]);
                if id.is_empty() {
                    anonymous += 1;
                    format!("anon:{anonymous}")
                } else {
                    id
                }
            } else {
                id
            }
        };
        if !by_id.contains_key(&key) {
            order.push(key.clone());
        }
        by_id.insert(key, record);
    }
    order
        .into_iter()
        .filter_map(|key| by_id.remove(&key))
        .collect()
}

fn record_from_line(
    value: &Value,
    source_file: &str,
    meta_project: &str,
    session_fallback: &str,
) -> Option<UsageRecord> {
    let provider = value.get("providerData").cloned().unwrap_or(Value::Null);
    if provider.get("skipRun").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    if let Some(status) = value.get("status").and_then(Value::as_str) {
        if status != "completed" {
            return None;
        }
    }
    let usage = usage_object(value, &provider)?;
    let cache_read = i64_field(
        usage,
        &[
            "cache_read_input_tokens",
            "cacheReadInputTokens",
            "prompt_cache_hit_tokens",
            "cached_tokens",
            "cacheTokens",
        ],
    ) + details_sum(usage.get("inputTokensDetails"))
        + details_sum(usage.get("input_tokens_details"));
    let cache_write = i64_field(
        usage,
        &[
            "cache_creation_input_tokens",
            "cacheCreationInputTokens",
            "prompt_cache_write_tokens",
            "cachedWriteTokens",
        ],
    );
    let input = match first_present_i64(
        usage,
        &[
            "cachedMissTokens",
            "cacheMissTokens",
            "prompt_cache_miss_tokens",
        ],
    ) {
        Some(miss) => miss.max(0),
        None => {
            let raw_input = i64_field(usage, &["prompt_tokens", "inputTokens", "input_tokens"]);
            (raw_input - cache_read - cache_write).max(0)
        }
    };
    let model = {
        let from_provider = text_field(&provider, &["model", "requestModelId"]);
        if from_provider.is_empty() {
            value
                .get("message")
                .map(|message| text_field(message, &["model"]))
                .unwrap_or_default()
        } else {
            from_provider
        }
    };
    if model.is_empty() {
        return None;
    }
    let project = {
        let cwd = text_field(value, &["cwd"]);
        if cwd.is_empty() {
            meta_project.to_string()
        } else {
            cwd
        }
    };
    let session_id = {
        let id = text_field(value, &["sessionId"]);
        if id.is_empty() {
            session_fallback.to_string()
        } else {
            id
        }
    };
    let record = finish(UsageRecord {
        occurred_at: occurred_at_from_ts(i64_field(value, &["timestamp"])),
        source: Source::CodeBuddy,
        model,
        provider: String::new(),
        project,
        session_id,
        source_file: source_file.to_string(),
        input_tokens: input,
        output_tokens: i64_field(
            usage,
            &["completion_tokens", "outputTokens", "output_tokens"],
        ),
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_write,
        cache_creation_1h_tokens: 0,
        reasoning_tokens: i64_field(
            usage,
            &[
                "completion_thinking_tokens",
                "completionThinkingTokens",
                "reasoningTokens",
            ],
        ),
        total_tokens: 0,
        native_cost: None,
    });
    has_billable_tokens(&record).then_some(record)
}

fn usage_object<'a>(value: &'a Value, provider: &'a Value) -> Option<&'a Value> {
    for candidate in [
        provider.get("rawUsage"),
        provider.get("usage"),
        value
            .get("message")
            .and_then(|message| message.get("usage")),
    ] {
        if let Some(usage) = candidate.filter(|value| value.is_object()) {
            return Some(usage);
        }
    }
    None
}

fn first_present_i64(value: &Value, keys: &[&str]) -> Option<i64> {
    for key in keys {
        if value.get(*key).is_some() {
            return Some(i64_field(value, &[key]));
        }
    }
    None
}

fn details_sum(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| i64_field(item, &["cached_tokens"]))
            .sum(),
        Some(object @ Value::Object(_)) => i64_field(object, &["cached_tokens"]),
        _ => 0,
    }
}

fn meta_beside(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_str()?;
    Some(path.parent()?.join(format!("{stem}.meta.json")))
}

fn read_meta_cwd(source_file: &str) -> String {
    let Some(meta) = meta_beside(Path::new(source_file)) else {
        return String::new();
    };
    let Ok(text) = std::fs::read_to_string(meta) else {
        return String::new();
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .map(|value| text_field(&value, &["cwd"]))
        .unwrap_or_default()
}

fn occurred_at_from_ts(ts: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ts)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}
