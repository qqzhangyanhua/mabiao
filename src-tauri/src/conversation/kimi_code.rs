use std::path::Path;

use serde_json::Value;

use super::super::toolbox::*;
use super::super::ConversationIndexIssue;
use crate::domain::ConversationMessage;

pub(super) enum WireAccept {
    /// 记账、镜像或注入，不进对话正文，也不报未适配。
    Silent,
    /// 已经投影成事件。
    Handled,
    /// 旧版 `message.type` 信封，交给原来的分支。
    Legacy,
}

pub(super) struct WireCursor<'a> {
    pub path: &'a Path,
    pub line: usize,
    pub sequence: usize,
    pub occurred_at: &'a str,
    pub messages: &'a mut Vec<ConversationMessage>,
    pub events: &'a mut Vec<ConversationEvent>,
    pub diagnostics: &'a mut Vec<ConversationIndexIssue>,
}

pub(super) fn observed_model(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) != Some("llm.request") {
        return None;
    }
    let model = value.get("model").and_then(Value::as_str)?.trim();
    if model.is_empty() {
        None
    } else {
        Some(model.to_string())
    }
}

pub(super) fn accept(cursor: &mut WireCursor<'_>, value: &Value) -> WireAccept {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
    if is_silent_record(kind) {
        return WireAccept::Silent;
    }
    match kind {
        "turn.prompt" => {
            push_projected_message(
                cursor.sequence,
                cursor.occurred_at,
                "user",
                value.get("input").unwrap_or(&Value::Null),
                structural_details(cursor.line, kind, value),
                cursor.messages,
                cursor.events,
            );
            WireAccept::Handled
        }
        "context.append_message" => accept_context_message(cursor, value),
        "context.append_loop_event" => accept_loop_event(cursor, value),
        _ => WireAccept::Legacy,
    }
}

fn is_silent_record(kind: &str) -> bool {
    matches!(
        kind,
        "usage.record"
            | "forked"
            | "runtime.set_binding"
            | "mcp.tools_discovered"
            | "permission.set_mode"
            | "profile.bind"
            | "config.update"
            | "plugin.session_start"
            | "llm.tools_snapshot"
            | "llm.request"
            | "token_counting.measured"
            | "token_counting.turn_recorded"
            | "agent.turn.started"
            | "agent.turn.ended"
            | "turn.ended"
            | "prompt.completed"
            | "agent.message.appended"
    )
}

fn accept_context_message(cursor: &mut WireCursor<'_>, value: &Value) -> WireAccept {
    let message = value.get("message").unwrap_or(value);
    let origin_kind = message
        .pointer("/origin/kind")
        .and_then(Value::as_str)
        .unwrap_or("");
    // `turn.prompt` 已经是用户原话。同一次提交的 context 副本，以及日期一类注入，不再入账。
    if origin_kind == "user" || origin_kind == "injection" {
        return WireAccept::Silent;
    }
    match message.get("role").and_then(Value::as_str) {
        Some(role @ ("user" | "assistant")) => {
            push_projected_message(
                cursor.sequence,
                cursor.occurred_at,
                role,
                message.get("content").unwrap_or(&Value::Null),
                structural_details(cursor.line, "context.append_message", message),
                cursor.messages,
                cursor.events,
            );
            WireAccept::Handled
        }
        _ => {
            push_unadapted(cursor, "context.append_message", value);
            WireAccept::Handled
        }
    }
}

fn accept_loop_event(cursor: &mut WireCursor<'_>, value: &Value) -> WireAccept {
    let event = value.get("event").unwrap_or(&Value::Null);
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
    if event_type == "step.begin" || event_type == "step.end" {
        return WireAccept::Silent;
    }
    if event_type == "content.part" {
        let part = event.get("part").unwrap_or(&Value::Null);
        match part.get("type").and_then(Value::as_str).unwrap_or("") {
            "think" => {
                cursor.events.push(semantic_event(
                    cursor.sequence,
                    EventKind::Plan,
                    cursor.occurred_at,
                    Some(EventActor::Assistant),
                    Some("think".to_string()),
                    optional_text(part, &["think", "text"]),
                    part.clone(),
                ));
            }
            "text" => {
                push_projected_message(
                    cursor.sequence,
                    cursor.occurred_at,
                    "assistant",
                    part.get("text").unwrap_or(&Value::Null),
                    structural_details(cursor.line, "content.part", part),
                    cursor.messages,
                    cursor.events,
                );
            }
            other => {
                let display = if other.is_empty() {
                    "content.part".to_string()
                } else {
                    format!("content.part:{other}")
                };
                push_unadapted(cursor, &display, value);
            }
        }
        return WireAccept::Handled;
    }
    let display = if event_type.is_empty() {
        "context.append_loop_event"
    } else {
        event_type
    };
    push_unadapted(cursor, display, value);
    WireAccept::Handled
}

fn push_unadapted(cursor: &mut WireCursor<'_>, raw_kind: &str, value: &Value) {
    cursor.events.push(unadapted_event(
        cursor.sequence,
        cursor.occurred_at,
        raw_kind,
        value.clone(),
    ));
    cursor.diagnostics.push(ConversationIndexIssue {
        path: cursor.path.to_string_lossy().to_string(),
        message: format!(
            "Kimi wire 第 {} 行事件类型 {} 尚未适配",
            cursor.line + 1,
            display_kind(raw_kind)
        ),
        event_type: Some("kimi_wire_event".to_string()),
        line: Some((cursor.line + 1) as u64),
    });
}

fn structural_details(line: usize, raw_kind: &str, message: &Value) -> Value {
    serde_json::json!({
        "line": line + 1,
        "type": raw_kind,
        "message_id": message.get("id").or_else(|| message.get("message_id")),
    })
}

fn display_kind(kind: &str) -> &str {
    if kind.is_empty() {
        "<missing>"
    } else {
        kind
    }
}
