//! 源文件变成对话记录时用的内部工具箱。
//!
//! 覆盖事件构造、jsonl 读盘、会话收尾、附件元数据和 provenance。
//! 目录 SQL、多文件 merge、详情读附件字节、Cursor 挂接仍留在 catalog。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use base64::prelude::*;
use serde_json::Value;

pub(crate) use crate::domain::{
    ConversationAttachment, ConversationAttachmentKind as AttachmentKind,
    ConversationAttachmentStatus as AttachmentStatus, ConversationEvent,
    ConversationEventActor as EventActor, ConversationEventCapabilityStatus as EventStatus,
    ConversationEventContentStatus as ContentStatus, ConversationEventKind as EventKind,
    ConversationMessage, ConversationSessionRow, Source,
};

const TITLE_MAX_CHARS: usize = 80;
pub(crate) const CAPABILITY_MESSAGES: &str = "messages";
pub(crate) const CAPABILITY_EVENTS: &str = "events";
pub(crate) const CAPABILITY_USAGE: &str = "usage";

pub(crate) const EXPERIMENTAL: &str = "experimental";

const LARGE_CONTENT_THRESHOLD: usize = 4_096;
const CONTENT_PREVIEW_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy)]
pub(crate) struct FileIndexCursor {
    pub byte_offset: i64,
    pub line: i64,
}

pub(crate) struct ParsedConversation {
    pub(crate) session: ConversationSessionRow,
    pub(crate) messages: Vec<ConversationMessage>,
    pub(crate) events: Vec<ConversationEvent>,
    pub(crate) is_top_level: bool,
    pub(crate) index_cursor: Option<FileIndexCursor>,
}

pub(crate) struct PendingMessageDelta {
    sequence: u32,
    occurred_at: String,
    role: String,
    text: String,
}

pub(crate) fn tag_source_events(
    events: &mut [ConversationEvent],
    source_sequence: usize,
    native_identity: Option<&str>,
) {
    for event in events {
        event.source_sequence = source_sequence as u32;
        if let (Some(native_identity), Value::Object(details)) =
            (native_identity, &mut event.details)
        {
            details.insert(
                "native_id".to_string(),
                Value::String(native_identity.to_string()),
            );
        }
    }
}

pub(crate) fn assign_native_event_ids(
    events: &mut [ConversationEvent],
    source: Source,
    session_id: &str,
) {
    for event in events {
        if !event.event_id.is_empty() {
            continue;
        }
        if let Some(native_id) = optional_text(
            &event.details,
            &[
                "native_id",
                "call_id",
                "message_id",
                "prompt_id",
                "event_id",
            ],
        ) {
            event.event_id = format!(
                "{}:{session_id}:{}:{native_id}",
                source.as_str(),
                event.kind.as_str()
            );
        }
    }
}

pub(crate) fn assign_event_provenance(events: &mut [ConversationEvent], source_file: &str) {
    let mut occurrences = BTreeMap::<u32, u32>::new();
    for event in events {
        let occurrence = occurrences.entry(event.source_sequence).or_default();
        event.source_file = source_file.to_string();
        if event.event_id.is_empty() {
            let base_id = event_id_for(source_file, event.source_sequence);
            event.event_id = if *occurrence == 0 {
                base_id
            } else {
                format!("{base_id}:{}", *occurrence)
            };
        }
        *occurrence += 1;
        for (index, attachment) in event.attachments.iter_mut().enumerate() {
            attachment.id = format!("{}:{index}", event.event_id);
        }
    }
}

/// 这 7 个来源（claude/pi/cursor/kimi/grok/droid/copilot）后续都要按 id/prompt 做
/// 「只留最后一次」的去重或跨行聚合，天然需要整份 `Vec<(usize, Value)>` 常驻——这块内存
/// 省不掉。但「先把整份文件读成一份 `String`，再逐行解析出第二份 `Value` 树」等于同时
/// 攥着两份内容，其中原始文本那份纯属浪费：按行流式读盘，只让当前这一行的原始文本活着，
/// 能把这一步的峰值内存打个对折（省掉的正是原始文件那一份）。
pub(crate) fn parse_jsonl_conversation_values(path: &Path) -> Result<Vec<(usize, Value)>, String> {
    let file = fs::File::open(path).map_err(|error| format!("读取原始文件失败：{error}"))?;
    BufReader::new(file)
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let raw = match line {
                Ok(raw) => raw,
                Err(error) => return Some(Err(format!("第 {} 行读取失败：{error}", index + 1))),
            };
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            Some(
                serde_json::from_str(trimmed)
                    .map(|value| (index, value))
                    .map_err(|error| format!("第 {} 行 JSON 无效：{error}", index + 1)),
            )
        })
        .collect()
}

pub(crate) fn update_time_bounds(timestamp: &str, started_at: &mut String, ended_at: &mut String) {
    if timestamp.is_empty() {
        return;
    }
    if started_at.is_empty() || compare_timestamps(timestamp, started_at).is_lt() {
        *started_at = timestamp.to_string();
    }
    if ended_at.is_empty() || compare_timestamps(timestamp, ended_at).is_gt() {
        *ended_at = timestamp.to_string();
    }
}

pub(crate) fn push_projected_message(
    sequence: usize,
    timestamp: &str,
    role: &str,
    content: &Value,
    details: Value,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
) {
    let text = content_text(content);
    if text.is_empty() {
        return;
    }
    let message = ConversationMessage {
        role: role.to_string(),
        occurred_at: timestamp.to_string(),
        text,
    };
    events.push(message_event(sequence, &message, details));
    messages.push(message);
}

pub(crate) fn append_capability_degradation_status(
    sequence: usize,
    messages: &[ConversationMessage],
    model: &str,
    events: &mut Vec<ConversationEvent>,
) {
    let mut missing = Vec::new();
    if !messages.iter().any(|message| message.role == "user") {
        missing.push("user_message");
    }
    if model.is_empty() {
        missing.push("model");
    }
    let tool_results = events
        .iter()
        .filter(|event| event.kind == EventKind::ToolResult)
        .filter_map(|event| event.details.get("call_id").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    if events
        .iter()
        .filter(|event| event.kind == EventKind::ToolCall)
        .any(|event| {
            event
                .details
                .get("call_id")
                .and_then(Value::as_str)
                .is_none_or(|call_id| !tool_results.contains(call_id))
        })
    {
        missing.push("tool_result");
    }
    if events.iter().any(|event| {
        matches!(
            event.capability_status,
            EventStatus::MissingTimestamp | EventStatus::UnadaptedMissingTimestamp
        )
    }) {
        missing.push("timestamp");
    }
    append_declared_capability_degradation_status(sequence, &missing, events);
}

pub(crate) fn append_declared_capability_degradation_status(
    sequence: usize,
    missing: &[&str],
    events: &mut Vec<ConversationEvent>,
) {
    if missing.is_empty() {
        return;
    }
    let occurred_at = events
        .iter()
        .filter_map(|event| event.occurred_at.as_deref())
        .max()
        .unwrap_or("");
    events.push(semantic_event(
        sequence,
        EventKind::SystemStatus,
        occurred_at,
        None,
        Some("capability_degraded".to_string()),
        Some(missing.join(", ")),
        serde_json::json!({ "missing": missing }),
    ));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn finish_source_conversation(
    source: Source,
    path: &Path,
    session_id: String,
    mut title: String,
    project: String,
    model: String,
    started_at: String,
    ended_at: String,
    messages: Vec<ConversationMessage>,
    mut events: Vec<ConversationEvent>,
    is_top_level: bool,
) -> Result<ParsedConversation, String> {
    if session_id.is_empty() {
        return Err(format!("缺少 {} 会话 ID", source.application_name()));
    }
    populate_attachments(&mut events, &project);
    strip_message_bodies_from_details(&mut events);
    deduplicate_message_channels(&mut events);
    let source_file = path.to_string_lossy().to_string();
    assign_event_provenance(&mut events, &source_file);
    events.sort_by(compare_event_order);
    if title.is_empty() {
        title = messages
            .iter()
            .find(|message| message.role == "user")
            .map(|message| truncate_title(&strip_prompt_wrappers(&message.text)))
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| session_id.clone());
    }
    let mut capabilities = Vec::new();
    if !messages.is_empty() {
        capabilities.push(CAPABILITY_MESSAGES.to_string());
    }
    if !events.is_empty() {
        capabilities.push(CAPABILITY_EVENTS.to_string());
    }
    // Capabilities describe supported detail surfaces; an empty usage list is valid data, not missing capability.
    capabilities.push(CAPABILITY_USAGE.to_string());
    Ok(ParsedConversation {
        session: ConversationSessionRow {
            source: source.as_str().to_string(),
            session_id,
            title,
            project,
            model,
            started_at,
            ended_at,
            source_file: source_file.clone(),
            source_files: vec![source_file],
            capabilities,
            support_status: EXPERIMENTAL.to_string(),
            file_available: true,
            ..Default::default()
        },
        messages,
        events,
        is_top_level,
        index_cursor: None,
    })
}

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

fn event_id_for(source_file: &str, source_sequence: u32) -> String {
    format!(
        "{}:{source_sequence}",
        BASE64_URL_SAFE_NO_PAD.encode(source_file.as_bytes())
    )
}

pub(crate) fn compare_event_timestamps(
    left: &ConversationEvent,
    right: &ConversationEvent,
) -> std::cmp::Ordering {
    compare_optional_timestamps(&left.occurred_at, &right.occurred_at)
}

/// 缺时间的事件一律排在有时间的之后。
pub(crate) fn compare_optional_timestamps(
    left: &Option<String>,
    right: &Option<String>,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left_time), Some(right_time)) => compare_timestamps(left_time, right_time),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageChannel {
    Response,
    Event,
    Delta,
}

pub(crate) fn deduplicate_message_channels(events: &mut Vec<ConversationEvent>) {
    let mut current_actor = None;
    let mut seen: Vec<(String, MessageChannel)> = Vec::new();
    events.retain(|event| {
        if event.kind != EventKind::Message {
            return true;
        }
        let Some(actor) = event.actor.as_ref() else {
            return true;
        };
        let Some(text) = event.text.as_ref() else {
            return true;
        };
        if current_actor.as_ref() != Some(actor) {
            current_actor = Some(*actor);
            seen.clear();
        }
        let channel = match event.details.get("type").and_then(Value::as_str) {
            Some("message") => MessageChannel::Response,
            Some("user_message" | "agent_message") => MessageChannel::Event,
            _ => MessageChannel::Delta,
        };
        if seen
            .iter()
            .any(|(seen_text, seen_channel)| seen_text == text && *seen_channel != channel)
        {
            return false;
        }
        seen.push((text.clone(), channel));
        true
    });
}

pub(crate) fn compare_event_order(
    left: &ConversationEvent,
    right: &ConversationEvent,
) -> std::cmp::Ordering {
    match (&left.occurred_at, &right.occurred_at) {
        (Some(left_time), Some(right_time)) => compare_timestamps(left_time, right_time)
            .then_with(|| left.sequence.cmp(&right.sequence)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.sequence.cmp(&right.sequence),
    }
}

pub(crate) fn compare_timestamps(left: &str, right: &str) -> std::cmp::Ordering {
    match (
        chrono::DateTime::parse_from_rfc3339(left),
        chrono::DateTime::parse_from_rfc3339(right),
    ) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
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

pub(crate) fn response_semantic_event(
    sequence: usize,
    occurred_at: &str,
    payload: &Value,
    include_deferred_content: bool,
) -> Option<ConversationEvent> {
    let kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "message" => None,
        "function_call" | "custom_tool_call" | "web_search_call" | "local_shell_call" => {
            Some(semantic_event(
                sequence,
                EventKind::ToolCall,
                occurred_at,
                Some(EventActor::Assistant),
                optional_text(payload, &["name", "tool", "type"]),
                optional_text(payload, &["arguments", "input", "query", "command"]),
                payload.clone(),
            ))
        }
        "function_call_output" | "custom_tool_call_output" => Some(tool_result_event(
            sequence,
            occurred_at,
            payload,
            include_deferred_content,
        )),
        "reasoning" => Some(semantic_event(
            sequence,
            EventKind::Plan,
            occurred_at,
            Some(EventActor::Assistant),
            None,
            optional_text(payload, &["summary", "text", "content"]),
            payload.clone(),
        )),
        "developer" | "system" => None,
        _ => Some(unadapted_event(
            sequence,
            occurred_at,
            kind,
            payload.clone(),
        )),
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
        EventKind::ToolResult,
        occurred_at,
        Some(EventActor::Tool),
        optional_text(payload, &["name"]),
        rendered_text,
        details,
    );
    if should_defer {
        event.content_status = ContentStatus::Deferred;
    }
    event
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
        "error" | "stream_error" => semantic_event(
            sequence,
            EventKind::Error,
            occurred_at,
            None,
            optional_text(payload, &["code", "type"]),
            optional_text(payload, &["message", "error"]),
            payload.clone(),
        ),
        "task_started" | "task_complete" | "turn_aborted" | "context_compacted" | "warning" => {
            semantic_event(
                sequence,
                EventKind::SystemStatus,
                occurred_at,
                None,
                Some(kind.to_string()),
                optional_text(payload, &["message", "reason", "text"]),
                payload.clone(),
            )
        }
        _ => unadapted_event(sequence, occurred_at, kind, payload.clone()),
    }
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

pub(crate) struct AttachmentCandidate {
    pub(crate) attachment: ConversationAttachment,
    pub(crate) source: String,
    pub(crate) resolved_path: Option<PathBuf>,
}

pub(crate) fn populate_attachments(events: &mut [ConversationEvent], project: &str) {
    for event in events {
        event.attachments = attachment_candidates(event.sequence, &event.details, project)
            .into_iter()
            .map(|candidate| candidate.attachment)
            .collect();
    }
}

pub(crate) fn strip_message_bodies_from_details(events: &mut [ConversationEvent]) {
    for event in events {
        if event.kind != EventKind::Message {
            continue;
        }
        if let Value::Object(object) = &mut event.details {
            object.remove("content");
            object.remove("message");
            object.remove("attachments");
        }
    }
}

pub(crate) fn attachment_candidates(
    sequence: u32,
    payload: &Value,
    project: &str,
) -> Vec<AttachmentCandidate> {
    let mut values = Vec::new();
    for key in ["content", "attachments"] {
        match payload.get(key) {
            Some(Value::Array(items)) => values.extend(items),
            Some(value @ Value::Object(_)) => values.push(value),
            _ => {}
        }
    }
    values
        .into_iter()
        .filter_map(|value| attachment_candidate(value, project))
        .enumerate()
        .map(|(index, mut candidate)| {
            candidate.attachment.id = format!("{sequence}:{index}");
            candidate
        })
        .collect()
}

fn attachment_candidate(value: &Value, project: &str) -> Option<AttachmentCandidate> {
    let object = value.as_object()?;
    let raw_type = object.get("type").and_then(Value::as_str).unwrap_or("");
    let kind = if raw_type.contains("image") {
        AttachmentKind::Image
    } else if raw_type.contains("file") || raw_type.contains("attachment") {
        AttachmentKind::File
    } else {
        return None;
    };
    let source = ["file_path", "path", "url", "image_url"]
        .iter()
        .find_map(|key| object.get(*key).and_then(attachment_source_value))?;
    let embedded = source.starts_with("data:");
    let remote = source.starts_with("http://") || source.starts_with("https://");
    let resolved_path = if embedded || remote {
        None
    } else {
        let path = PathBuf::from(&source);
        Some(if path.is_absolute() || project.is_empty() {
            path
        } else {
            PathBuf::from(project).join(path)
        })
    };
    let metadata = resolved_path
        .as_ref()
        .and_then(|path| fs::metadata(path).ok());
    let status = if embedded {
        AttachmentStatus::Embedded
    } else if remote {
        AttachmentStatus::Unsupported
    } else if metadata.is_some() {
        AttachmentStatus::Available
    } else {
        AttachmentStatus::Missing
    };
    let original_path = if embedded {
        "内嵌图片数据".to_string()
    } else {
        source.clone()
    };
    let name = first_text(value, &["name", "file_name"]);
    let name = if name.is_empty() {
        Path::new(&original_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(if kind == AttachmentKind::Image {
                "image"
            } else {
                "attachment"
            })
            .to_string()
    } else {
        name
    };
    let media_type = optional_text(value, &["mime_type", "media_type"])
        .unwrap_or_else(|| infer_media_type(&name, kind));
    Some(AttachmentCandidate {
        attachment: ConversationAttachment {
            id: String::new(),
            kind,
            name,
            original_path,
            media_type,
            size_bytes: metadata.map(|metadata| metadata.len()),
            status,
        },
        source,
        resolved_path,
    })
}

fn attachment_source_value(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.get("url").and_then(Value::as_str).map(str::to_string))
}

fn infer_media_type(name: &str, kind: AttachmentKind) -> String {
    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "md" => "text/markdown",
        "txt" => "text/plain",
        _ if kind == AttachmentKind::Image => "image/*",
        _ => "application/octet-stream",
    }
    .to_string()
}

pub(crate) fn optional_text(value: &Value, keys: &[&str]) -> Option<String> {
    let text = first_text(value, keys);
    (!text.is_empty()).then_some(text)
}

pub(crate) fn response_message(payload: &Value, occurred_at: &str) -> Option<ConversationMessage> {
    if payload.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    let role = payload.get("role").and_then(Value::as_str)?;
    if role != "user" && role != "assistant" {
        return None;
    }
    message(role, occurred_at, payload.get("content")?)
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

pub(crate) fn content_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(content_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                [
                    "content",
                    "output",
                    "result",
                    "response",
                    "functionResponse",
                ]
                .iter()
                .find_map(|key| object.get(*key).map(content_text))
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub(crate) fn first_text(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

pub(crate) fn text_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

pub(crate) fn strip_prompt_wrappers(text: &str) -> String {
    let mut remaining = text;
    let mut stripped = String::new();
    while let Some(start) = remaining.find("<timestamp>") {
        stripped.push_str(&remaining[..start]);
        let after = &remaining[start + "<timestamp>".len()..];
        match after.find("</timestamp>") {
            Some(end) => remaining = &after[end + "</timestamp>".len()..],
            None => {
                remaining = "";
                break;
            }
        }
    }
    stripped.push_str(remaining);
    stripped
        .replace("<user_query>", " ")
        .replace("</user_query>", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn truncate_title(text: &str) -> String {
    let mut chars = text.chars();
    let title: String = chars.by_ref().take(TITLE_MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{title}…")
    } else {
        title
    }
}
