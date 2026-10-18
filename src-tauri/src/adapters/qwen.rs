use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::project::decode_url_dir;
use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, parse_streaming_jsonl,
    parse_whole_json, text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "QWEN_DATA_DIR", ".qwen", "")
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        for path in ingest::walk_files(root, "json")? {
            if path.file_name().and_then(|name| name.to_str()) == Some("logs.json") {
                paths.push(path);
            }
        }
        for path in ingest::walk_files(root, "jsonl")? {
            if is_chat_jsonl(&path) {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
    {
        parse_streaming_jsonl(path, parse_qwen_jsonl)
    } else {
        parse_whole_json(path, parse_qwen_session)
    }
}

/// `~/.qwen/tmp/*/logs.json` 只有 user 文本，没有 token。
pub fn parse_qwen_session(_content: &str, _source_file: &str) -> Vec<UsageRecord> {
    Vec::new()
}

/// `~/.qwen/projects/**/chats/*.jsonl` 的 `usageMetadata`（Gemini generateContent 形态）。
pub fn parse_qwen_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    let project = project_from_chat_path(source_file);
    let fallback_session = path_session_id(source_file);
    parse_jsonl_value_lines(lines())
        .filter_map(|value| parse_usage_line(&value, source_file, &project, &fallback_session))
        .collect()
}

fn parse_usage_line(
    value: &Value,
    source_file: &str,
    project: &str,
    fallback_session: &str,
) -> Option<UsageRecord> {
    let meta = value.get("usageMetadata")?;
    if !meta.is_object() {
        return None;
    }
    let cached = i64_field(meta, &["cachedContentTokenCount"]).max(0);
    let mut session_id = text_field(value, &["sessionId", "session_id"]);
    if session_id.is_empty() {
        session_id = fallback_session.to_string();
    }
    let record = finish(UsageRecord {
        occurred_at: text_field(value, &["timestamp"]),
        source: Source::Qwen,
        model: text_field(value, &["model"]),
        provider: String::new(),
        project: project.to_string(),
        session_id,
        source_file: source_file.to_string(),
        input_tokens: exclusive_qwen_input(meta, cached),
        output_tokens: i64_field(meta, &["candidatesTokenCount"]).max(0),
        cache_read_tokens: cached,
        cache_creation_tokens: 0,
        cache_creation_1h_tokens: 0,
        reasoning_tokens: i64_field(meta, &["thoughtsTokenCount"]).max(0),
        total_tokens: i64_field(meta, &["totalTokenCount"]).max(0),
        native_cost: None,
    });
    has_billable_tokens(&record).then_some(record)
}

/// `promptTokenCount` 常常已经含 `cachedContentTokenCount`。对齐 Gemini：
/// `totalTokenCount == prompt+candidates`（或再加 thoughts）时从 input 减去 cache。
fn exclusive_qwen_input(meta: &Value, cached: i64) -> i64 {
    let input = i64_field(meta, &["promptTokenCount"]).max(0);
    if cached <= 0 {
        return input;
    }
    let output = i64_field(meta, &["candidatesTokenCount"]).max(0);
    let thoughts = i64_field(meta, &["thoughtsTokenCount"]).max(0);
    let inclusive = input + output;
    let inclusive_with_thoughts = inclusive + thoughts;
    let total = meta.get("totalTokenCount").and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().map(|n| n as i64))
            .or_else(|| value.as_f64().map(|n| n.round() as i64))
    });
    let should_subtract = match total {
        Some(total) if total > 0 => {
            (total == inclusive || total == inclusive_with_thoughts)
                && total != inclusive + cached
                && total != inclusive_with_thoughts + cached
        }
        _ => true,
    };
    if should_subtract {
        input.saturating_sub(cached.min(input))
    } else {
        input
    }
}

pub(crate) fn is_chat_jsonl(path: &Path) -> bool {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        == Some("chats")
}

fn project_from_chat_path(source_file: &str) -> String {
    Path::new(source_file)
        .parent()
        .and_then(Path::parent)
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(decode_url_dir)
        .unwrap_or_default()
}

fn path_session_id(source_file: &str) -> String {
    Path::new(source_file)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}
