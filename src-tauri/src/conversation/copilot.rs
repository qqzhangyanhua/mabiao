use std::path::{Path, PathBuf};

use serde_json::Value;

use super::toolbox::*;
use super::{
    diagnostic_detail, diagnostic_index, discover_jsonl, ConversationIndexBatch,
    ConversationIndexIssue,
};

#[cfg(test)]
#[path = "copilot_test.rs"]
mod tests;

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    diagnostic_index(path, "copilot_events", parse)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    diagnostic_detail(path, session_id, include_deferred_content, parse)
}

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(discover_jsonl(roots)?
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some("events.jsonl"))
        .collect())
}

fn parse(
    path: &Path,
    include_deferred_content: bool,
) -> Result<(ParsedConversation, Vec<ConversationIndexIssue>), String> {
    let values = parse_jsonl_conversation_values(path)?;
    if values.is_empty() {
        return Err("Copilot events.jsonl 不包含事件".to_string());
    }
    let mut session_id = String::new();
    let mut project = String::new();
    let mut model = String::new();
    let mut has_usage = false;
    for (_, value) in &values {
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let data = value.get("data").unwrap_or(&Value::Null);
        if kind == "session.start" {
            if let Some(candidate) = data
                .get("sessionId")
                .and_then(Value::as_str)
                .filter(|candidate| !candidate.is_empty())
            {
                session_id = candidate.to_string();
            }
            if let Some(candidate) = data
                .pointer("/context/cwd")
                .and_then(Value::as_str)
                .filter(|candidate| !candidate.is_empty())
            {
                project = candidate.to_string();
            }
        }
        if kind == "session.shutdown" {
            has_usage = data
                .get("modelMetrics")
                .and_then(Value::as_object)
                .is_some_and(|metrics| !metrics.is_empty());
            if let Some(candidate) = data
                .get("currentModel")
                .and_then(Value::as_str)
                .filter(|candidate| !candidate.is_empty())
            {
                model = candidate.to_string();
            }
        }
    }
    if session_id.is_empty() {
        session_id = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
    }
    if session_id.is_empty() {
        return Err("Copilot 事件缺少必需的会话 ID".to_string());
    }

    let messages = Vec::new();
    let mut events = Vec::new();
    let mut diagnostics = Vec::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut sequence = 0usize;
    for (line, value) in values {
        let event_start = events.len();
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let data = value.get("data").unwrap_or(&Value::Null);
        let occurred_at = value
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        update_time_bounds(&occurred_at, &mut started_at, &mut ended_at);
        match kind {
            "session.start" => events.push(semantic_event(
                sequence,
                EventKind::SystemStatus,
                &occurred_at,
                None,
                Some("session_started".to_string()),
                None,
                data.clone(),
            )),
            "session.shutdown" => events.push(semantic_event(
                sequence,
                EventKind::SystemStatus,
                &occurred_at,
                None,
                Some("session_shutdown".to_string()),
                data.get("shutdownType")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                data.clone(),
            )),
            "tool.execution_start" => events.push(semantic_event(
                sequence,
                EventKind::ToolCall,
                &occurred_at,
                Some(EventActor::Assistant),
                data.get("tool").and_then(Value::as_str).map(str::to_string),
                tool_text(data),
                normalize_tool_event(data),
            )),
            "tool.execution_complete" => {
                let details = normalize_tool_event(data);
                events.push(tool_result_event(
                    sequence,
                    &occurred_at,
                    &details,
                    include_deferred_content,
                ));
            }
            "tool.execution_error" => events.push(semantic_event(
                sequence,
                EventKind::Error,
                &occurred_at,
                Some(EventActor::Tool),
                data.get("tool").and_then(Value::as_str).map(str::to_string),
                optional_text(data, &["error", "message"]),
                normalize_tool_event(data),
            )),
            _ => {
                events.push(unadapted_event(sequence, &occurred_at, kind, value.clone()));
                diagnostics.push(ConversationIndexIssue {
                    path: path.to_string_lossy().to_string(),
                    message: format!(
                        "Copilot events.jsonl 第 {} 行事件类型 {} 尚未适配",
                        line + 1,
                        display_kind(kind)
                    ),
                    event_type: Some("copilot_event".to_string()),
                    line: Some((line + 1) as u64),
                });
            }
        }
        let native_identity = native_identity(kind, &value);
        tag_source_events(&mut events[event_start..], line, native_identity.as_deref());
        sequence += 1;
    }

    let mut missing = vec!["user_message", "assistant_message"];
    if model.is_empty() {
        missing.push("model");
    }
    if !has_usage {
        missing.push("usage");
    }
    if events.iter().any(|event| event.occurred_at.is_none()) {
        missing.push("timestamp");
    }
    append_declared_capability_degradation_status(sequence, &missing, &mut events);
    assign_native_event_ids(&mut events, Source::Copilot, &session_id);
    let parsed = finish_source_conversation(
        Source::Copilot,
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
    )?;
    Ok((parsed, diagnostics))
}

fn normalize_tool_event(data: &Value) -> Value {
    let mut details = normalize_tool_result_details(data);
    if let Value::Object(object) = &mut details {
        if !object.contains_key("call_id") {
            if let Some(call_id) = object.get("tool_call_id").cloned() {
                object.insert("call_id".to_string(), call_id);
            }
        }
    }
    details
}

fn native_identity(kind: &str, value: &Value) -> Option<String> {
    value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .or_else(|| {
            value
                .pointer("/data/tool_call_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
        })
        .map(|id| format!("{kind}:{id}"))
}

fn tool_text(data: &Value) -> Option<String> {
    optional_text(data, &["path", "command", "query", "description"])
}

fn display_kind(kind: &str) -> &str {
    if kind.is_empty() {
        "<missing>"
    } else {
        kind
    }
}
