use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, parse_streaming_jsonl,
    text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// 只读用户用 `--analytics-log` 主动打开的 JSONL。
///
/// 默认只认 `~/.aider/analytics.jsonl`（用户自己把日志指到这里时才能扫到）。
/// `AIDER_ANALYTICS_LOG` 可改成目录或日志文件。不读、不写 `.aider.conf.yml`，
/// 也不读 `~/.aider/analytics.json`（那是 opt-in UUID，不是用量）。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "AIDER_ANALYTICS_LOG", ".aider", "")
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        if root.is_file() {
            if is_analytics_log(root) && !paths.contains(root) {
                paths.push(root.clone());
            }
            continue;
        }
        let candidate = root.join("analytics.jsonl");
        if candidate.is_file() && !paths.contains(&candidate) {
            paths.push(candidate);
        }
    }
    Ok(paths)
}

pub(crate) fn detected(dirs: &[PathBuf]) -> bool {
    discover(dirs).is_ok_and(|paths| !paths.is_empty())
}

pub fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_aider_analytics)
}

pub fn parse_aider_analytics(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    let mut records = Vec::new();
    for value in parse_jsonl_value_lines(lines()) {
        if let Some(record) = parse_event(&value, source_file) {
            records.push(record);
        }
    }
    records
}

fn parse_event(value: &Value, source_file: &str) -> Option<UsageRecord> {
    if value.get("event").and_then(Value::as_str) != Some("message_send") {
        return None;
    }
    let properties = value.get("properties")?;
    let model = usable_model(&text_field(properties, &["main_model"]))?;
    let provider = provider_from_model(&model);
    let input = i64_field(properties, &["prompt_tokens"]);
    let output = i64_field(properties, &["completion_tokens"]);
    let total = i64_field(properties, &["total_tokens"]);
    let cost = properties.get("cost").and_then(Value::as_f64);
    let native_cost = cost.filter(|value| value.is_finite() && *value > 0.0);
    let session_id = text_field(value, &["user_id"]);
    let occurred_at = unix_seconds_to_rfc3339(i64_field(value, &["time"]));
    let record = finish(UsageRecord {
        occurred_at,
        source: Source::Aider,
        model,
        provider,
        project: String::new(),
        session_id,
        source_file: source_file.to_string(),
        input_tokens: input.max(0),
        output_tokens: output.max(0),
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        cache_creation_1h_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: total.max(0),
        native_cost,
    });
    has_billable_tokens(&record).then_some(record)
}

fn usable_model(model: &str) -> Option<String> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    if trimmed.ends_with("/REDACTED") {
        return None;
    }
    Some(trimmed.to_string())
}

fn provider_from_model(model: &str) -> String {
    model
        .split_once('/')
        .map(|(provider, _)| provider.to_string())
        .unwrap_or_default()
}

fn is_analytics_log(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
}

fn unix_seconds_to_rfc3339(started_at: i64) -> String {
    if started_at <= 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp(started_at, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}
