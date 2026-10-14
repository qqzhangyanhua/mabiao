//! 把 OpenCode 快照投影成会话、消息和事件。

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

use serde_json::Value;

use super::super::toolbox::*;
use super::{DatabaseSnapshot, MessageRow, PartRow, SessionRow};

pub(super) fn project_snapshot(
    path: &Path,
    snapshot: DatabaseSnapshot,
    session_filter: Option<&str>,
    include_deferred_content: bool,
) -> Result<Vec<ParsedConversation>, String> {
    let capabilities = snapshot.capabilities;
    let mut messages_by_session = BTreeMap::<String, Vec<MessageRow>>::new();
    for message in snapshot.messages {
        messages_by_session
            .entry(message.session_id.clone())
            .or_default()
            .push(message);
    }
    let mut parts_by_message = BTreeMap::<String, Vec<PartRow>>::new();
    for part in snapshot.parts {
        parts_by_message
            .entry(part.message_id.clone())
            .or_default()
            .push(part);
    }

    snapshot
        .sessions
        .into_iter()
        .filter(|session| session_filter.is_none_or(|filter| session.id == filter))
        .map(|session| {
            let source_messages = messages_by_session.remove(&session.id).unwrap_or_default();
            project_session(
                path,
                session,
                source_messages,
                &mut parts_by_message,
                &capabilities,
                include_deferred_content,
            )
        })
        .collect()
}

fn project_session(
    path: &Path,
    session: SessionRow,
    mut source_messages: Vec<MessageRow>,
    parts_by_message: &mut BTreeMap<String, Vec<PartRow>>,
    capabilities: &[String],
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    if source_messages.is_empty() {
        source_messages = Vec::new();
    }
    source_messages.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut project = session.directory.clone();
    let mut model = String::new();
    let mut started_at = millis_timestamp(session.created_at);
    let mut ended_at = millis_timestamp(session.updated_at.or(session.created_at));
    let mut sequence = 0usize;

    for source_message in source_messages {
        let Some(data) = source_message.data.as_ref() else {
            let mut event = unadapted_event(
                sequence,
                &millis_timestamp(source_message.created_at),
                "invalid_message_json",
                serde_json::json!({"table":"message","row_id":source_message.id}),
            );
            set_native_event_id(path, &mut event, "message", &source_message.id, "invalid");
            events.push(event);
            sequence += 1;
            continue;
        };
        let role = data.get("role").and_then(Value::as_str).unwrap_or("");
        let message_time = data
            .get("time")
            .and_then(|time| time.get("created"))
            .and_then(Value::as_i64)
            .or(source_message.created_at);
        let timestamp = millis_timestamp(message_time);
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        if role == "assistant" {
            let next_model = first_text(data, &["modelID", "modelId"]);
            if !next_model.is_empty() {
                model = next_model;
            }
            if project.is_empty() {
                let path_data = data.get("path").unwrap_or(&Value::Null);
                project = first_text(path_data, &["root", "cwd"]);
            }
        }

        let mut parts = parts_by_message
            .remove(&source_message.id)
            .unwrap_or_default();
        parts.retain(|part| part.session_id == session.id);
        parts.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        let text = parts
            .iter()
            .filter_map(|part| part.data.as_ref())
            .filter_map(visible_text)
            .collect::<Vec<_>>()
            .join("\n");
        if matches!(role, "user" | "assistant") && !text.is_empty() {
            let message = ConversationMessage {
                role: role.to_string(),
                occurred_at: timestamp.clone(),
                text,
            };
            let mut event = message_event(
                sequence,
                &message,
                serde_json::json!({"message_id":source_message.id,"role":role}),
            );
            set_native_event_id(path, &mut event, "message", &source_message.id, "text");
            events.push(event);
            messages.push(message);
            sequence += 1;
        }
        for part in parts {
            let part_timestamp = millis_timestamp(part.created_at.or(message_time));
            update_time_bounds(&part_timestamp, &mut started_at, &mut ended_at);
            project_part(
                path,
                &part,
                &part_timestamp,
                &mut sequence,
                &mut events,
                include_deferred_content,
            );
        }
    }

    let mut parsed = finish_source_conversation(
        Source::Opencode,
        path,
        session.id,
        session.title,
        project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        session.parent_id.is_empty(),
        ConversationFinishPrep::NONE,
    )?;
    if !session.parent_id.is_empty() {
        parsed.events.insert(
            0,
            semantic_event(
                0,
                EventKind::SystemStatus,
                &parsed.session.started_at,
                None,
                Some("session_started".to_string()),
                None,
                serde_json::json!({ "parent_id": session.parent_id }),
            ),
        );
        assign_event_provenance(&mut parsed.events, &parsed.session.source_file);
    }
    parsed.session.capabilities = capabilities.to_vec();
    Ok(parsed)
}

fn project_part(
    path: &Path,
    part: &PartRow,
    timestamp: &str,
    sequence: &mut usize,
    events: &mut Vec<ConversationEvent>,
    include_deferred_content: bool,
) {
    let Some(data) = part.data.as_ref() else {
        let mut event = unadapted_event(
            *sequence,
            timestamp,
            "invalid_part_json",
            serde_json::json!({"table":"part","row_id":part.id,"message_id":part.message_id}),
        );
        set_native_event_id(path, &mut event, "part", &part.id, "invalid");
        events.push(event);
        *sequence += 1;
        return;
    };
    if data.get("synthetic").and_then(Value::as_bool) == Some(true)
        || data.get("ignored").and_then(Value::as_bool) == Some(true)
    {
        return;
    }
    let kind = safe_kind(
        data.get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
    );
    match kind.as_str() {
        "text" => {}
        "reasoning" => {
            let mut event = semantic_event(
                *sequence,
                EventKind::Plan,
                timestamp,
                Some(EventActor::Assistant),
                None,
                optional_text(data, &["text"]),
                serde_json::json!({"part_id":part.id,"type":"reasoning"}),
            );
            set_native_event_id(path, &mut event, "part", &part.id, "reasoning");
            events.push(event);
            *sequence += 1;
        }
        "tool" => {
            project_tool_part(
                path,
                part,
                data,
                timestamp,
                sequence,
                events,
                include_deferred_content,
            );
        }
        "step-start" | "step-finish" | "snapshot" | "patch" | "compaction" => {
            let mut event = semantic_event(
                *sequence,
                EventKind::SystemStatus,
                timestamp,
                None,
                Some(kind.clone()),
                None,
                serde_json::json!({"part_id":part.id,"type":kind}),
            );
            set_native_event_id(path, &mut event, "part", &part.id, "status");
            events.push(event);
            *sequence += 1;
        }
        "file" => {
            let mut event = semantic_event(
                *sequence,
                EventKind::SystemStatus,
                timestamp,
                None,
                Some("file".to_string()),
                optional_text(data, &["filename"]),
                serde_json::json!({"part_id":part.id,"type":"file"}),
            );
            set_native_event_id(path, &mut event, "part", &part.id, "file");
            events.push(event);
            *sequence += 1;
        }
        _ => {
            let mut event = unadapted_event(
                *sequence,
                timestamp,
                &kind,
                serde_json::json!({"table":"part","row_id":part.id,"message_id":part.message_id,"type":kind}),
            );
            set_native_event_id(path, &mut event, "part", &part.id, "unadapted");
            events.push(event);
            *sequence += 1;
        }
    }
}

fn project_tool_part(
    path: &Path,
    part: &PartRow,
    data: &Value,
    timestamp: &str,
    sequence: &mut usize,
    events: &mut Vec<ConversationEvent>,
    include_deferred_content: bool,
) {
    let state = data.get("state").unwrap_or(&Value::Null);
    let call_id = first_text(data, &["callID", "callId"]);
    let name = first_text(data, &["tool", "name"]);
    let mut call = semantic_event(
        *sequence,
        EventKind::ToolCall,
        timestamp,
        Some(EventActor::Assistant),
        (!name.is_empty()).then_some(name.clone()),
        state.get("input").map(Value::to_string),
        serde_json::json!({"part_id":part.id,"call_id":call_id,"name":name,"input":state.get("input")}),
    );
    set_native_event_id(path, &mut call, "part", &part.id, "tool_call");
    events.push(call);
    *sequence += 1;

    let status = state.get("status").and_then(Value::as_str).unwrap_or("");
    if matches!(status, "completed" | "error") {
        let output_text = if status == "error" {
            state.get("error").map(content_text).unwrap_or_default()
        } else {
            tool_output_text(state)
        };
        let details = serde_json::json!({
            "part_id":part.id,
            "call_id":call_id,
            "name":name,
            "output":output_text,
            "status":status
        });
        let mut result =
            tool_result_event(*sequence, timestamp, &details, include_deferred_content);
        set_native_event_id(path, &mut result, "part", &part.id, "tool_result");
        events.push(result);
        *sequence += 1;
    } else if matches!(status, "pending" | "running") {
        let mut state_event = semantic_event(
            *sequence,
            EventKind::SystemStatus,
            timestamp,
            None,
            Some(format!("tool_{status}")),
            None,
            serde_json::json!({"part_id":part.id,"call_id":call_id,"status":status}),
        );
        set_native_event_id(path, &mut state_event, "part", &part.id, "tool_status");
        events.push(state_event);
        *sequence += 1;
    }
}

fn set_native_event_id(
    path: &Path,
    event: &mut ConversationEvent,
    table: &str,
    row_id: &str,
    projection: &str,
) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    table.hash(&mut hasher);
    row_id.hash(&mut hasher);
    projection.hash(&mut hasher);
    event.event_id = format!("opencode:{:016x}", hasher.finish());
}

/// 用户亲手打的字。`synthetic` / `ignored` 是 OpenCode 塞进去的文件注入和命令模板。
fn visible_text(part: &Value) -> Option<&str> {
    if part.get("synthetic").and_then(Value::as_bool) == Some(true)
        || part.get("ignored").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    if part.get("type").and_then(Value::as_str) != Some("text") {
        return None;
    }
    part.get("text")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

/// 工具结果优先用 `state.output`。output 为空时用 `state.content` 里的文本；两边都有且不同则都保留。
fn tool_output_text(state: &Value) -> String {
    let output = state.get("output").map(content_text).unwrap_or_default();
    let content = state.get("content").map(content_text).unwrap_or_default();
    if output.is_empty() {
        content
    } else if content.is_empty() || output == content {
        output
    } else {
        format!("{output}\n{content}")
    }
}

pub(super) fn is_known_part_kind(kind: &str) -> bool {
    matches!(
        kind,
        "text"
            | "reasoning"
            | "tool"
            | "step-start"
            | "step-finish"
            | "snapshot"
            | "patch"
            | "compaction"
            | "file"
    )
}

pub(super) fn safe_kind(kind: &str) -> String {
    if !kind.is_empty()
        && kind.len() <= 64
        && kind
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        kind.to_string()
    } else {
        "unknown".to_string()
    }
}

fn millis_timestamp(value: Option<i64>) -> String {
    value
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|timestamp| timestamp.to_rfc3339())
        .unwrap_or_default()
}
