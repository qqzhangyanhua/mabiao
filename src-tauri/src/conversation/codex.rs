use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use super::toolbox::*;
use super::{single_detail, ConversationIndexBatch, ConversationIndexIssue};

const VSCODE_CONTEXT_PREFIX: &str = "# Context from my IDE setup:";
const CODEX_REQUEST_MARKER: &str = "my request for codex";

#[cfg(test)]
#[path = "codex_test.rs"]
mod tests;

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    parse_file_mode(path, false, false).map(|conversation| ConversationIndexBatch {
        conversations: vec![conversation],
        diagnostics: Vec::new(),
    })
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, parse_file)
}

fn parse_file_mode(
    path: &Path,
    tolerate_incomplete_tail: bool,
    include_deferred_content: bool,
) -> Result<ParsedConversation, ConversationIndexIssue> {
    let content = fs::read_to_string(path).map_err(|error| ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message: format!("读取原始文件失败：{error}"),
        event_type: None,
        line: None,
    })?;
    parse_content(
        path,
        &content,
        0,
        0,
        tolerate_incomplete_tail,
        include_deferred_content,
        None,
    )
}

pub(super) fn index_suffix(
    path: &Path,
    byte_offset: u64,
    start_line: u32,
    session_id: &str,
) -> Result<ParsedConversation, ConversationIndexIssue> {
    let content = read_file_suffix(path, byte_offset)?;
    parse_content(
        path,
        &content,
        byte_offset,
        start_line,
        true,
        false,
        Some(session_id.to_string()),
    )
}

pub(super) fn parse_content(
    path: &Path,
    content: &str,
    start_byte: u64,
    start_line: u32,
    tolerate_incomplete_tail: bool,
    include_deferred_content: bool,
    session_hint: Option<String>,
) -> Result<ParsedConversation, ConversationIndexIssue> {
    let mut session_id = session_hint.clone().unwrap_or_default();
    let mut title = String::new();
    let mut project = String::new();
    let mut model = String::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut response_messages = Vec::new();
    let mut event_messages = Vec::new();
    let mut events = Vec::new();
    let mut pending_delta = None;
    let last_line_index = content.lines().count().saturating_sub(1);
    let has_unterminated_tail = !content.ends_with('\n');
    let mut skipped_incomplete = false;

    for (index, raw) in content.lines().enumerate() {
        let line = start_line as usize + index;
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(raw) {
            Ok(value) => value,
            Err(error)
                if tolerate_incomplete_tail
                    && has_unterminated_tail
                    && index == last_line_index
                    && error.classify() == serde_json::error::Category::Eof =>
            {
                skipped_incomplete = true;
                break;
            }
            Err(error) => {
                return Err(ConversationIndexIssue {
                    path: path.to_string_lossy().to_string(),
                    message: format!("JSON 无效：{error}"),
                    event_type: Some("json_line".to_string()),
                    line: Some((line + 1) as u64),
                });
            }
        };
        let timestamp = text_field(&value, "timestamp");
        if !timestamp.is_empty() {
            if started_at.is_empty() {
                started_at = timestamp.clone();
            }
            ended_at = timestamp.clone();
        }
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let payload = value.get("payload").unwrap_or(&Value::Null);
        if session_id.is_empty() && kind.is_empty() {
            session_id = first_text(&value, &["id"]);
        }
        match kind {
            "session_meta" => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                session_id = first_text(payload, &["id", "session_id"]);
                project = first_text(payload, &["cwd"]);
                title = first_text(payload, &["title", "name"]);
                events.push(semantic_event(
                    line,
                    EventKind::SystemStatus,
                    &timestamp,
                    None,
                    Some("session_started".to_string()),
                    None,
                    payload.clone(),
                ));
            }
            "turn_context" => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                let next_project = first_text(payload, &["cwd"]);
                if !next_project.is_empty() {
                    project = next_project;
                }
                let next_model = first_text(payload, &["model"]);
                if !next_model.is_empty() && next_model != model {
                    events.push(semantic_event(
                        line,
                        EventKind::ModelChange,
                        &timestamp,
                        None,
                        Some(next_model.clone()),
                        None,
                        payload.clone(),
                    ));
                    model = next_model;
                }
            }
            "response_item" => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                project_codex_response_payload(
                    line,
                    &timestamp,
                    payload,
                    include_deferred_content,
                    &mut response_messages,
                    &mut events,
                );
            }
            "message"
            | "function_call"
            | "function_call_output"
            | "reasoning"
            | "custom_tool_call"
            | "custom_tool_call_output"
            | "web_search_call"
            | "local_shell_call" => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                project_codex_response_payload(
                    line,
                    &timestamp,
                    &value,
                    include_deferred_content,
                    &mut response_messages,
                    &mut events,
                );
            }
            "compacted" => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                events.push(semantic_event(
                    line,
                    EventKind::SystemStatus,
                    &timestamp,
                    None,
                    Some("compacted".to_string()),
                    None,
                    payload.clone(),
                ));
            }
            "world_state" | "inter_agent_communication_metadata" => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                events.push(event_msg_semantic_event(line, &timestamp, kind, payload));
            }
            "event_msg" => {
                let event_kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
                match event_kind {
                    "agent_message_delta" => append_message_delta(
                        &mut pending_delta,
                        line,
                        &timestamp,
                        "assistant",
                        payload,
                    ),
                    "token_count" | "heartbeat" | "item_completed" => {}
                    "exec_command_end" | "patch_apply_end" | "mcp_tool_call_end"
                    | "web_search_end" => {
                        flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                        if let Some(event) =
                            failed_tool_end_event(line, &timestamp, event_kind, payload)
                        {
                            events.push(event);
                        }
                    }
                    _ => {
                        flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                        if let Some(message) = event_message(payload, &timestamp) {
                            project_codex_message(
                                line,
                                message,
                                payload.clone(),
                                &mut event_messages,
                                &mut events,
                            );
                        } else {
                            events.push(event_msg_semantic_event(
                                line, &timestamp, event_kind, payload,
                            ));
                        }
                    }
                }
            }
            "" => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
            }
            _ => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                events.push(unadapted_event(line, &timestamp, kind, value.clone()));
            }
        }
    }
    flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
    let messages = if response_messages.is_empty() {
        event_messages
    } else {
        response_messages
    };
    let (consumed_bytes, consumed_lines) = if skipped_incomplete {
        match content.rfind('\n') {
            Some(pos) => (
                (pos + 1) as i64,
                i64::from(next_line_index(&content[..=pos])),
            ),
            None => (0, 0),
        }
    } else {
        (content.len() as i64, i64::from(next_line_index(content)))
    };
    let mut parsed = finish_source_conversation(
        Source::Codex,
        path,
        session_id,
        title,
        project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        true,
    )
    .map_err(|message| ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: Some("session_meta".to_string()),
        line: None,
    })?;
    parsed.index_cursor = Some(FileIndexCursor {
        byte_offset: start_byte as i64 + consumed_bytes,
        line: i64::from(start_line) + consumed_lines,
    });
    Ok(parsed)
}

fn parse_file(path: &Path, include_deferred_content: bool) -> Result<ParsedConversation, String> {
    parse_file_mode(path, true, include_deferred_content).map_err(|issue| issue.message)
}

fn read_file_suffix(path: &Path, byte_offset: u64) -> Result<String, ConversationIndexIssue> {
    let mut file = fs::File::open(path).map_err(|error| ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message: format!("读取原始文件失败：{error}"),
        event_type: None,
        line: None,
    })?;
    file.seek(SeekFrom::Start(byte_offset))
        .map_err(|error| ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message: format!("读取原始文件失败：{error}"),
            event_type: None,
            line: None,
        })?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|error| ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message: format!("读取原始文件失败：{error}"),
            event_type: None,
            line: None,
        })?;
    Ok(content)
}

fn next_line_index(content: &str) -> u32 {
    let newlines = content.bytes().filter(|&byte| byte == b'\n').count() as u32;
    if content.is_empty() || content.ends_with('\n') {
        newlines
    } else {
        newlines + 1
    }
}

fn project_codex_response_payload(
    line: usize,
    timestamp: &str,
    payload: &Value,
    include_deferred_content: bool,
    response_messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
) {
    if let Some(message) = response_message(payload, timestamp) {
        project_codex_message(line, message, payload.clone(), response_messages, events);
    } else if let Some(event) =
        response_semantic_event(line, timestamp, payload, include_deferred_content)
    {
        events.push(event);
    }
}

enum CodexUserInjection {
    EnvironmentContext,
    AgentsMd,
    IdeContext { prompt: Option<String> },
}

fn project_codex_message(
    line: usize,
    message: ConversationMessage,
    details: Value,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
) {
    if message.role == "user" {
        if let Some(injection) = classify_codex_user_injection(&message.text) {
            let name = match &injection {
                CodexUserInjection::EnvironmentContext => "environment_context",
                CodexUserInjection::AgentsMd => "agents_md",
                CodexUserInjection::IdeContext { .. } => "ide_context",
            };
            let mut status_details = details.clone();
            if let Value::Object(object) = &mut status_details {
                object.remove("content");
                object.remove("message");
            }
            events.push(semantic_event(
                line,
                EventKind::SystemStatus,
                &message.occurred_at,
                None,
                Some(name.to_string()),
                Some(message.text.clone()),
                status_details,
            ));
            if let CodexUserInjection::IdeContext {
                prompt: Some(prompt),
            } = injection
            {
                let extracted = ConversationMessage {
                    role: message.role,
                    occurred_at: message.occurred_at,
                    text: prompt,
                };
                events.push(message_event(line, &extracted, details));
                messages.push(extracted);
            }
            return;
        }
    }
    events.push(message_event(line, &message, details));
    messages.push(message);
}

fn classify_codex_user_injection(text: &str) -> Option<CodexUserInjection> {
    let trimmed = text.trim();
    if trimmed.starts_with("<environment_context>") {
        Some(CodexUserInjection::EnvironmentContext)
    } else if trimmed.starts_with("# AGENTS.md") {
        Some(CodexUserInjection::AgentsMd)
    } else if trimmed.starts_with(VSCODE_CONTEXT_PREFIX) {
        Some(CodexUserInjection::IdeContext {
            prompt: extract_codex_prompt_from_ide_context(trimmed),
        })
    } else {
        None
    }
}

fn extract_codex_prompt_from_ide_context(text: &str) -> Option<String> {
    let normalized = text.replace("\r\n", "\n");
    let lines = normalized.lines().collect::<Vec<_>>();
    let mut prompt = None;
    for (index, line) in lines.iter().enumerate() {
        let Some(inline_prompt) = codex_request_heading_payload(line) else {
            continue;
        };
        if !inline_prompt.is_empty() {
            prompt = Some(inline_prompt.to_string());
            continue;
        }
        let following = lines[index + 1..].join("\n").trim().to_string();
        prompt = (!following.is_empty()).then_some(following);
    }
    prompt
}

fn codex_request_heading_payload(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with('#') {
        return None;
    }
    let heading = trimmed.trim_start_matches('#').trim_start();
    let lowered = heading.to_ascii_lowercase();
    if !lowered.starts_with(CODEX_REQUEST_MARKER) {
        return None;
    }
    let suffix = heading.get(CODEX_REQUEST_MARKER.len()..)?.trim_start();
    let Some(separator) = suffix.chars().next() else {
        return Some("");
    };
    if !matches!(separator, ':' | '：' | '-' | '—') {
        return None;
    }
    Some(
        suffix
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '：' | '-' | '—'))
            .trim(),
    )
}
