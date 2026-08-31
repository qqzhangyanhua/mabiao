use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::toolbox::*;
use super::{
    diagnostic_detail, diagnostic_index, discover_jsonl, regular_source_revision,
    ConversationIndexBatch, ConversationIndexIssue,
};
use crate::adapters::project::decode_url_dir;
use crate::ingest;

#[cfg(test)]
#[path = "grok_test.rs"]
mod tests;

#[derive(Default)]
struct ChunkAggregate {
    first_line: usize,
    text: String,
}

struct FallbackChunkStream {
    stream_signature: String,
    stream_key: String,
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    diagnostic_index(path, "grok_updates", parse)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    diagnostic_detail(path, session_id, include_deferred_content, parse)
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    let summary = summary_path(path);
    validate_summary(&summary)?;
    Ok(format!(
        "{}:{}",
        regular_source_revision(path)?,
        ingest::content_fingerprint(&summary)
    ))
}

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(discover_jsonl(roots)?
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some("updates.jsonl"))
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
        .ok_or_else(|| "Grok update 路径缺少必需的会话 ID".to_string())?
        .to_string();
    let project = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map(decode_url_dir)
        .unwrap_or_default();
    let summary = validate_summary(&summary_path(path))?;
    let mut model = summary
        .get("current_model_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let values = parse_jsonl_conversation_values(path)?;
    let latest = latest_identities(&values);
    let (chunk_keys, chunks) = aggregate_chunks(&values);
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut diagnostics = Vec::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut sequence = 0usize;

    for (line, value) in values {
        let params = value.get("params").unwrap_or(&Value::Null);
        let update = params.get("update").unwrap_or(&Value::Null);
        let raw_kind = update
            .get("sessionUpdate")
            .and_then(Value::as_str)
            .unwrap_or("");
        let dedup_identity = identity(&value, raw_kind);
        if dedup_identity
            .as_ref()
            .is_some_and(|identity| latest.get(identity).copied() != Some(line))
        {
            continue;
        }
        let native_identity = if is_chunk(raw_kind) {
            chunk_keys.get(&line).cloned()
        } else {
            dedup_identity
        };
        let occurred_at = grok_timestamp(&value, params);
        update_time_bounds(&occurred_at, &mut started_at, &mut ended_at);
        let event_start = events.len();

        if is_chunk(raw_kind) {
            let key = chunk_keys
                .get(&line)
                .expect("chunk key must exist for every chunk");
            let chunk = chunks.get(key).expect("chunk aggregate must exist");
            if chunk.first_line != line {
                continue;
            }
            match raw_kind {
                "user_message_chunk" => push_projected_message(
                    sequence,
                    &occurred_at,
                    "user",
                    &Value::String(chunk.text.clone()),
                    structural_details(line, raw_kind, &value),
                    &mut messages,
                    &mut events,
                ),
                "agent_message_chunk" => push_projected_message(
                    sequence,
                    &occurred_at,
                    "assistant",
                    &Value::String(chunk.text.clone()),
                    structural_details(line, raw_kind, &value),
                    &mut messages,
                    &mut events,
                ),
                "agent_thought_chunk" => events.push(semantic_event(
                    sequence,
                    EventKind::Plan,
                    &occurred_at,
                    Some(EventActor::Assistant),
                    Some("agent_thought".to_string()),
                    Some(chunk.text.clone()),
                    structural_details(line, raw_kind, &value),
                )),
                _ => unreachable!(),
            }
            tag_source_events(&mut events[event_start..], line, native_identity.as_deref());
            sequence += 1;
            continue;
        }

        let role = update.get("role").and_then(Value::as_str);
        match (raw_kind, role) {
            ("tool_call", _) => {
                events.push(semantic_event(
                    sequence,
                    EventKind::ToolCall,
                    &occurred_at,
                    Some(EventActor::Assistant),
                    optional_text(update, &["title", "name", "tool_name"]),
                    grok_tool_call_text(update),
                    normalize_grok_tool_call(update),
                ));
                sequence += 1;
            }
            ("tool_call_update", _) => {
                let status = optional_text(update, &["status"]).unwrap_or_default();
                let details = normalize_grok_tool_result(update);
                if !content_text(update.get("content").unwrap_or(&Value::Null)).is_empty()
                    && matches!(status.as_str(), "completed" | "failed")
                {
                    events.push(tool_result_event(
                        sequence,
                        &occurred_at,
                        &details,
                        include_deferred_content,
                    ));
                } else {
                    events.push(semantic_event(
                        sequence,
                        EventKind::SystemStatus,
                        &occurred_at,
                        None,
                        Some(if status.is_empty() {
                            "tool_call_update".to_string()
                        } else {
                            status
                        }),
                        None,
                        details,
                    ));
                }
                sequence += 1;
            }
            ("turn_completed", _) => {
                events.push(semantic_event(
                    sequence,
                    EventKind::SystemStatus,
                    &occurred_at,
                    None,
                    Some("turn_completed".to_string()),
                    optional_text(update, &["stop_reason"]),
                    update.clone(),
                ));
                sequence += 1;
            }
            ("usage_snapshot", _) => {
                events.push(semantic_event(
                    sequence,
                    EventKind::SystemStatus,
                    &occurred_at,
                    None,
                    Some("usage_snapshot".to_string()),
                    None,
                    update.clone(),
                ));
                sequence += 1;
            }
            ("plan", _) => {
                events.push(semantic_event(
                    sequence,
                    EventKind::Plan,
                    &occurred_at,
                    Some(EventActor::Assistant),
                    Some("plan".to_string()),
                    optional_text(update, &["text", "message"]),
                    update.clone(),
                ));
                sequence += 1;
            }
            (
                "available_commands_update"
                | "current_mode_update"
                | "config_option_update"
                | "session_info_update",
                _,
            ) => {
                events.push(semantic_event(
                    sequence,
                    EventKind::SystemStatus,
                    &occurred_at,
                    None,
                    Some(raw_kind.to_string()),
                    None,
                    update.clone(),
                ));
                sequence += 1;
            }
            (_, Some("user")) => {
                push_projected_message(
                    sequence,
                    &occurred_at,
                    "user",
                    update
                        .get("content")
                        .or_else(|| update.get("text"))
                        .unwrap_or(&Value::Null),
                    structural_details(line, raw_kind, &value),
                    &mut messages,
                    &mut events,
                );
                sequence += 1;
            }
            (_, Some("assistant")) => {
                push_projected_message(
                    sequence,
                    &occurred_at,
                    "assistant",
                    update
                        .get("content")
                        .or_else(|| update.get("text"))
                        .unwrap_or(&Value::Null),
                    structural_details(line, raw_kind, &value),
                    &mut messages,
                    &mut events,
                );
                sequence += 1;
            }
            ("", _) if update.pointer("/_meta/modelId").is_some() => {
                let next_model = update
                    .pointer("/_meta/modelId")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if model.is_empty() && !next_model.is_empty() {
                    model = next_model.to_string();
                }
                events.push(semantic_event(
                    sequence,
                    EventKind::ModelChange,
                    &occurred_at,
                    None,
                    (!next_model.is_empty()).then(|| next_model.to_string()),
                    None,
                    structural_details(line, "model_update", &value),
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
                        "Grok update 第 {} 行事件类型 {} 尚未适配",
                        line + 1,
                        display_kind(raw_kind)
                    ),
                    event_type: Some("grok_update_event".to_string()),
                    line: Some((line + 1) as u64),
                });
                sequence += 1;
            }
        }
        tag_source_events(&mut events[event_start..], line, native_identity.as_deref());
    }

    assign_native_event_ids(&mut events, Source::Grok, &session_id);
    append_capability_degradation_status(sequence, &messages, &model, &mut events);
    let parsed = finish_source_conversation(
        Source::Grok,
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

fn latest_identities(values: &[(usize, Value)]) -> BTreeMap<String, usize> {
    values
        .iter()
        .filter_map(|(line, value)| {
            let update = value.pointer("/params/update").unwrap_or(&Value::Null);
            let kind = update
                .get("sessionUpdate")
                .and_then(Value::as_str)
                .unwrap_or("");
            identity(value, kind).map(|identity| (identity, *line))
        })
        .collect()
}

fn identity(value: &Value, kind: &str) -> Option<String> {
    let update = value.pointer("/params/update").unwrap_or(&Value::Null);
    if kind == "turn_completed" {
        return optional_text(update, &["prompt_id"])
            .or_else(|| {
                value
                    .pointer("/params/_meta/promptId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .map(|id| format!("turn_completed:{id}"));
    }
    if kind == "tool_call_update" {
        return optional_text(update, &["toolCallId", "tool_call_id", "id"])
            .map(|id| format!("tool_call_update:{id}"));
    }
    if is_chunk(kind) {
        return None;
    }
    value
        .pointer("/params/_meta/eventId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(|id| format!("event:{id}"))
}

fn aggregate_chunks(
    values: &[(usize, Value)],
) -> (BTreeMap<usize, String>, BTreeMap<String, ChunkAggregate>) {
    let mut line_keys = BTreeMap::new();
    let mut chunks = BTreeMap::<String, ChunkAggregate>::new();
    let mut fallback_stream: Option<FallbackChunkStream> = None;
    for (line, value) in values {
        let kind = value
            .pointer("/params/update/sessionUpdate")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !is_chunk(kind) {
            fallback_stream = None;
            continue;
        }
        let message_id = value
            .pointer("/params/update/message_id")
            .or_else(|| value.pointer("/params/update/messageId"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        let key = if let Some(message_id) = message_id {
            fallback_stream = None;
            format!("{kind}:message:{message_id}")
        } else if let Some(prompt_id) = value
            .pointer("/params/update/prompt_id")
            .or_else(|| value.pointer("/params/_meta/promptId"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            let stream_signature = format!("{kind}:prompt:{prompt_id}");
            if let Some(current) = &fallback_stream {
                if current.stream_signature == stream_signature {
                    current.stream_key.clone()
                } else {
                    start_fallback_stream(value, *line, stream_signature, &mut fallback_stream)
                }
            } else {
                start_fallback_stream(value, *line, stream_signature, &mut fallback_stream)
            }
        } else {
            fallback_stream = None;
            value
                .pointer("/params/_meta/eventId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(|id| format!("{kind}:event:{id}"))
                .unwrap_or_else(|| format!("{kind}:line:{line}"))
        };
        line_keys.insert(*line, key.clone());
        let aggregate = chunks.entry(key).or_insert_with(|| ChunkAggregate {
            first_line: *line,
            text: String::new(),
        });
        aggregate.text.push_str(&content_text(
            value
                .pointer("/params/update/content")
                .unwrap_or(&Value::Null),
        ));
    }
    (line_keys, chunks)
}

fn start_fallback_stream(
    value: &Value,
    line: usize,
    stream_signature: String,
    fallback_stream: &mut Option<FallbackChunkStream>,
) -> String {
    let anchor = value
        .pointer("/params/_meta/eventId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("line-{line}"));
    let stream_key = format!("{stream_signature}:stream:{anchor}");
    *fallback_stream = Some(FallbackChunkStream {
        stream_signature,
        stream_key: stream_key.clone(),
    });
    stream_key
}

fn is_chunk(kind: &str) -> bool {
    matches!(
        kind,
        "user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk"
    )
}

fn grok_tool_call_text(update: &Value) -> Option<String> {
    let input = update
        .get("rawInput")
        .or_else(|| update.get("input"))
        .or_else(|| update.get("arguments"))?;
    optional_text(input, &["path", "file_path", "command", "query"])
}

fn normalize_grok_tool_call(update: &Value) -> Value {
    let mut details = update.clone();
    if let Value::Object(object) = &mut details {
        if let Some(call_id) = object
            .get("toolCallId")
            .or_else(|| object.get("tool_call_id"))
            .or_else(|| object.get("id"))
            .cloned()
        {
            object.insert("call_id".to_string(), call_id);
        }
    }
    details
}

fn normalize_grok_tool_result(update: &Value) -> Value {
    let mut details = normalize_tool_result_details(update);
    if let Value::Object(object) = &mut details {
        if !object.contains_key("output") {
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

fn summary_path(path: &Path) -> PathBuf {
    path.parent()
        .map(|parent| parent.join("summary.json"))
        .unwrap_or_default()
}

fn validate_summary(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(Value::Null);
    }
    let content = fs::read_to_string(path)
        .map_err(|error| format!("读取 Grok summary.json 失败：{error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("Grok summary.json 无效：{error}"))
}

fn structural_details(line: usize, kind: &str, value: &Value) -> Value {
    json!({
        "line": line + 1,
        "type": kind,
        "event_id": value.pointer("/params/_meta/eventId"),
        "prompt_id": value.pointer("/params/_meta/promptId"),
    })
}

fn display_kind(kind: &str) -> &str {
    if kind.is_empty() {
        "<missing>"
    } else {
        kind
    }
}

fn grok_timestamp(value: &Value, params: &Value) -> String {
    if let Some(timestamp) = value.get("timestamp") {
        if let Some(text) = timestamp.as_str() {
            return text.to_string();
        }
        if let Some(raw) = timestamp.as_f64() {
            let milliseconds = if raw > 100_000_000_000.0 {
                raw as i64
            } else {
                (raw * 1000.0) as i64
            };
            return chrono::DateTime::from_timestamp_millis(milliseconds)
                .map(|timestamp| timestamp.to_rfc3339())
                .unwrap_or_default();
        }
    }
    params
        .pointer("/_meta/agentTimestampMs")
        .and_then(Value::as_i64)
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|timestamp| timestamp.to_rfc3339())
        .unwrap_or_default()
}
