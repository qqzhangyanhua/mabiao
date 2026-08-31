use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::toolbox::*;
use super::{
    diagnostic_detail, diagnostic_index, discover_jsonl, regular_source_revision,
    ConversationIndexBatch, ConversationIndexIssue,
};
use crate::ingest;

#[cfg(test)]
#[path = "kimi_test.rs"]
mod tests;

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    diagnostic_index(path, "kimi_wire", parse)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    diagnostic_detail(path, session_id, include_deferred_content, parse)
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    let sidecar = sidecar_path(path)?;
    validate_sidecar(&sidecar)?;
    Ok(format!(
        "{}:{}",
        regular_source_revision(path)?,
        ingest::content_fingerprint(&sidecar)
    ))
}

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(discover_jsonl(roots)?
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some("wire.jsonl"))
        .collect())
}

fn parse(
    path: &Path,
    include_deferred_content: bool,
) -> Result<(ParsedConversation, Vec<ConversationIndexIssue>), String> {
    let session_id = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Kimi wire 路径缺少必需的会话 ID".to_string())?
        .to_string();
    let project = project(path, &session_id)?;
    let values = parse_jsonl_conversation_values(path)?;
    let latest = latest_identities(&values);
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut diagnostics = Vec::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut sequence = 0usize;

    for (line, value) in values {
        let message = value.get("message").unwrap_or(&value);
        let raw_kind = message.get("type").and_then(Value::as_str).unwrap_or("");
        let native_identity = identity(message, raw_kind);
        if native_identity
            .as_ref()
            .is_some_and(|identity| latest.get(identity).copied() != Some(line))
        {
            continue;
        }
        let payload = message.get("payload").unwrap_or(message);
        let occurred_at = kimi_timestamp(&value);
        update_time_bounds(&occurred_at, &mut started_at, &mut ended_at);
        let event_start = events.len();
        let role = payload
            .get("role")
            .or_else(|| message.get("role"))
            .and_then(Value::as_str);
        let normalized = normalize_kind(raw_kind);
        match (normalized.as_str(), role) {
            ("usermessage" | "userinput", _) | (_, Some("user")) => {
                push_projected_message(
                    sequence,
                    &occurred_at,
                    "user",
                    message_content(payload),
                    structural_details(line, raw_kind, message),
                    &mut messages,
                    &mut events,
                );
                sequence += 1;
            }
            ("assistantmessage" | "agentmessage", _) | (_, Some("assistant")) => {
                push_projected_message(
                    sequence,
                    &occurred_at,
                    "assistant",
                    message_content(payload),
                    structural_details(line, raw_kind, message),
                    &mut messages,
                    &mut events,
                );
                sequence += 1;
            }
            ("toolcall", _) => {
                events.push(semantic_event(
                    sequence,
                    EventKind::ToolCall,
                    &occurred_at,
                    Some(EventActor::Assistant),
                    optional_text(payload, &["name", "tool_name"]),
                    tool_call_text(payload),
                    normalize_kimi_tool_call(payload),
                ));
                sequence += 1;
            }
            ("toolresult", _) => {
                let details = normalize_kimi_tool_result(payload);
                events.push(tool_result_event(
                    sequence,
                    &occurred_at,
                    &details,
                    include_deferred_content,
                ));
                sequence += 1;
            }
            ("statusupdate", _) => {
                let name = optional_text(payload, &["status", "state", "phase"])
                    .unwrap_or_else(|| "status_update".to_string());
                events.push(semantic_event(
                    sequence,
                    EventKind::SystemStatus,
                    &occurred_at,
                    None,
                    Some(name),
                    optional_text(payload, &["message", "description", "summary"]),
                    payload.clone(),
                ));
                sequence += 1;
            }
            ("metadata", _) => {
                events.push(semantic_event(
                    sequence,
                    EventKind::SystemStatus,
                    &occurred_at,
                    None,
                    Some("wire_metadata".to_string()),
                    None,
                    structural_details(line, raw_kind, message),
                ));
                sequence += 1;
            }
            _ => {
                events.push(unadapted_event(
                    sequence,
                    &occurred_at,
                    raw_kind,
                    value.clone(),
                ));
                diagnostics.push(ConversationIndexIssue {
                    path: path.to_string_lossy().to_string(),
                    message: format!(
                        "Kimi wire 第 {} 行事件类型 {} 尚未适配",
                        line + 1,
                        display_kind(raw_kind)
                    ),
                    event_type: Some("kimi_wire_event".to_string()),
                    line: Some((line + 1) as u64),
                });
                sequence += 1;
            }
        }
        tag_source_events(&mut events[event_start..], line, native_identity.as_deref());
    }

    assign_native_event_ids(&mut events, Source::Kimi, &session_id);
    append_capability_degradation_status(sequence, &messages, "", &mut events);
    let parsed = finish_source_conversation(
        Source::Kimi,
        path,
        session_id,
        String::new(),
        project,
        String::new(),
        started_at,
        ended_at,
        messages,
        events,
        true,
    )?;
    Ok((parsed, diagnostics))
}

fn latest_identities(values: &[(usize, Value)]) -> BTreeMap<String, usize> {
    values
        .iter()
        .filter_map(|(line, value)| {
            let message = value.get("message").unwrap_or(value);
            let kind = message.get("type").and_then(Value::as_str).unwrap_or("");
            identity(message, kind).map(|identity| (identity, *line))
        })
        .collect()
}

fn identity(message: &Value, raw_kind: &str) -> Option<String> {
    let normalized = normalize_kind(raw_kind);
    let payload = message.get("payload").unwrap_or(message);
    let id = optional_text(payload, &["message_id", "tool_call_id", "toolCallId", "id"])?;
    matches!(
        normalized.as_str(),
        "statusupdate"
            | "usermessage"
            | "userinput"
            | "assistantmessage"
            | "agentmessage"
            | "toolcall"
            | "toolresult"
    )
    .then(|| format!("{normalized}:{id}"))
}

fn project(path: &Path, session_id: &str) -> Result<String, String> {
    let sidecar = sidecar_path(path)?;
    let value = validate_sidecar(&sidecar)?;
    Ok(value
        .get("work_dirs")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("last_session_id").and_then(Value::as_str) == Some(session_id)
            })
        })
        .and_then(|item| item.get("path"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            path.parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_string()
        }))
}

fn sidecar_path(path: &Path) -> Result<PathBuf, String> {
    let sessions = path
        .ancestors()
        .find(|ancestor| ancestor.file_name().and_then(|name| name.to_str()) == Some("sessions"))
        .ok_or_else(|| "Kimi wire 路径不在 sessions 目录内".to_string())?;
    sessions
        .parent()
        .map(|root| root.join("kimi.json"))
        .ok_or_else(|| "Kimi sessions 目录缺少工具根目录".to_string())
}

fn validate_sidecar(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(Value::Null);
    }
    let content =
        fs::read_to_string(path).map_err(|error| format!("读取 kimi.json 失败：{error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("kimi.json 无效：{error}"))
}

fn message_content(payload: &Value) -> &Value {
    payload
        .get("content")
        .or_else(|| payload.get("text"))
        .or_else(|| payload.get("message"))
        .unwrap_or(&Value::Null)
}

fn tool_call_text(payload: &Value) -> Option<String> {
    let input = payload.get("arguments").or_else(|| payload.get("input"))?;
    if let Some(input) = input.as_str() {
        return serde_json::from_str::<Value>(input)
            .ok()
            .as_ref()
            .and_then(|value| optional_text(value, &["path", "file_path", "command", "query"]));
    }
    optional_text(input, &["path", "file_path", "command", "query"])
}

fn normalize_kimi_tool_call(payload: &Value) -> Value {
    let mut details = payload.clone();
    if let Value::Object(object) = &mut details {
        if let Some(call_id) = object
            .get("tool_call_id")
            .or_else(|| object.get("toolCallId"))
            .or_else(|| object.get("id"))
            .cloned()
        {
            object.insert("call_id".to_string(), call_id);
        }
    }
    details
}

fn normalize_kimi_tool_result(payload: &Value) -> Value {
    let mut details = normalize_tool_result_details(payload);
    if let Value::Object(object) = &mut details {
        if !object.contains_key("call_id") {
            if let Some(call_id) = object.get("tool_call_id").cloned() {
                object.insert("call_id".to_string(), call_id);
            }
        }
    }
    details
}

fn structural_details(line: usize, raw_kind: &str, message: &Value) -> Value {
    json!({
        "line": line + 1,
        "type": raw_kind,
        "message_id": message.pointer("/payload/message_id").or_else(|| message.get("message_id")),
    })
}

fn normalize_kind(kind: &str) -> String {
    kind.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn display_kind(kind: &str) -> &str {
    if kind.is_empty() {
        "<missing>"
    } else {
        kind
    }
}

fn kimi_timestamp(value: &Value) -> String {
    let Some(timestamp) = value.get("timestamp") else {
        return String::new();
    };
    if let Some(text) = timestamp.as_str() {
        return text.to_string();
    }
    timestamp
        .as_f64()
        .and_then(|seconds| chrono::DateTime::from_timestamp_millis((seconds * 1000.0) as i64))
        .map(|timestamp| timestamp.to_rfc3339())
        .unwrap_or_default()
}
