//! Buddy JSONL 家族消耗记录（WorkBuddy / CodeBuddy 共用）。
//!
//! 两个来源共用目录布局（`<root>/projects/**/*.jsonl` + 同名 `.meta.json` 侧车）、
//! 流式按行解析与按消息 ID 末条覆盖去重；各自独有的字段映射与过滤条件通过
//! [`BuddyUsageProfile`] 参数化，确保归一化输出逐字节不变。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest;

pub(crate) struct BuddyUsageProfile {
    pub source: Source,
    pub skip_run_and_incomplete: bool,
    pub dedupe_provider_keys: &'static [&'static str],
    pub dedupe_fallback_root_id: bool,
    pub use_raw_and_message_usage: bool,
    pub cache_read_direct_keys: &'static [&'static str],
    pub cache_write_keys: &'static [&'static str],
    pub input_miss_keys: &'static [&'static str],
    pub raw_input_keys: &'static [&'static str],
    pub output_keys: &'static [&'static str],
    pub reasoning_keys: &'static [&'static str],
    pub model_fallback_message: bool,
}

pub(crate) fn sidecar_fingerprint(path: &Path) -> String {
    match meta_beside(path) {
        Some(meta) if meta.exists() => ingest::metadata_fingerprint(&meta),
        _ => String::new(),
    }
}

pub(crate) fn parse_jsonl(
    lines: &LineFactory<'_>,
    source_file: &str,
    profile: &BuddyUsageProfile,
) -> Vec<UsageRecord> {
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
        let Some(record) = record_from_line(
            &value,
            source_file,
            &meta_project,
            &session_fallback,
            profile,
        ) else {
            continue;
        };
        let key = dedupe_key(&value, profile, &mut anonymous);
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

fn dedupe_key(value: &Value, profile: &BuddyUsageProfile, anonymous: &mut usize) -> String {
    let provider = value.get("providerData").unwrap_or(&Value::Null);
    let from_provider = text_field(provider, profile.dedupe_provider_keys);
    if !from_provider.is_empty() {
        return from_provider;
    }
    if profile.dedupe_fallback_root_id {
        let from_root = text_field(value, &["id"]);
        if !from_root.is_empty() {
            return from_root;
        }
    }
    *anonymous += 1;
    format!("anon:{anonymous}")
}

fn record_from_line(
    value: &Value,
    source_file: &str,
    meta_project: &str,
    session_fallback: &str,
    profile: &BuddyUsageProfile,
) -> Option<UsageRecord> {
    let provider = value.get("providerData").unwrap_or(&Value::Null);
    if profile.skip_run_and_incomplete {
        if provider.get("skipRun").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        if let Some(status) = value.get("status").and_then(Value::as_str) {
            if status != "completed" {
                return None;
            }
        }
    }
    let usage = usage_object(value, provider, profile)?;
    let direct_cache_read = if profile.cache_read_direct_keys.is_empty() {
        0
    } else {
        i64_field(usage, profile.cache_read_direct_keys)
    };
    let cache_read = direct_cache_read
        + details_sum(usage.get("inputTokensDetails"))
        + details_sum(usage.get("input_tokens_details"));
    let cache_write = i64_field(usage, profile.cache_write_keys);
    let input = match first_present_i64(usage, profile.input_miss_keys) {
        Some(miss) => miss.max(0),
        None => {
            let raw_input = i64_field(usage, profile.raw_input_keys);
            (raw_input - cache_read - cache_write).max(0)
        }
    };
    let model = {
        let from_provider = text_field(provider, &["model", "requestModelId"]);
        if from_provider.is_empty() && profile.model_fallback_message {
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
    let reasoning_tokens = if profile.reasoning_keys.is_empty() {
        0
    } else {
        i64_field(usage, profile.reasoning_keys)
    };
    let record = finish(UsageRecord {
        occurred_at: occurred_at_from_ts(i64_field(value, &["timestamp"])),
        source: profile.source,
        model,
        provider: String::new(),
        project,
        session_id,
        source_file: source_file.to_string(),
        input_tokens: input,
        output_tokens: i64_field(usage, profile.output_keys),
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_write,
        cache_creation_1h_tokens: 0,
        reasoning_tokens,
        total_tokens: 0,
        native_cost: None,
    });
    has_billable_tokens(&record).then_some(record)
}

fn usage_object<'a>(
    value: &'a Value,
    provider: &'a Value,
    profile: &BuddyUsageProfile,
) -> Option<&'a Value> {
    if !profile.use_raw_and_message_usage {
        return provider.get("usage").filter(|usage| usage.is_object());
    }
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
