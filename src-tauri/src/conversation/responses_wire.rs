//! OpenAI Responses / Codex rollout 及事件消息（`event_msg`）线格式投影。
//!
//! 仅供遵循 Responses / `event_msg` 约定的对话记录适配器显式引入，
//! 通用归一化构造在 `normalize`。

use serde_json::Value;

use crate::domain::{
    ConversationEvent, ConversationEventActor as EventActor, ConversationEventKind as EventKind,
    ConversationMessage,
};

use super::normalize::{
    compression_boundary_text, content_text, first_text, message_event,
    normalize_tool_call_details, optional_text, semantic_event, tool_payload_failed,
    tool_result_event, unadapted_event,
};

pub(crate) struct PendingMessageDelta {
    sequence: u32,
    occurred_at: String,
    role: String,
    text: String,
}

pub(crate) fn append_message_delta(
    pending: &mut Option<PendingMessageDelta>,
    sequence: usize,
    occurred_at: &str,
    role: &str,
    payload: &Value,
) {
    let delta = first_text(payload, &["delta", "message", "text"]);
    if delta.is_empty() {
        return;
    }
    match pending {
        Some(current) if current.role == role => current.text.push_str(&delta),
        Some(_) => {}
        None => {
            *pending = Some(PendingMessageDelta {
                sequence: sequence as u32,
                occurred_at: occurred_at.to_string(),
                role: role.to_string(),
                text: delta,
            });
        }
    }
}

pub(crate) fn flush_message_delta(
    pending: &mut Option<PendingMessageDelta>,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
) {
    let Some(delta) = pending.take() else {
        return;
    };
    let Some(message) = message(&delta.role, &delta.occurred_at, &Value::String(delta.text)) else {
        return;
    };
    events.push(message_event(
        delta.sequence as usize,
        &message,
        Value::Null,
    ));
    messages.push(message);
}

pub(crate) fn response_semantic_event(
    sequence: usize,
    occurred_at: &str,
    payload: &Value,
    include_deferred_content: bool,
) -> Option<ConversationEvent> {
    let kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "message" => None,
        "function_call"
        | "custom_tool_call"
        | "web_search_call"
        | "local_shell_call"
        | "tool_search_call"
        | "view_image_tool_call" => Some(semantic_event(
            sequence,
            EventKind::ToolCall,
            occurred_at,
            Some(EventActor::Assistant),
            optional_text(payload, &["name", "tool", "type"])
                .or_else(|| (kind == "view_image_tool_call").then(|| "view_image".to_string())),
            tool_call_text(payload),
            normalize_tool_call_details(payload),
        )),
        "function_call_output" | "custom_tool_call_output" | "tool_search_output" => Some(
            tool_result_event(sequence, occurred_at, payload, include_deferred_content),
        ),
        "reasoning" => Some(semantic_event(
            sequence,
            EventKind::Plan,
            occurred_at,
            Some(EventActor::Assistant),
            None,
            reasoning_text(payload),
            payload.clone(),
        )),
        "ghost_snapshot" => Some(semantic_event(
            sequence,
            EventKind::SystemStatus,
            occurred_at,
            None,
            Some("ghost_snapshot".to_string()),
            None,
            payload.clone(),
        )),
        "developer" | "system" | "agent_message" => None,
        _ => Some(unadapted_event(
            sequence,
            occurred_at,
            kind,
            payload.clone(),
        )),
    }
}

pub(crate) fn event_msg_semantic_event(
    sequence: usize,
    occurred_at: &str,
    kind: &str,
    payload: &Value,
) -> ConversationEvent {
    match kind {
        "plan_update" | "agent_reasoning" => semantic_event(
            sequence,
            EventKind::Plan,
            occurred_at,
            Some(EventActor::Assistant),
            None,
            optional_text(payload, &["explanation", "message", "text"]),
            payload.clone(),
        ),
        "thread_goal_updated" => semantic_event(
            sequence,
            EventKind::Plan,
            occurred_at,
            Some(EventActor::Assistant),
            Some("thread_goal_updated".to_string()),
            payload
                .get("goal")
                .and_then(|goal| optional_text(goal, &["objective", "goal"]))
                .or_else(|| optional_text(payload, &["objective", "goal"])),
            payload.clone(),
        ),
        "view_image_tool_call" => semantic_event(
            sequence,
            EventKind::ToolCall,
            occurred_at,
            Some(EventActor::Assistant),
            Some("view_image".to_string()),
            optional_text(payload, &["path", "file_path"]),
            normalize_tool_call_details(payload),
        ),
        "todo_state" => semantic_event(
            sequence,
            EventKind::Plan,
            occurred_at,
            Some(EventActor::Assistant),
            Some("todo_state".to_string()),
            todo_state_text(payload),
            payload.clone(),
        ),
        "error" | "stream_error" => semantic_event(
            sequence,
            EventKind::Error,
            occurred_at,
            None,
            optional_text(payload, &["code", "type"]),
            optional_text(payload, &["message", "error"]),
            payload.clone(),
        ),
        kind if is_lifecycle_kind(kind) => semantic_event(
            sequence,
            EventKind::SystemStatus,
            occurred_at,
            None,
            Some(lifecycle_event_name(kind, payload)),
            lifecycle_status_text(kind, payload),
            payload.clone(),
        ),
        _ => unadapted_event(sequence, occurred_at, kind, payload.clone()),
    }
}

fn is_lifecycle_kind(kind: &str) -> bool {
    matches!(
        kind,
        "task_started"
            | "task_complete"
            | "turn_aborted"
            | "context_compacted"
            | "warning"
            | "thinking_level_change"
            | "title"
            | "title_change"
            | "credential_pin"
            | "session_init"
            | "reset_boundary"
            | "ttsr_injection"
            | "session_start"
            | "compaction_state"
            | "compacted"
            | "session_info"
            | "compaction"
            | "compression"
            | "compact"
            | "branch_summary"
            | "custom"
            | "custom_message"
            | "thread_settings_applied"
            | "thread_name_updated"
            | "thread_rolled_back"
            | "world_state"
            | "inter_agent_communication_metadata"
            | "sub_agent_activity"
            | "collab_waiting_end"
            | "collab_agent_spawn_end"
            | "collab_close_end"
            | "collab_agent_interaction_end"
            | "collab_resume_end"
    )
}

fn lifecycle_event_name(kind: &str, payload: &Value) -> String {
    payload
        .get("customType")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(kind)
        .to_string()
}

fn lifecycle_status_text(kind: &str, payload: &Value) -> Option<String> {
    match kind {
        "thinking_level_change" => optional_text(payload, &["thinkingLevel", "thinking_level"]),
        "title" | "title_change" | "session_start" => optional_text(payload, &["title"]),
        "credential_pin" => optional_text(payload, &["provider"]),
        "ttsr_injection" => payload
            .get("injectedRules")
            .and_then(Value::as_array)
            .map(|rules| {
                rules
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|text| !text.is_empty()),
        "compression" | "compact" | "compaction" | "compacted" | "context_compacted" => {
            Some(compression_boundary_text(payload))
        }
        "custom_message" => optional_text(payload, &["content", "text", "message"]),
        "custom" => payload
            .get("data")
            .and_then(|data| optional_text(data, &["reason", "kind", "intent", "message"]))
            .or_else(|| optional_text(payload, &["reason", "message"])),
        "thread_settings_applied" => payload
            .get("thread_settings")
            .and_then(|settings| optional_text(settings, &["model"])),
        "thread_name_updated" => optional_text(payload, &["thread_name", "title"]),
        "sub_agent_activity" => optional_text(payload, &["kind", "agent_path"]),
        "collab_agent_spawn_end" => {
            optional_text(payload, &["new_agent_nickname", "new_agent_role", "prompt"])
        }
        "collab_close_end" | "collab_resume_end" | "collab_agent_interaction_end" => optional_text(
            payload,
            &["receiver_agent_nickname", "receiver_agent_role", "prompt"],
        ),
        _ => optional_text(
            payload,
            &["message", "reason", "text", "summary", "explanation"],
        ),
    }
}

fn reasoning_text(payload: &Value) -> Option<String> {
    optional_text(payload, &["text", "content"]).or_else(|| {
        let text = content_text(payload.get("summary").unwrap_or(&Value::Null));
        (!text.is_empty()).then_some(text)
    })
}

fn todo_state_text(payload: &Value) -> Option<String> {
    let todos = payload
        .pointer("/todos/todos")
        .or_else(|| payload.get("todos"))
        .and_then(Value::as_array)?;
    let lines = todos
        .iter()
        .filter_map(|todo| todo.get("content").and_then(Value::as_str))
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

pub(crate) fn is_duplicate_tool_execution_start(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("custom")
        && value.get("customType").and_then(Value::as_str) == Some("tool_execution_start")
}

pub(crate) fn failed_tool_end_event(
    sequence: usize,
    occurred_at: &str,
    kind: &str,
    payload: &Value,
) -> Option<ConversationEvent> {
    if !tool_payload_failed(payload) {
        return None;
    }
    let name = match kind {
        "exec_command_end" => Some("exec_command".to_string()),
        "patch_apply_end" => Some("apply_patch".to_string()),
        "web_search_end" => Some("web_search_call".to_string()),
        "mcp_tool_call_end" => payload
            .pointer("/invocation/tool")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| optional_text(payload, &["name", "tool"])),
        _ => Some(kind.to_string()),
    };
    Some(semantic_event(
        sequence,
        EventKind::Error,
        occurred_at,
        Some(EventActor::Tool),
        name,
        optional_text(
            payload,
            &[
                "aggregated_output",
                "formatted_output",
                "stdout",
                "stderr",
                "output",
                "message",
            ],
        ),
        payload.clone(),
    ))
}

fn tool_call_text(payload: &Value) -> Option<String> {
    optional_text(payload, &["arguments", "input", "query", "command", "path"]).or_else(|| {
        payload.get("arguments").and_then(|arguments| {
            arguments
                .as_str()
                .map(str::to_string)
                .or_else(|| optional_text(arguments, &["query", "path", "command"]))
        })
    })
}

pub(crate) fn response_message(payload: &Value, occurred_at: &str) -> Option<ConversationMessage> {
    let kind = payload.get("type").and_then(Value::as_str)?;
    let (role, content) = match kind {
        "message" => {
            let role = payload.get("role").and_then(Value::as_str)?;
            if role != "user" && role != "assistant" {
                return None;
            }
            (role, payload.get("content")?)
        }
        "agent_message" => (
            "assistant",
            payload.get("content").or_else(|| payload.get("message"))?,
        ),
        _ => return None,
    };
    message(role, occurred_at, content)
}

pub(crate) fn event_message(payload: &Value, occurred_at: &str) -> Option<ConversationMessage> {
    let role = match payload.get("type").and_then(Value::as_str)? {
        "user_message" => "user",
        "agent_message" => "assistant",
        _ => return None,
    };
    message(role, occurred_at, payload.get("message")?)
}

fn message(role: &str, occurred_at: &str, content: &Value) -> Option<ConversationMessage> {
    let text = content_text(content).trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(ConversationMessage {
        role: role.to_string(),
        occurred_at: occurred_at.to_string(),
        text,
    })
}
