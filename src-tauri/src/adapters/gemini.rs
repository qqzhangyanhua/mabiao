use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{finish, has_billable_tokens, i64_field, parse_whole_json, text_field};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "GEMINI_DATA_DIR", ".gemini/tmp", "")
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        for path in ingest::walk_files(root, "json")? {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .starts_with("session-")
            {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_whole_json(path, parse_gemini_session)
}

pub fn parse_gemini_session(content: &str, source_file: &str) -> Vec<UsageRecord> {
    let value: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let session_id = text_field(&value, &["sessionId"]);
    let project = project_from_path(source_file);
    let messages = value
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    messages
        .into_iter()
        .filter(|msg| msg.get("type").and_then(|v| v.as_str()) == Some("gemini"))
        .filter_map(|msg| {
            let tokens = msg.get("tokens")?.clone();
            if !tokens.is_object() {
                return None;
            }
            let record = finish(UsageRecord {
                occurred_at: text_field(&msg, &["timestamp"]),
                source: Source::Gemini,
                model: text_field(&msg, &["model"]),
                provider: String::new(),
                project: project.clone(),
                session_id: session_id.clone(),
                source_file: source_file.to_string(),
                input_tokens: i64_field(&tokens, &["input"]),
                output_tokens: i64_field(&tokens, &["output"]),
                cache_read_tokens: i64_field(&tokens, &["cached"]),
                cache_creation_tokens: 0,
                reasoning_tokens: i64_field(&tokens, &["thoughts"]),
                total_tokens: i64_field(&tokens, &["total"]),
                native_cost: None,
            });
            has_billable_tokens(&record).then_some(record)
        })
        .collect()
}

fn project_from_path(source_file: &str) -> String {
    std::path::Path::new(source_file)
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
}
