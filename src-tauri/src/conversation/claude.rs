use super::*;

pub(super) fn parse(
    path: &Path,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    parse_from_values(
        path,
        parse_jsonl_conversation_values(path)?,
        include_deferred_content,
        None,
        false,
    )
}

pub(super) fn parse_from_values(
    path: &Path,
    values: Vec<(usize, Value)>,
    include_deferred_content: bool,
    session_hint: Option<&str>,
    line_direct: bool,
) -> Result<ParsedConversation, String> {
    let mut parent_session_id = String::new();
    let mut agent_id = String::new();
    let mut project = String::new();
    let mut model = String::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let path_is_subagent = path
        .components()
        .any(|part| part.as_os_str() == "subagents");

    for (index, value) in &values {
        let timestamp = text_field(value, "timestamp");
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        if parent_session_id.is_empty() {
            parent_session_id = first_text(value, &["sessionId", "session_id"]);
        }
        if agent_id.is_empty() {
            agent_id = first_text(value, &["agentId", "agent_id"]);
        }
        if project.is_empty() {
            project = first_text(value, &["cwd"]);
        }
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let message = value.get("message").unwrap_or(&Value::Null);
        let role = first_text(message, &["role"]);
        let next_model = first_text(message, &["model"]);
        if !next_model.is_empty() && next_model != model {
            model = next_model.clone();
            events.push(semantic_event(
                *index,
                EventKind::ModelChange,
                &timestamp,
                None,
                Some(next_model),
                None,
                message.clone(),
            ));
        }
        let content = message.get("content").unwrap_or(&Value::Null);
        if matches!(role.as_str(), "user" | "assistant") {
            push_projected_message(
                *index,
                &timestamp,
                &role,
                content,
                message.clone(),
                &mut messages,
                &mut events,
            );
        }
        if let Some(items) = content.as_array() {
            for item in items {
                match item.get("type").and_then(Value::as_str).unwrap_or("") {
                    "tool_use" => events.push(semantic_event(
                        *index,
                        EventKind::ToolCall,
                        &timestamp,
                        Some(EventActor::Assistant),
                        optional_text(item, &["name"]),
                        item.get("input").map(Value::to_string),
                        normalize_tool_call_details(item),
                    )),
                    "tool_result" => events.push(tool_result_event(
                        *index,
                        &timestamp,
                        &normalize_tool_result_details(item),
                        include_deferred_content,
                    )),
                    "thinking" => events.push(semantic_event(
                        *index,
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
        if !matches!(kind, "user" | "assistant") {
            let event = if matches!(kind, "system" | "progress" | "result" | "queue-operation") {
                semantic_event(
                    *index,
                    if value.get("is_error").and_then(Value::as_bool) == Some(true) {
                        EventKind::Error
                    } else {
                        EventKind::SystemStatus
                    },
                    &timestamp,
                    None,
                    Some(kind.to_string()),
                    optional_text(value, &["result", "content", "message"]),
                    value.clone(),
                )
            } else {
                event_msg_semantic_event(*index, &timestamp, kind, value)
            };
            events.push(event);
        }
    }

    let is_top_level = !path_is_subagent && agent_id.is_empty();
    let mut session_id = if is_top_level {
        parent_session_id.clone()
    } else if !agent_id.is_empty() {
        agent_id
    } else {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("")
            .trim_start_matches("agent-")
            .to_string()
    };
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
    if !is_top_level && !line_direct {
        let mut details = serde_json::Map::new();
        details.insert("parent_id".to_string(), Value::String(parent_session_id));
        events.push(semantic_event(
            0,
            EventKind::SystemStatus,
            &started_at,
            None,
            Some("session_started".to_string()),
            None,
            Value::Object(details),
        ));
    }
    finish_source_conversation(
        Source::Claude,
        path,
        session_id,
        String::new(),
        project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        is_top_level,
    )
}
