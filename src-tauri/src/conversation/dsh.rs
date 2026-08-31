use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use super::toolbox::*;
use super::{diagnostic_detail, diagnostic_index, ConversationIndexBatch, ConversationIndexIssue};

#[cfg(test)]
#[path = "dsh_test.rs"]
mod tests;

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    diagnostic_index(path, "dsh_session", parse)
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
    let values = decode_values(path)?;
    let session = values
        .iter()
        .find_map(|(_, value)| {
            (value.get("type").and_then(Value::as_str) == Some("session")).then_some(value)
        })
        .ok_or_else(|| "DSH 压缩会话缺少必需的 session 事件".to_string())?;
    let session_id = session
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "DSH session 事件缺少必需的 id".to_string())?
        .to_string();
    let project = session
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut model = String::new();
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut diagnostics = Vec::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut sequence = 0usize;

    for (line, value) in values {
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let occurred_at = dsh_timestamp(&value);
        update_time_bounds(&occurred_at, &mut started_at, &mut ended_at);
        let data = value.get("data").unwrap_or(&Value::Null);
        match kind {
            "session" => {
                events.push(semantic_event(
                    sequence,
                    EventKind::SystemStatus,
                    &occurred_at,
                    None,
                    Some("session_started".to_string()),
                    None,
                    structural_details(line, &value),
                ));
                sequence += 1;
            }
            "request/header" => {
                let config = data.pointer("/header/config").unwrap_or(&Value::Null);
                let next_model = config.get("model").and_then(Value::as_str).unwrap_or("");
                if !next_model.is_empty() {
                    model = next_model.to_string();
                }
                events.push(semantic_event(
                    sequence,
                    EventKind::ModelChange,
                    &occurred_at,
                    None,
                    (!next_model.is_empty()).then(|| next_model.to_string()),
                    config
                        .get("provider")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    config.clone(),
                ));
                sequence += 1;
            }
            "user/message" => {
                let content = data.get("content").unwrap_or(&Value::Null);
                match data.pointer("/source/kind").and_then(Value::as_str) {
                    Some("user") => push_projected_message(
                        sequence,
                        &occurred_at,
                        "user",
                        content,
                        structural_details(line, &value),
                        &mut messages,
                        &mut events,
                    ),
                    Some(source_kind) => events.push(semantic_event(
                        sequence,
                        EventKind::SystemStatus,
                        &occurred_at,
                        None,
                        Some(source_kind.to_string()),
                        Some(content_text(content)),
                        data.get("source").cloned().unwrap_or(Value::Null),
                    )),
                    None => {
                        events.push(unadapted_event(
                            sequence,
                            &occurred_at,
                            "user/message",
                            structural_details(line, &value),
                        ));
                        diagnostics.push(ConversationIndexIssue {
                            path: path.to_string_lossy().to_string(),
                            message: format!(
                                "DSH 压缩会话第 {} 行 user/message 缺少 source.kind",
                                line + 1
                            ),
                            event_type: Some("dsh_event".to_string()),
                            line: Some((line + 1) as u64),
                        });
                    }
                }
                sequence += 1;
            }
            "assistant/message" => {
                let message = data.get("message").unwrap_or(&Value::Null);
                if let Some(next_model) = message
                    .pointer("/source/model")
                    .and_then(Value::as_str)
                    .filter(|model| !model.is_empty())
                {
                    model = next_model.to_string();
                }
                push_projected_message(
                    sequence,
                    &occurred_at,
                    "assistant",
                    message.get("content").unwrap_or(&Value::Null),
                    structural_details(line, &value),
                    &mut messages,
                    &mut events,
                );
                sequence += 1;
            }
            "tool/call" => {
                events.push(semantic_event(
                    sequence,
                    EventKind::ToolCall,
                    &occurred_at,
                    Some(EventActor::Assistant),
                    data.get("name").and_then(Value::as_str).map(str::to_string),
                    dsh_tool_call_text(data),
                    normalize_dsh_tool_call(data),
                ));
                sequence += 1;
            }
            "tool/result" => {
                let details = normalize_dsh_tool_result(data);
                events.push(tool_result_event(
                    sequence,
                    &occurred_at,
                    &details,
                    include_deferred_content,
                ));
                sequence += 1;
            }
            "turn/start" | "turn/end" | "step/start" | "step/end" => {
                let name = match kind {
                    "turn/end" => data
                        .get("reason")
                        .and_then(|reason| {
                            reason
                                .get("kind")
                                .and_then(Value::as_str)
                                .or_else(|| reason.as_str())
                        })
                        .map(|reason| format!("turn_{reason}"))
                        .unwrap_or_else(|| "turn_end".to_string()),
                    _ => kind.replace('/', "_"),
                };
                events.push(semantic_event(
                    sequence,
                    EventKind::SystemStatus,
                    &occurred_at,
                    None,
                    Some(name),
                    None,
                    structural_details(line, &value),
                ));
                sequence += 1;
            }
            "todo/write" => {
                events.push(semantic_event(
                    sequence,
                    EventKind::Plan,
                    &occurred_at,
                    Some(EventActor::Assistant),
                    Some("todo_write".to_string()),
                    None,
                    data.clone(),
                ));
                sequence += 1;
            }
            "assistant/chunk" | "request/context" | "session/end-seed" => {}
            raw_kind => {
                events.push(unadapted_event(
                    sequence,
                    &occurred_at,
                    raw_kind,
                    structural_details(line, &value),
                ));
                diagnostics.push(ConversationIndexIssue {
                    path: path.to_string_lossy().to_string(),
                    message: format!("DSH 压缩会话第 {} 行事件类型尚未适配", line + 1),
                    event_type: Some("dsh_event".to_string()),
                    line: Some((line + 1) as u64),
                });
                sequence += 1;
            }
        }
    }

    append_capability_degradation_status(sequence, &messages, &model, &mut events);
    let parsed = finish_source_conversation(
        Source::Dsh,
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

fn decode_values(path: &Path) -> Result<Vec<(usize, Value)>, String> {
    let bytes = fs::read(path).map_err(|error| format!("读取 DSH 压缩会话失败：{error}"))?;
    let decoded = zstd::decode_all(bytes.as_slice())
        .map_err(|error| format!("解压 DSH 会话失败：{error}"))?;
    let content =
        String::from_utf8(decoded).map_err(|error| format!("DSH 解压内容不是 UTF-8：{error}"))?;
    content
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(line, raw)| {
            serde_json::from_str(raw.trim())
                .map(|value| (line, value))
                .map_err(|error| format!("DSH 解压内容第 {} 行 JSON 无效：{error}", line + 1))
        })
        .collect()
}

fn dsh_tool_call_text(data: &Value) -> Option<String> {
    let arguments = data.get("arguments")?.as_str()?;
    let parsed = serde_json::from_str::<Value>(arguments).ok()?;
    optional_text(
        &parsed,
        &[
            "file_path",
            "path",
            "command",
            "query",
            "pattern",
            "description",
        ],
    )
}

fn normalize_dsh_tool_call(data: &Value) -> Value {
    let mut details = data.clone();
    if let Value::Object(object) = &mut details {
        if let Some(call_id) = object.get("callId").cloned() {
            object.insert("call_id".to_string(), call_id);
        }
    }
    details
}

fn normalize_dsh_tool_result(data: &Value) -> Value {
    let mut details = data.clone();
    if let Value::Object(object) = &mut details {
        let message = object.get("message").cloned().unwrap_or(Value::Null);
        if let Some(call_id) = message
            .pointer("/source/callId")
            .or_else(|| message.pointer("/content/0/toolCallId"))
            .or_else(|| message.get("toolCallId"))
            .or_else(|| message.get("tool_use_id"))
            .cloned()
        {
            object.insert("call_id".to_string(), call_id);
        }
        let output = message
            .get("content")
            .map(content_text)
            .filter(|text| !text.is_empty());
        if let Some(output) = output {
            object.insert("output".to_string(), Value::String(output));
        }
    }
    details
}

fn structural_details(line: usize, value: &Value) -> Value {
    json!({
        "line": line + 1,
        "type": value.get("type").and_then(Value::as_str),
        "seq": value.get("seq"),
    })
}

fn dsh_timestamp(value: &Value) -> String {
    value
        .get("time")
        .or_else(|| value.get("createdAt"))
        .and_then(Value::as_i64)
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|timestamp| timestamp.to_rfc3339())
        .unwrap_or_default()
}
