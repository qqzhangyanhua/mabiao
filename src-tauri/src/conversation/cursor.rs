use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{FixedOffset, NaiveDateTime, TimeZone};
use serde_json::{json, Value};

use super::toolbox::*;
use super::{discover_extension, ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::cursor_session::{
    group_transcripts, is_subagent_transcript, project_from_transcript_path,
    session_dir_from_transcript,
};

#[cfg(test)]
#[path = "cursor_test.rs"]
mod tests;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for group in group_transcripts(discover_extension(roots, "jsonl")?) {
        let Some(parent) = group.parent else {
            continue;
        };
        paths.push(parent);
        paths.extend(group.subagents);
    }
    paths.sort();
    Ok(paths)
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let (conversation, diagnostics) =
        parse(path, false).map_err(|message| ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message,
            event_type: Some("cursor_transcript".to_string()),
            line: None,
        })?;
    Ok(ConversationIndexBatch {
        conversations: vec![conversation],
        diagnostics,
    })
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let (parsed, _) = parse(path, include_deferred_content)?;
    if parsed.session.session_id == session_id {
        Ok(parsed)
    } else {
        Err("Cursor transcript 中的会话 ID 与索引不一致".to_string())
    }
}

pub(crate) fn is_native_transcript(path: &Path) -> bool {
    session_dir_from_transcript(path).is_some()
}

fn parse(
    path: &Path,
    include_deferred_content: bool,
) -> Result<(ParsedConversation, Vec<ConversationIndexIssue>), String> {
    let session_dir = session_dir_from_transcript(path)
        .ok_or_else(|| "Cursor transcript 路径不属于 agent-transcripts 会话目录".to_string())?;
    let parent_session_id = session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Cursor transcript 路径缺少父会话 ID".to_string())?
        .to_string();
    let is_child = is_subagent_transcript(path);
    let session_id = if is_child {
        path.file_stem()
            .and_then(|name| name.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "Cursor 子代理 transcript 路径缺少会话 ID".to_string())?
            .to_string()
    } else {
        parent_session_id.clone()
    };
    let values = parse_jsonl_conversation_values(path)?;
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut diagnostics = Vec::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut sequence = 0usize;

    if is_child {
        let timestamp = values
            .first()
            .map(|(_, value)| timestamp(value))
            .unwrap_or_default();
        events.push(semantic_event(
            sequence,
            EventKind::SystemStatus,
            &timestamp,
            None,
            Some("session_started".to_string()),
            None,
            json!({"parent_session_id": parent_session_id}),
        ));
        sequence += 1;
    }

    for (line, value) in values {
        let occurred_at = timestamp(&value);
        update_time_bounds(&occurred_at, &mut started_at, &mut ended_at);
        let role = value.get("role").and_then(Value::as_str);
        if matches!(role, Some("user" | "assistant")) {
            project_message_record(
                line,
                &value,
                role.unwrap_or_default(),
                &occurred_at,
                &mut sequence,
                &mut messages,
                &mut events,
                include_deferred_content,
            );
            continue;
        }
        if value.get("type").and_then(Value::as_str) == Some("turn_ended") {
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let event = match status {
                "success" | "aborted" => semantic_event(
                    sequence,
                    EventKind::SystemStatus,
                    &occurred_at,
                    None,
                    Some(format!("turn_{status}")),
                    optional_text(&value, &["error", "message", "reason"]),
                    value,
                ),
                "error" => semantic_event(
                    sequence,
                    EventKind::Error,
                    &occurred_at,
                    None,
                    Some("turn_error".to_string()),
                    optional_text(&value, &["error", "message"]),
                    value,
                ),
                _ => unadapted_event(
                    sequence,
                    &occurred_at,
                    "turn_ended",
                    structural_details(line, &value),
                ),
            };
            events.push(event);
            sequence += 1;
            continue;
        }

        let raw_kind = value
            .get("type")
            .and_then(Value::as_str)
            .or(role)
            .unwrap_or("unknown")
            .to_string();
        events.push(unadapted_event(
            sequence,
            &occurred_at,
            &raw_kind,
            structural_details(line, &value),
        ));
        diagnostics.push(ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message: format!("Cursor transcript 第 {} 行事件类型尚未适配", line + 1),
            event_type: Some("cursor_event".to_string()),
            line: Some((line + 1) as u64),
        });
        sequence += 1;
    }

    if started_at.is_empty() && ended_at.is_empty() {
        let mtime = path_mtime_rfc3339(path);
        started_at = mtime.clone();
        ended_at = mtime;
    } else if started_at.is_empty() {
        started_at = ended_at.clone();
    } else if ended_at.is_empty() {
        ended_at = started_at.clone();
    }

    let parsed = finish_source_conversation(
        Source::CursorAgent,
        path,
        session_id,
        String::new(),
        project_from_transcript_path(path),
        String::new(),
        started_at,
        ended_at,
        messages,
        events,
        !is_child,
    )?;
    Ok((parsed, diagnostics))
}

#[allow(clippy::too_many_arguments)]
fn project_message_record(
    line: usize,
    value: &Value,
    role: &str,
    occurred_at: &str,
    sequence: &mut usize,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
    include_deferred_content: bool,
) {
    let content = value.pointer("/message/content");
    let Some(content) = content else {
        return;
    };
    let Some(blocks) = content.as_array() else {
        push_projected_message(
            *sequence,
            occurred_at,
            role,
            content,
            json!({"line": line + 1}),
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
            json!({"line": line + 1}),
            messages,
            events,
        );
        *sequence += 1;
    }

    for block in blocks {
        match block.get("type").and_then(Value::as_str).unwrap_or("") {
            "text" | "" => {}
            "tool_use" => {
                events.push(semantic_event(
                    *sequence,
                    EventKind::ToolCall,
                    occurred_at,
                    Some(EventActor::Assistant),
                    optional_text(block, &["name"]),
                    cursor_tool_call_text(block),
                    normalize_tool_call_details(block),
                ));
                *sequence += 1;
            }
            "tool_result" => {
                let details = cursor_tool_result_details(block);
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

fn cursor_tool_call_text(block: &Value) -> Option<String> {
    optional_text(block, &["command", "text"]).or_else(|| {
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
    })
}

fn cursor_tool_result_details(block: &Value) -> Value {
    let mut details = normalize_tool_result_details(block);
    if let Value::Object(object) = &mut details {
        let has_output = object.contains_key("output") || object.contains_key("result");
        if !has_output {
            let output = object
                .get("content")
                .map(content_text)
                .filter(|output| !output.is_empty());
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
        "status": value.get("status").and_then(Value::as_str),
    })
}

fn timestamp(value: &Value) -> String {
    let direct = first_text(
        value,
        &["timestamp", "created_at", "createdAt", "occurred_at"],
    );
    if !direct.is_empty() {
        return direct;
    }
    let nested = value
        .get("message")
        .map(|message| first_text(message, &["timestamp", "created_at", "createdAt"]))
        .unwrap_or_default();
    if !nested.is_empty() {
        return nested;
    }
    value
        .get("message")
        .map(content_text)
        .map(|text| parse_embedded_timestamp(&text))
        .unwrap_or_default()
}

fn parse_embedded_timestamp(text: &str) -> String {
    let Some(inner) = text
        .split("<timestamp>")
        .nth(1)
        .and_then(|rest| rest.split("</timestamp>").next())
    else {
        return String::new();
    };
    parse_cursor_clock(inner.trim()).unwrap_or_default()
}

fn parse_cursor_clock(raw: &str) -> Option<String> {
    let (datetime, tz) = raw
        .rsplit_once(" (")
        .map(|(datetime, tz)| (datetime.trim(), tz.trim().trim_end_matches(')')))
        .unwrap_or((raw, "UTC"));
    let naive = NaiveDateTime::parse_from_str(datetime, "%A, %B %d, %Y, %I:%M %p")
        .or_else(|_| NaiveDateTime::parse_from_str(datetime, "%A, %B %e, %Y, %I:%M %p"))
        .ok()?;
    let offset = parse_utc_offset_label(tz)?;
    Some(offset.from_local_datetime(&naive).single()?.to_rfc3339())
}

fn parse_utc_offset_label(label: &str) -> Option<FixedOffset> {
    let label = label.trim();
    if label.eq_ignore_ascii_case("utc") || label.eq_ignore_ascii_case("gmt") {
        return FixedOffset::east_opt(0);
    }
    let rest = label
        .strip_prefix("UTC")
        .or_else(|| label.strip_prefix("GMT"))
        .or_else(|| label.strip_prefix("utc"))
        .or_else(|| label.strip_prefix("gmt"))?;
    let rest = rest.trim();
    if rest.is_empty() {
        return FixedOffset::east_opt(0);
    }
    let sign = if rest.starts_with('-') { -1 } else { 1 };
    let digits = rest.trim_start_matches(['+', '-']).replace(':', "");
    let seconds = if digits.len() <= 2 {
        digits.parse::<i32>().ok()? * 3600
    } else if digits.len() == 4 {
        let hours = digits[..2].parse::<i32>().ok()?;
        let minutes = digits[2..].parse::<i32>().ok()?;
        hours * 3600 + minutes * 60
    } else {
        return None;
    };
    FixedOffset::east_opt(sign * seconds)
}

fn path_mtime_rfc3339(path: &Path) -> String {
    let Ok(metadata) = fs::metadata(path) else {
        return String::new();
    };
    let Ok(modified) = metadata.modified() else {
        return String::new();
    };
    let Ok(duration) = modified.duration_since(UNIX_EPOCH) else {
        return String::new();
    };
    chrono::DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())
        .map(|time| time.to_rfc3339())
        .unwrap_or_default()
}
