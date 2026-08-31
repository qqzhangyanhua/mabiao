use std::path::Path;

use serde_json::{json, Value};

use super::toolbox::*;
use super::{diagnostic_detail, diagnostic_index, ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::project::project_from_source_file;

#[cfg(test)]
#[path = "droid_test.rs"]
mod tests;

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    diagnostic_index(path, "droid_session", parse)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    diagnostic_detail(path, session_id, include_deferred_content, parse)
}

fn parse(
    path: &Path,
    include_deferred_content: bool,
) -> Result<(ParsedConversation, Vec<ConversationIndexIssue>), String> {
    let session_id = path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Droid 原始会话文件名缺少必需的会话 ID".to_string())?
        .to_string();
    let values = parse_jsonl_conversation_values(path)?;
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut diagnostics = Vec::new();
    let mut model = String::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut sequence = 0usize;

    for (line, value) in values {
        let occurred_at = timestamp(&value);
        update_time_bounds(&occurred_at, &mut started_at, &mut ended_at);
        let raw_kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        if matches!(raw_kind, "tool_result" | "tool/result") {
            let details = tool_result_details(&value);
            events.push(tool_result_event(
                sequence,
                &occurred_at,
                &details,
                include_deferred_content,
            ));
            sequence += 1;
            continue;
        }
        let envelope = value.get("message").unwrap_or(&value);
        let role = value
            .get("role")
            .and_then(Value::as_str)
            .or_else(|| envelope.get("role").and_then(Value::as_str))
            .or_else(|| match value.get("type").and_then(Value::as_str) {
                Some("user") => Some("user"),
                Some("assistant") => Some("assistant"),
                _ => None,
            });
        if matches!(role, Some("user" | "assistant")) {
            if let Some(next_model) = envelope
                .get("model")
                .or_else(|| value.get("model"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                model = next_model.to_string();
            }
            project_message(
                line,
                envelope,
                role.unwrap_or_default(),
                &occurred_at,
                &mut sequence,
                &mut messages,
                &mut events,
                include_deferred_content,
            );
            continue;
        }

        match raw_kind {
            "system" | "session" => {
                events.push(semantic_event(
                    sequence,
                    EventKind::SystemStatus,
                    &occurred_at,
                    None,
                    Some("session_status".to_string()),
                    None,
                    structural_details(line, &value),
                ));
                sequence += 1;
            }
            raw_kind => {
                events.push(unadapted_event(
                    sequence,
                    &occurred_at,
                    raw_kind,
                    structural_details(line, &value),
                ));
                diagnostics.push(ConversationIndexIssue {
                    path: path.to_string_lossy().to_string(),
                    message: format!("Droid 会话第 {} 行事件类型尚未适配", line + 1),
                    event_type: Some("droid_event".to_string()),
                    line: Some((line + 1) as u64),
                });
                sequence += 1;
            }
        }
    }

    append_capability_degradation_status(sequence, &messages, &model, &mut events);
    let parsed = finish_source_conversation(
        Source::Factory,
        path,
        session_id,
        String::new(),
        project_from_source_file(&path.to_string_lossy()),
        model,
        started_at,
        ended_at,
        messages,
        events,
        true,
    )?;
    Ok((parsed, diagnostics))
}

#[allow(clippy::too_many_arguments)]
fn project_message(
    line: usize,
    envelope: &Value,
    role: &str,
    occurred_at: &str,
    sequence: &mut usize,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
    include_deferred_content: bool,
) {
    let content = envelope
        .get("content")
        .or_else(|| envelope.get("message"))
        .unwrap_or(&Value::Null);
    let Some(blocks) = content.as_array() else {
        push_projected_message(
            *sequence,
            occurred_at,
            role,
            content,
            structural_details(line, envelope),
            messages,
            events,
        );
        *sequence += 1;
        return;
    };

    let text = blocks
        .iter()
        .filter(|block| {
            block.get("type").and_then(Value::as_str) == Some("text") || block.get("type").is_none()
        })
        .map(content_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        push_projected_message(
            *sequence,
            occurred_at,
            role,
            &Value::String(text),
            structural_details(line, envelope),
            messages,
            events,
        );
        *sequence += 1;
    }

    for block in blocks {
        match block.get("type").and_then(Value::as_str).unwrap_or("") {
            "text" | "" => {}
            "tool_use" | "tool_call" => {
                events.push(semantic_event(
                    *sequence,
                    EventKind::ToolCall,
                    occurred_at,
                    Some(EventActor::Assistant),
                    block
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    tool_call_text(block),
                    normalize_tool_call_details(block),
                ));
                *sequence += 1;
            }
            "tool_result" => {
                let details = tool_result_details(block);
                events.push(tool_result_event(
                    *sequence,
                    occurred_at,
                    &details,
                    include_deferred_content,
                ));
                *sequence += 1;
            }
            "thinking" | "reasoning" => {
                events.push(semantic_event(
                    *sequence,
                    EventKind::Plan,
                    occurred_at,
                    Some(EventActor::Assistant),
                    None,
                    optional_text(block, &["text", "thinking"]),
                    block.clone(),
                ));
                *sequence += 1;
            }
            raw_kind => {
                events.push(unadapted_event(
                    *sequence,
                    occurred_at,
                    raw_kind,
                    structural_details(line, block),
                ));
                *sequence += 1;
            }
        }
    }
}

fn tool_call_text(block: &Value) -> Option<String> {
    let input = block.get("input")?;
    optional_text(
        input,
        &["path", "command", "query", "pattern", "description"],
    )
    .or_else(|| {
        input
            .get("paths")
            .map(content_text)
            .filter(|paths| !paths.is_empty())
    })
}

fn tool_result_details(value: &Value) -> Value {
    let mut details = normalize_tool_result_details(value);
    if let Value::Object(object) = &mut details {
        if !object.contains_key("output") && !object.contains_key("result") {
            let output = object
                .get("content")
                .map(content_text)
                .filter(|text| !text.is_empty());
            if let Some(output) = output {
                object.insert("output".to_string(), Value::String(output));
            }
        }
    }
    details
}

fn structural_details(line: usize, value: &Value) -> Value {
    json!({
        "line": line + 1,
        "type": value.get("type").and_then(Value::as_str),
        "role": value.get("role").and_then(Value::as_str),
    })
}

fn timestamp(value: &Value) -> String {
    for candidate in [
        value.get("timestamp"),
        value.get("created_at"),
        value.get("createdAt"),
        value.get("time"),
        value.pointer("/message/timestamp"),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(timestamp) = candidate.as_str() {
            return timestamp.to_string();
        }
        if let Some(milliseconds) = candidate.as_i64() {
            return chrono::DateTime::from_timestamp_millis(milliseconds)
                .map(|timestamp| timestamp.to_rfc3339())
                .unwrap_or_default();
        }
    }
    String::new()
}
