use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::toolbox::*;
use super::{single_detail, ConversationIndexBatch, ConversationIndexIssue};

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::discover_suffix(roots, ".messages.json")
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    parse(path, false)
        .map(|conversation| ConversationIndexBatch {
            conversations: vec![conversation],
            diagnostics: Vec::new(),
        })
        .map_err(|message| ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message,
            event_type: None,
            line: None,
        })
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, parse)
}

pub(super) fn parse(
    path: &Path,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let root: Value = serde_json::from_reader(
        fs::File::open(path).map_err(|error| format!("读取原始文件失败：{error}"))?,
    )
    .map_err(|error| format!("JSON 无效：{error}"))?;
    let messages = root
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "缺少 Cline messages 数组".to_string())?;
    let manifest = read_manifest(path);
    let forked_at = manifest.as_ref().and_then(forked_at_millis);
    let folder_id = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    let file_stem = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .strip_suffix(".messages.json")
        .unwrap_or("")
        .to_string();
    let is_top_level = file_stem == folder_id || file_stem.is_empty();
    let mut session_id = if is_top_level {
        let from_file = first_text(&root, &["sessionId"]);
        if from_file.is_empty() {
            manifest
                .as_ref()
                .map(|value| first_text(value, &["session_id"]))
                .unwrap_or_default()
        } else {
            from_file
        }
    } else {
        file_stem.clone()
    };
    if session_id.is_empty() {
        session_id = if is_top_level {
            folder_id.clone()
        } else {
            file_stem
        };
    }
    let mut title = manifest
        .as_ref()
        .and_then(|value| value.pointer("/metadata/title"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let project = manifest
        .as_ref()
        .map(|value| first_text(value, &["cwd"]))
        .unwrap_or_default();
    let mut model = String::new();
    let mut started_at = manifest
        .as_ref()
        .map(|value| first_text(value, &["started_at"]))
        .unwrap_or_default();
    let mut ended_at = started_at.clone();
    let mut projected = Vec::new();
    let mut events = Vec::new();

    for (index, message) in messages.iter().enumerate() {
        let ts = message.get("ts").and_then(Value::as_i64).unwrap_or(0);
        if forked_at.is_some_and(|since| ts < since) {
            continue;
        }
        let timestamp = millis_timestamp(ts);
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        let role = first_text(message, &["role"]);
        let model_info = message.get("modelInfo").cloned().unwrap_or(Value::Null);
        let next_model = first_text(&model_info, &["id"]);
        if !next_model.is_empty() && next_model != model {
            model = next_model.clone();
            events.push(semantic_event(
                index,
                EventKind::ModelChange,
                &timestamp,
                None,
                Some(next_model),
                None,
                message.clone(),
            ));
        }
        if matches!(role.as_str(), "user" | "assistant") {
            push_projected_message(
                index,
                &timestamp,
                &role,
                message.get("content").unwrap_or(&Value::Null),
                message.clone(),
                &mut projected,
                &mut events,
            );
        }
        if let Some(items) = message.get("content").and_then(Value::as_array) {
            for item in items {
                match item.get("type").and_then(Value::as_str).unwrap_or("") {
                    "tool_use" | "tool_call" => events.push(semantic_event(
                        index,
                        EventKind::ToolCall,
                        &timestamp,
                        Some(EventActor::Assistant),
                        optional_text(item, &["name"]),
                        item.get("input").map(Value::to_string),
                        normalize_tool_call_details(item),
                    )),
                    "tool_result" => events.push(tool_result_event(
                        index,
                        &timestamp,
                        &normalize_tool_result_details(item),
                        include_deferred_content,
                    )),
                    _ => {}
                }
            }
        }
    }
    if !is_top_level {
        events.push(semantic_event(
            0,
            EventKind::SystemStatus,
            &started_at,
            None,
            Some("session_started".to_string()),
            None,
            serde_json::json!({ "parent_id": folder_id }),
        ));
    }
    if title.is_empty() {
        title = projected
            .iter()
            .find(|message| message.role == "user")
            .map(|message| message.text.clone())
            .unwrap_or_default();
    }
    finish_source_conversation(
        Source::Cline,
        path,
        session_id,
        title,
        project,
        model,
        started_at,
        ended_at,
        projected,
        events,
        is_top_level,
        ConversationFinishPrep::NONE,
    )
}

fn read_manifest(path: &Path) -> Option<Value> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".messages.json")?;
    let manifest = path.parent()?.join(format!("{stem}.json"));
    serde_json::from_str(&fs::read_to_string(manifest).ok()?).ok()
}

fn forked_at_millis(manifest: &Value) -> Option<i64> {
    let raw = manifest.pointer("/metadata/fork/forkedAt")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn millis_timestamp(ts: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ts)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}
