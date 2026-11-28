//! 对话记录的通用归一化与会话收尾管线。
//!
//! 负责 `ParsedConversation` / `FileIndexCursor` 类型、原生身份与来源归属（provenance）、
//! 能力降级状态、时间排序、标题清洗、JSONL 流式读取与统一会话收尾；
//! 通用事件与工具构造留在 `events` 子模块并在此统一对外 re-export。
//! 附件发现与加载在 `attachments`；Responses / Codex rollout 线格式投影在 `responses_wire`。

mod events;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use base64::prelude::*;
use serde_json::Value;

use crate::domain::{
    ConversationEvent, ConversationEventCapabilityStatus as EventStatus,
    ConversationEventKind as EventKind, ConversationMessage, ConversationSessionRow, Source,
};

use super::attachments::{payload_has_local_media, populate_attachments};

use events::assign_tool_result_names;
pub(crate) use events::{
    compression_boundary_text, message_event, normalize_tool_call_details,
    normalize_tool_result_details, semantic_event, tool_payload_failed, tool_result_event,
    unadapted_event,
};

const TITLE_MAX_CHARS: usize = 80;
pub(crate) const CAPABILITY_MESSAGES: &str = "messages";
pub(crate) const CAPABILITY_EVENTS: &str = "events";
pub(crate) const CAPABILITY_USAGE: &str = "usage";

pub(crate) const EXPERIMENTAL: &str = "experimental";

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

fn assign_native_event_ids(events: &mut [ConversationEvent], source: Source, session_id: &str) {
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
    if text.is_empty() && !payload_has_local_media(content) && !payload_has_local_media(&details) {
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

fn append_capability_degradation_status(
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

fn append_declared_capability_degradation_status(
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

/// 共享收尾之前要不要按已打的 native_id 分配原生事件 id。
pub(crate) enum NativeEventIdPrep {
    Skip,
    Assign,
}

/// 共享收尾之前要不要追加能力降级状态。
pub(crate) enum CapabilityDegradationPrep<'a> {
    None,
    /// 根据消息、模型和工具配对推断缺失能力。
    Inferred {
        sequence: usize,
    },
    /// 追加调用方声明的缺失能力。
    Declared {
        sequence: usize,
        missing: &'a [&'a str],
    },
}

/// 共享收尾之前的准备模式。新来源选两个小枚举即可，不必记住该调哪几步。
///
/// `tag_source_events` 仍留在各对话记录适配器的逐行解析里：它依赖每条记录的行号和原生身份，
/// 不能收进这份批量钩子。
pub(crate) struct ConversationFinishPrep<'a> {
    pub native_ids: NativeEventIdPrep,
    pub degradation: CapabilityDegradationPrep<'a>,
}

impl ConversationFinishPrep<'static> {
    pub const NONE: Self = Self {
        native_ids: NativeEventIdPrep::Skip,
        degradation: CapabilityDegradationPrep::None,
    };
}

fn apply_conversation_finish_prep(
    source: Source,
    session_id: &str,
    messages: &[ConversationMessage],
    model: &str,
    events: &mut Vec<ConversationEvent>,
    prep: ConversationFinishPrep<'_>,
) {
    let assign_native_ids = matches!(prep.native_ids, NativeEventIdPrep::Assign);
    let assign_before_degradation =
        !matches!(prep.degradation, CapabilityDegradationPrep::Declared { .. });
    if assign_native_ids && assign_before_degradation {
        assign_native_event_ids(events, source, session_id);
    }
    match prep.degradation {
        CapabilityDegradationPrep::None => {}
        CapabilityDegradationPrep::Inferred { sequence } => {
            append_capability_degradation_status(sequence, messages, model, events);
        }
        CapabilityDegradationPrep::Declared { sequence, missing } => {
            append_declared_capability_degradation_status(sequence, missing, events);
        }
    }
    if assign_native_ids && !assign_before_degradation {
        assign_native_event_ids(events, source, session_id);
    }
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
    prep: ConversationFinishPrep<'_>,
) -> Result<ParsedConversation, String> {
    if session_id.is_empty() {
        return Err(format!("缺少 {} 会话 ID", source.application_name()));
    }
    apply_conversation_finish_prep(source, &session_id, &messages, &model, &mut events, prep);
    populate_attachments(&mut events, &project);
    strip_message_bodies_from_details(&mut events);
    deduplicate_message_channels(&mut events);
    assign_tool_result_names(&mut events);
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

pub(crate) fn event_id_for(source_file: &str, source_sequence: u32) -> String {
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

fn deduplicate_message_channels(events: &mut Vec<ConversationEvent>) {
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

fn strip_message_bodies_from_details(events: &mut [ConversationEvent]) {
    for event in events {
        if event.kind != EventKind::Message {
            continue;
        }
        if let Value::Object(object) = &mut event.details {
            object.remove("content");
            object.remove("message");
            object.remove("attachments");
            object.remove("parts");
            object.remove("inlineData");
            object.remove("inline_data");
        }
    }
}

pub(crate) fn optional_text(value: &Value, keys: &[&str]) -> Option<String> {
    let text = first_text(value, keys);
    (!text.is_empty()).then_some(text)
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
