use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::project::decode_dashed_dir;
use crate::adapters::{discover_suffix, finish, i64_field, parse_whole_json, text_field};
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
    parse_whole_json(path, parse_factory_settings)
}

pub fn parse_factory_settings(content: &str, source_file: &str) -> Vec<UsageRecord> {
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
    let session_id = file_name
        .strip_suffix(".settings.json")
        .unwrap_or(file_name)
        .to_string();
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
        model: String::new(),
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
