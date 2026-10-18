//! Amp 对话记录：`threads/<id>.json` 的 `messages` 数组。
//!
//! 用户/助手走 Anthropic 形态 content 块（text / thinking / tool_use / tool_result）。
//! 当前 Amp CLI 以云端线程为准，本机文件来自仍落盘的本地优先版本，或
//! `amp threads export` 导出件。

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::toolbox::*;
use super::{single_detail, ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::amp::{file_uri_to_path, millis_to_rfc3339, parse_occurred_at};
use crate::domain::Source;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::amp::discover(roots)
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
        std::fs::File::open(path).map_err(|error| format!("读取原始文件失败：{error}"))?,
    )
    .map_err(|error| format!("JSON 无效：{error}"))?;
    let session_id = first_text(&root, &["id"]);
    if session_id.is_empty() {
        return Err("缺少 Amp 线程 id".to_string());
    }
    let title = first_text(&root, &["title"]);
    let project = root
        .pointer("/env/initial/trees/0/uri")
        .and_then(Value::as_str)
        .and_then(file_uri_to_path)
        .unwrap_or_default();
    let created = millis_to_rfc3339(root.get("created").and_then(Value::as_i64).unwrap_or(0));
    let mut model = String::new();
    let mut started_at = created.clone();
    let mut ended_at = created;
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let raw_messages = root
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for (index, message) in raw_messages.iter().enumerate() {
        if !message.is_object() {
            continue;
        }
        let timestamp = message_timestamp(message);
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        let usage = message.get("usage").cloned().unwrap_or(Value::Null);
        let next_model = non_empty_text(&usage, &["model"])
            .or_else(|| non_empty_text(message, &["model"]))
            .unwrap_or_default();
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
        let role = first_text(message, &["role"]);
        let content = message.get("content").cloned().unwrap_or(Value::Null);
        if matches!(role.as_str(), "user" | "assistant") {
            push_projected_message(
                index,
                &timestamp,
                &role,
                &projected_content(&content),
                message.clone(),
                &mut messages,
                &mut events,
            );
        }
        push_content_events(
            index,
            &timestamp,
            &content,
            include_deferred_content,
            &mut events,
        );
    }

    finish_source_conversation(
        Source::Amp,
        path,
        session_id,
        title,
        project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        true,
        ConversationFinishPrep {
            native_ids: NativeEventIdPrep::Skip,
            degradation: CapabilityDegradationPrep::Inferred {
                sequence: raw_messages.len().saturating_sub(1),
            },
        },
    )
}

fn push_content_events(
    index: usize,
    timestamp: &str,
    content: &Value,
    include_deferred_content: bool,
    events: &mut Vec<ConversationEvent>,
) {
    let items = match content {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![content.clone()],
        _ => return,
    };
    for item in items {
        match item.get("type").and_then(Value::as_str).unwrap_or("") {
            "tool_use" | "tool_call" => events.push(semantic_event(
                index,
                EventKind::ToolCall,
                timestamp,
                Some(EventActor::Assistant),
                optional_text(&item, &["name"]),
                item.get("input").map(Value::to_string),
                normalize_tool_call_details(&item),
            )),
            "tool_result" => events.push(tool_result_event(
                index,
                timestamp,
                &normalize_tool_result_details(&item),
                include_deferred_content,
            )),
            "thinking" => {
                let text = non_empty_text(&item, &["thinking", "text"]);
                events.push(semantic_event(
                    index,
                    EventKind::Plan,
                    timestamp,
                    Some(EventActor::Assistant),
                    None,
                    text,
                    item,
                ));
            }
            _ => {}
        }
    }
}

fn projected_content(content: &Value) -> Value {
    match content {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .filter(|item| {
                    matches!(
                        item.get("type").and_then(Value::as_str).unwrap_or("text"),
                        "text" | "thinking"
                    )
                })
                .cloned()
                .collect(),
        ),
        other => other.clone(),
    }
}

fn message_timestamp(message: &Value) -> String {
    let usage = message.get("usage").cloned().unwrap_or(Value::Null);
    let from_usage = parse_occurred_at(&first_text(&usage, &["timestamp"]));
    if !from_usage.is_empty() {
        return from_usage;
    }
    let from_message = parse_occurred_at(&first_text(message, &["timestamp"]));
    if !from_message.is_empty() {
        return from_message;
    }
    millis_to_rfc3339(
        message
            .pointer("/meta/sentAt")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    )
}

fn non_empty_text(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = value.get(*key).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}
