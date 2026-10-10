use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, parse_streaming_jsonl,
    parse_whole_json, text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "GEMINI_DATA_DIR", ".gemini/tmp", "")
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        for extension in ["json", "jsonl"] {
            for path in ingest::walk_files(root, extension)? {
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
    }
    Ok(paths)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
    {
        parse_streaming_jsonl(path, parse_gemini_jsonl)
    } else {
        parse_whole_json(path, parse_gemini_session)
    }
}

pub fn parse_gemini_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    let snapshot = fold_gemini_jsonl(parse_jsonl_value_lines(lines()));
    parse_gemini_session(&snapshot.to_string(), source_file)
}

pub fn parse_gemini_session(content: &str, source_file: &str) -> Vec<UsageRecord> {
    let value: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    records_from_snapshot(&value, source_file)
}

fn records_from_snapshot(value: &Value, source_file: &str) -> Vec<UsageRecord> {
    let session_id = text_field(value, &["sessionId"]);
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

fn fold_gemini_jsonl(lines: impl Iterator<Item = Value>) -> Value {
    let mut session_id = String::new();
    let mut order: Vec<String> = Vec::new();
    let mut messages: HashMap<String, Value> = HashMap::new();

    for value in lines {
        apply_gemini_line(&value, &mut session_id, &mut order, &mut messages);
    }

    json!({
        "sessionId": session_id,
        "messages": order.iter().filter_map(|id| messages.get(id).cloned()).collect::<Vec<_>>(),
    })
}

fn apply_gemini_line(
    value: &Value,
    session_id: &mut String,
    order: &mut Vec<String>,
    messages: &mut HashMap<String, Value>,
) {
    let incoming_session = text_field(value, &["sessionId"]);
    if !incoming_session.is_empty() {
        *session_id = incoming_session;
    }
    if let Some(list) = value.get("messages").and_then(|v| v.as_array()) {
        for message in list {
            put_message(message, order, messages);
        }
    }
    if let Some(set) = value.get("$set") {
        let set_session = text_field(set, &["sessionId"]);
        if !set_session.is_empty() {
            *session_id = set_session;
        }
        if let Some(list) = set.get("messages").and_then(|v| v.as_array()) {
            order.clear();
            messages.clear();
            for message in list {
                put_message(message, order, messages);
            }
        }
    }
    if let Some(rewind) = value.get("$rewindTo").and_then(|v| v.as_str()) {
        if let Some(index) = order.iter().position(|id| id == rewind) {
            for id in order.split_off(index + 1) {
                messages.remove(&id);
            }
        }
    }
    if let Some(patch) = value.get("$patch") {
        if let Some(id) = patch.get("id").and_then(|v| v.as_str()) {
            if let Some(existing) = messages.get_mut(id) {
                if let Some(content) = patch.get("content") {
                    existing
                        .as_object_mut()
                        .map(|object| object.insert("content".into(), content.clone()));
                }
            }
        }
        if let Some(remove) = patch.get("removeIDs").and_then(|v| v.as_array()) {
            for id in remove.iter().filter_map(|v| v.as_str()) {
                messages.remove(id);
                order.retain(|kept| kept != id);
            }
        }
    }
    if value.get("id").and_then(|v| v.as_str()).is_some()
        && value.get("type").and_then(|v| v.as_str()).is_some()
    {
        put_message(value, order, messages);
    }
}

fn put_message(message: &Value, order: &mut Vec<String>, messages: &mut HashMap<String, Value>) {
    let id = text_field(message, &["id"]);
    if id.is_empty() {
        return;
    }
    if !messages.contains_key(&id) {
        order.push(id.clone());
    }
    messages.insert(id, message.clone());
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
