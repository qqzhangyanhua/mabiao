//! OpenClaw 对话记录：`agents/<id>/sessions/*.jsonl`（含 reset/deleted 归档）。

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::toolbox::*;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::domain::Source;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::openclaw::discover(roots)
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    super::discover::single_index(path, parse)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    super::discover::single_detail(path, session_id, include_deferred_content, parse)
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    super::discover::regular_source_revision(path)
}

fn parse(path: &Path, include_deferred_content: bool) -> Result<ParsedConversation, String> {
    parse_from_values(
        path,
        parse_jsonl_conversation_values(path)?,
        include_deferred_content,
    )
}

fn parse_from_values(
    path: &Path,
    values: Vec<(usize, Value)>,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let mut session_id = String::new();
    let mut title = String::new();
    let mut project = String::new();
    let mut model = String::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut messages = Vec::new();
    let mut events = Vec::new();

    for (index, value) in values {
        let timestamp = text_field(&value, "timestamp");
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        match value.get("type").and_then(Value::as_str).unwrap_or("") {
            "session" => {
                session_id = first_text(&value, &["id", "sessionId"]);
                project = first_text(&value, &["cwd"]);
                events.push(semantic_event(
                    index,
                    EventKind::SystemStatus,
                    &timestamp,
                    None,
                    Some("session_started".to_string()),
                    None,
                    value.clone(),
                ));
            }
            "model" | "model_change" => {
                let next_model = first_text(&value, &["modelId", "model"]);
                if !next_model.is_empty() {
                    model = next_model.clone();
                }
                events.push(semantic_event(
                    index,
                    EventKind::ModelChange,
                    &timestamp,
                    None,
                    (!next_model.is_empty()).then_some(next_model),
                    None,
                    value.clone(),
                ));
            }
            "message" | "" => {
                let message = value.get("message").unwrap_or(&Value::Null);
                let role = first_text(message, &["role"]);
                let role = if role.is_empty() {
                    first_text(&value, &["role"])
                } else {
                    role
                };
                let next_model = first_text(message, &["model", "modelId"]);
                if !next_model.is_empty() {
                    model = next_model;
                }
                let content = message.get("content").unwrap_or(&Value::Null);
                if matches!(role.as_str(), "user" | "assistant") {
                    push_projected_message(
                        index,
                        &timestamp,
                        &role,
                        content,
                        message.clone(),
                        &mut messages,
                        &mut events,
                    );
                }
                if matches!(role.as_str(), "toolResult" | "tool_result" | "tool") {
                    events.push(tool_result_event(
                        index,
                        &timestamp,
                        &normalize_tool_result_details(message),
                        include_deferred_content,
                    ));
                }
                if let Some(items) = content.as_array() {
                    for item in items {
                        match item.get("type").and_then(Value::as_str).unwrap_or("") {
                            "toolCall" | "tool_call" | "tool_use" => {
                                events.push(semantic_event(
                                    index,
                                    EventKind::ToolCall,
                                    &timestamp,
                                    Some(EventActor::Assistant),
                                    optional_text(item, &["name", "toolName"]),
                                    item.get("arguments")
                                        .or_else(|| item.get("input"))
                                        .map(Value::to_string),
                                    normalize_tool_call_details(item),
                                ));
                            }
                            "toolResult" | "tool_result" => events.push(tool_result_event(
                                index,
                                &timestamp,
                                &normalize_tool_result_details(item),
                                include_deferred_content,
                            )),
                            "thinking" => events.push(semantic_event(
                                index,
                                EventKind::Plan,
                                &timestamp,
                                Some(EventActor::Assistant),
                                None,
                                optional_text(item, &["thinking", "text"]),
                                item.clone(),
                            )),
                            _ => {}
                        }
                    }
                }
            }
            kind @ ("compaction" | "branch_summary" | "session_info") => {
                events.push(semantic_event(
                    index,
                    EventKind::SystemStatus,
                    &timestamp,
                    None,
                    Some(kind.to_string()),
                    optional_text(&value, &["summary", "message"]),
                    value.clone(),
                ));
            }
            kind @ ("title" | "title_change") => {
                let next_title = first_text(&value, &["title"]);
                if !next_title.is_empty() {
                    title = next_title;
                }
                events.push(event_msg_semantic_event(index, &timestamp, kind, &value));
            }
            kind => events.push(event_msg_semantic_event(index, &timestamp, kind, &value)),
        }
    }
    if session_id.is_empty() {
        session_id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("")
            .to_string();
    }
    finish_source_conversation(
        Source::OpenClaw,
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
        ConversationFinishPrep::NONE,
    )
}
