use std::path::Path;

use serde_json::Value;

use super::toolbox::*;

pub(super) fn parse(
    path: &Path,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    parse_from_values(
        path,
        parse_jsonl_conversation_values(path)?,
        include_deferred_content,
        None,
    )
}

pub(super) fn parse_from_values(
    path: &Path,
    values: Vec<(usize, Value)>,
    include_deferred_content: bool,
    session_hint: Option<&str>,
) -> Result<ParsedConversation, String> {
    let mut session_id = String::new();
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
            "model_change" => {
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
            "message" => {
                let message = value.get("message").unwrap_or(&Value::Null);
                let role = first_text(message, &["role"]);
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
            kind => events.push(event_msg_semantic_event(index, &timestamp, kind, &value)),
        }
    }
    if session_id.is_empty() {
        session_id = session_hint
            .map(str::to_string)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("")
                    .to_string()
            });
    }
    finish_source_conversation(
        Source::Pi,
        path,
        session_id,
        String::new(),
        project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        true,
    )
}
