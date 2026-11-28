//! 通用事件构造与工具调用/结果归一化。
//!
//! 涵盖消息事件、语义事件、4KB 超大输出延迟加载预览、工具详情字段归一、
//! 工具结果名称配对与失败状态判定。由 `normalize` 统一对外 re-export。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::domain::{
    ConversationEvent, ConversationEventActor as EventActor,
    ConversationEventCapabilityStatus as EventStatus,
    ConversationEventContentStatus as ContentStatus, ConversationEventKind as EventKind,
    ConversationMessage,
};

use super::{content_text, first_text, optional_text};

const LARGE_CONTENT_THRESHOLD: usize = 4_096;
const CONTENT_PREVIEW_CHARS: usize = 2_000;

pub(crate) fn normalize_tool_call_details(item: &Value) -> Value {
    let mut details = item.clone();
    if let Value::Object(object) = &mut details {
        if !object.contains_key("call_id") {
            if let Some(id) = object.get("id").cloned() {
                object.insert("call_id".to_string(), id);
            }
        }
    }
    details
}

pub(crate) fn normalize_tool_result_details(item: &Value) -> Value {
    let mut details = item.clone();
    if let Value::Object(object) = &mut details {
        if !object.contains_key("call_id") {
            if let Some(id) = object
                .get("tool_use_id")
                .or_else(|| object.get("toolCallId"))
                .or_else(|| object.get("id"))
                .cloned()
            {
                object.insert("call_id".to_string(), id);
            }
        }
        if !object.contains_key("agent_id") {
            if let Some(agent_id) = object.get("agentId").cloned() {
                object.insert("agent_id".to_string(), agent_id);
            }
        }
        if !object.contains_key("output") {
            if let Some(content) = object.get("content").or_else(|| object.get("result")) {
                object.insert("output".to_string(), Value::String(content_text(content)));
            }
        }
    }
    details
}

pub(crate) fn message_event(
    sequence: usize,
    message: &ConversationMessage,
    details: Value,
) -> ConversationEvent {
    let actor = match message.role.as_str() {
        "user" => EventActor::User,
        "assistant" => EventActor::Assistant,
        _ => unreachable!("conversation messages only contain user or assistant roles"),
    };
    semantic_event(
        sequence,
        EventKind::Message,
        &message.occurred_at,
        Some(actor),
        None,
        Some(message.text.clone()),
        details,
    )
}

pub(crate) fn semantic_event(
    sequence: usize,
    kind: EventKind,
    occurred_at: &str,
    actor: Option<EventActor>,
    name: Option<String>,
    text: Option<String>,
    details: Value,
) -> ConversationEvent {
    ConversationEvent {
        event_id: String::new(),
        sequence: sequence as u32,
        source_file: String::new(),
        source_sequence: sequence as u32,
        kind,
        occurred_at: (!occurred_at.is_empty()).then(|| occurred_at.to_string()),
        actor,
        name,
        text,
        details,
        attachments: Vec::new(),
        capability_status: if occurred_at.is_empty() {
            EventStatus::MissingTimestamp
        } else {
            EventStatus::Complete
        },
        content_status: ContentStatus::Complete,
    }
}

pub(crate) fn tool_result_event(
    sequence: usize,
    occurred_at: &str,
    payload: &Value,
    include_deferred_content: bool,
) -> ConversationEvent {
    let text = optional_text(payload, &["output", "result"]);
    let should_defer = !include_deferred_content
        && text
            .as_ref()
            .is_some_and(|text| text.len() > LARGE_CONTENT_THRESHOLD);
    let mut details = payload.clone();
    let rendered_text = if should_defer {
        if let Value::Object(object) = &mut details {
            object.remove("output");
            object.remove("result");
        }
        text.map(|text| text.chars().take(CONTENT_PREVIEW_CHARS).collect())
    } else {
        text
    };
    let mut event = semantic_event(
        sequence,
        if tool_payload_failed(payload) {
            EventKind::Error
        } else {
            EventKind::ToolResult
        },
        occurred_at,
        Some(EventActor::Tool),
        optional_text(payload, &["name", "toolName", "tool"]),
        rendered_text,
        details,
    );
    if should_defer {
        event.content_status = ContentStatus::Deferred;
    }
    event
}

pub(crate) fn compression_boundary_text(payload: &Value) -> String {
    let compression = payload.get("compression").unwrap_or(payload);
    if compression
        .get("isPending")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return "正在压缩上下文".to_string();
    }
    let original = compression
        .get("originalTokenCount")
        .or_else(|| compression.get("original_token_count"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let new_count = compression
        .get("newTokenCount")
        .or_else(|| compression.get("new_token_count"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if original > 0 && new_count > 0 {
        return format!("上下文已压缩：{original} → {new_count} tokens");
    }
    let text = first_text(
        payload,
        &["content", "message", "summary", "text", "summaryText"],
    );
    if text.is_empty() {
        "上下文已压缩".to_string()
    } else {
        text
    }
}

pub(super) fn assign_tool_result_names(events: &mut [ConversationEvent]) {
    let mut names = BTreeMap::<String, String>::new();
    for event in events.iter() {
        if event.kind != EventKind::ToolCall {
            continue;
        }
        let Some(call_id) = event.details.get("call_id").and_then(Value::as_str) else {
            continue;
        };
        if let Some(name) = event.name.clone() {
            names.insert(call_id.to_string(), name);
        }
    }
    for event in events.iter_mut() {
        if event.name.is_some() || !is_tool_outcome(event) {
            continue;
        }
        if let Some(call_id) = event.details.get("call_id").and_then(Value::as_str) {
            event.name = names.get(call_id).cloned();
        }
    }
}

fn is_tool_outcome(event: &ConversationEvent) -> bool {
    event.kind == EventKind::ToolResult
        || (event.kind == EventKind::Error && event.actor == Some(EventActor::Tool))
}

pub(crate) fn tool_payload_failed(payload: &Value) -> bool {
    if json_flag(payload, &["is_error", "isError"]) {
        return true;
    }
    if payload.get("success").and_then(Value::as_bool) == Some(false) {
        return true;
    }
    if matches!(
        payload.get("status").and_then(Value::as_str),
        Some("failed" | "error" | "errored")
    ) {
        return true;
    }
    if json_i64(payload, "exit_code").is_some_and(|code| code != 0) {
        return true;
    }
    if payload.pointer("/result/Err").is_some() {
        return true;
    }
    if let Some(metadata) = payload.get("metadata") {
        if json_i64(metadata, "exit_code").is_some_and(|code| code != 0) {
            return true;
        }
    }
    output_metadata_failed(payload)
}

fn output_metadata_failed(payload: &Value) -> bool {
    let Some(output) = payload.get("output").and_then(Value::as_str) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(output) else {
        return false;
    };
    json_i64(&parsed, "exit_code").is_some_and(|code| code != 0)
        || parsed
            .get("metadata")
            .and_then(|metadata| json_i64(metadata, "exit_code"))
            .is_some_and(|code| code != 0)
        || json_flag(&parsed, &["is_error", "isError"])
}

fn json_flag(value: &Value, keys: &[&str]) -> bool {
    keys.iter()
        .any(|key| value.get(*key).and_then(Value::as_bool) == Some(true))
}

fn json_i64(value: &Value, key: &str) -> Option<i64> {
    let field = value.get(key)?;
    field
        .as_i64()
        .or_else(|| field.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| field.as_str()?.parse().ok())
}

pub(crate) fn unadapted_event(
    sequence: usize,
    occurred_at: &str,
    raw_kind: &str,
    details: Value,
) -> ConversationEvent {
    let mut event = semantic_event(
        sequence,
        EventKind::Unadapted,
        occurred_at,
        None,
        Some(if raw_kind.is_empty() {
            "unknown".to_string()
        } else {
            raw_kind.to_string()
        }),
        None,
        details,
    );
    event.capability_status = if occurred_at.is_empty() {
        EventStatus::UnadaptedMissingTimestamp
    } else {
        EventStatus::Unadapted
    };
    event
}
