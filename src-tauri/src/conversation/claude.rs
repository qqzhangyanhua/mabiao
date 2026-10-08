use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use super::toolbox::*;
use super::ConversationIndexIssue;

#[cfg(test)]
#[path = "claude_test.rs"]
mod tests;

pub(super) fn parse(
    path: &Path,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let values = parse_jsonl_conversation_values(path)?;
    let line = values.last().map(|(l, _)| *l as i64 + 1).unwrap_or(0);
    let mut parsed = parse_from_values(path, values, include_deferred_content, None, false, false)?;
    // 全量解析也设游标，这样首次索引后 indexed_byte_offset 就是文件大小，
    // 下一次文件增长时 plan_conversation_file_index 才能走增量路径。
    let byte_offset = fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
    parsed.index_cursor = Some(FileIndexCursor { byte_offset, line });
    Ok(parsed)
}

/// 后缀增量解析：只读字节偏移之后的新内容，只提取事件，不重新推导 title/started_at。
/// 会话 ID 由调用方注入（ADR 0011 的代次语义 + ADR 0024 的增量写入）。
pub(super) fn index_suffix(
    path: &Path,
    byte_offset: u64,
    start_line: u32,
    session_id: &str,
) -> Result<ParsedConversation, ConversationIndexIssue> {
    let content = read_file_suffix(path, byte_offset)?;
    let parsed_values =
        parse_suffix_values(&content, start_line as usize, true).map_err(|message| {
            ConversationIndexIssue {
                path: path.to_string_lossy().to_string(),
                message,
                event_type: None,
                line: None,
            }
        })?;
    let mut parsed = parse_from_values(
        path,
        parsed_values.values,
        false,
        Some(session_id),
        true, // line_direct: 跳过 session_started 事件（后缀里没有 started_at）
        true, // suffix_mode: 跳过首条 ModelChange（后缀里第一个 model 是当前模型）
    )
    .map_err(|message| ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: None,
        line: None,
    })?;
    parsed.index_cursor = Some(FileIndexCursor {
        byte_offset: byte_offset as i64 + parsed_values.consumed_bytes,
        line: start_line as i64 + parsed_values.consumed_lines,
    });
    Ok(parsed)
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

struct SuffixParseResult {
    values: Vec<(usize, Value)>,
    consumed_bytes: i64,
    consumed_lines: i64,
}

/// 解析后缀 JSONL，容忍最后一行不完整（写了一半、没有 `\n`）。
/// consumed 是完整行的字节数/行数，不完整尾行不计入，游标回退到最后一个 `\n`。
fn parse_suffix_values(
    content: &str,
    start_line: usize,
    tolerate_incomplete_tail: bool,
) -> Result<SuffixParseResult, String> {
    let lines: Vec<&str> = content.lines().collect();
    let last_index = lines.len().saturating_sub(1);
    let has_unterminated_tail = !content.ends_with('\n');
    let mut values = Vec::new();
    let mut skipped_incomplete = false;
    for (index, raw) in lines.iter().enumerate() {
        let line_num = start_line + index;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => values.push((line_num, value)),
            Err(error) => {
                if tolerate_incomplete_tail
                    && has_unterminated_tail
                    && index == last_index
                    && error.classify() == serde_json::error::Category::Eof
                {
                    skipped_incomplete = true;
                    break;
                }
                return Err(format!("第 {} 行 JSON 无效：{error}", line_num + 1));
            }
        }
    }
    let (consumed_bytes, consumed_lines) = if skipped_incomplete {
        match content.rfind('\n') {
            Some(pos) => (
                (pos + 1) as i64,
                content[..=pos].bytes().filter(|&b| b == b'\n').count() as i64,
            ),
            None => (0, 0),
        }
    } else {
        (content.len() as i64, lines.len() as i64)
    };
    Ok(SuffixParseResult {
        values,
        consumed_bytes,
        consumed_lines,
    })
}

pub(super) fn parse_from_values(
    path: &Path,
    values: Vec<(usize, Value)>,
    include_deferred_content: bool,
    session_hint: Option<&str>,
    line_direct: bool,
    suffix_mode: bool,
) -> Result<ParsedConversation, String> {
    let mut parent_session_id = String::new();
    let mut agent_id = String::new();
    let mut project = String::new();
    let mut model = String::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut custom_title = String::new();
    let mut first_prompt = String::new();
    let path_is_subagent = path
        .components()
        .any(|part| part.as_os_str() == "subagents");

    for (index, value) in &values {
        let timestamp = text_field(value, "timestamp");
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        if parent_session_id.is_empty() {
            parent_session_id = first_text(value, &["sessionId", "session_id"]);
        }
        if agent_id.is_empty() {
            agent_id = first_text(value, &["agentId", "agent_id"]);
        }
        if project.is_empty() {
            project = first_text(value, &["cwd"]);
        }
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let message = value.get("message").unwrap_or(&Value::Null);
        let role = first_text(message, &["role"]);
        let next_model = first_text(message, &["model"]);
        if !next_model.is_empty() && next_model != model {
            // 后缀模式下第一个 model 是会话当前在用的模型，不是"变更"——不发射
            // ModelChange 事件，只更新 model 字段（ADR 0011 代次语义）。
            let is_suffix_initial_model = suffix_mode && model.is_empty();
            model = next_model.clone();
            if !is_suffix_initial_model {
                events.push(semantic_event(
                    *index,
                    EventKind::ModelChange,
                    &timestamp,
                    None,
                    Some(next_model),
                    None,
                    message.clone(),
                ));
            }
        }
        let content = message.get("content").unwrap_or(&Value::Null);
        if matches!(role.as_str(), "user" | "assistant") {
            let text = content_text(content);
            if !text.is_empty() {
                if let Some(name) = claude_residue_status_name(value, &role, &text) {
                    events.push(semantic_event(
                        *index,
                        EventKind::SystemStatus,
                        &timestamp,
                        None,
                        Some(name.to_string()),
                        Some(text),
                        value.clone(),
                    ));
                } else {
                    if role == "user" && first_prompt.is_empty() && !is_claude_slash_command(&text)
                    {
                        first_prompt = text;
                    }
                    push_projected_message(
                        *index,
                        &timestamp,
                        &role,
                        content,
                        message.clone(),
                        &mut messages,
                        &mut events,
                    );
                }
            }
        }
        if let Some(items) = content.as_array() {
            for item in items {
                match item.get("type").and_then(Value::as_str).unwrap_or("") {
                    "tool_use" => events.push(semantic_event(
                        *index,
                        EventKind::ToolCall,
                        &timestamp,
                        Some(EventActor::Assistant),
                        optional_text(item, &["name"]),
                        item.get("input").map(Value::to_string),
                        normalize_tool_call_details(item),
                    )),
                    "tool_result" => events.push(tool_result_event(
                        *index,
                        &timestamp,
                        &normalize_tool_result_details(item),
                        include_deferred_content,
                    )),
                    "thinking" | "redacted_thinking" => events.push(semantic_event(
                        *index,
                        EventKind::Plan,
                        &timestamp,
                        Some(EventActor::Assistant),
                        (item.get("type").and_then(Value::as_str) == Some("redacted_thinking"))
                            .then(|| "redacted_thinking".to_string()),
                        optional_text(item, &["thinking", "text"]),
                        item.clone(),
                    )),
                    _ => {}
                }
            }
        }
        if !matches!(kind, "user" | "assistant") {
            let named_title = claude_named_title(kind, value);
            if let Some(title) = &named_title {
                custom_title = title.clone();
            }
            let event = if matches!(
                kind,
                "system"
                    | "progress"
                    | "result"
                    | "queue-operation"
                    | "attachment"
                    | "mode"
                    | "last-prompt"
                    | "permission-mode"
                    | "file-history-snapshot"
                    | "file-history-delta"
                    | "ai-title"
                    | "custom-title"
                    | "frame-link"
            ) {
                semantic_event(
                    *index,
                    if value.get("is_error").and_then(Value::as_bool) == Some(true) {
                        EventKind::Error
                    } else {
                        EventKind::SystemStatus
                    },
                    &timestamp,
                    None,
                    Some(kind.to_string()),
                    named_title.or_else(|| optional_text(value, &["result", "content", "message"])),
                    value.clone(),
                )
            } else {
                event_msg_semantic_event(*index, &timestamp, kind, value)
            };
            events.push(event);
        }
    }

    let is_top_level = !path_is_subagent && agent_id.is_empty();
    // 后缀模式（session_hint 非空）直接用注入的 session_id，不从数据推导——
    // 后缀里第一行的 sessionId 和首条 prompt 都不代表整段会话。
    let mut session_id = if session_hint.is_some() {
        session_hint
            .map(str::to_string)
            .filter(|v| !v.is_empty())
            .unwrap_or_default()
    } else if is_top_level {
        parent_session_id.clone()
    } else if !agent_id.is_empty() {
        agent_id
    } else {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("")
            .trim_start_matches("agent-")
            .to_string()
    };
    if session_id.is_empty() {
        session_id = session_hint
            .map(str::to_string)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("")
                    .to_string()
            });
    }
    if !is_top_level && !line_direct {
        let mut details = serde_json::Map::new();
        details.insert("parent_id".to_string(), Value::String(parent_session_id));
        events.push(semantic_event(
            0,
            EventKind::SystemStatus,
            &started_at,
            None,
            Some("session_started".to_string()),
            None,
            Value::Object(details),
        ));
    }
    let title = if !custom_title.is_empty() {
        truncate_title(&custom_title)
    } else {
        let peeled = truncate_title(&strip_prompt_wrappers(&first_prompt));
        if peeled.is_empty() {
            session_id.clone()
        } else {
            peeled
        }
    };
    finish_source_conversation(
        Source::Claude,
        path,
        session_id,
        title,
        project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        is_top_level,
        ConversationFinishPrep::NONE,
    )
}

fn claude_residue_status_name(value: &Value, role: &str, text: &str) -> Option<&'static str> {
    if value.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return Some("meta");
    }
    if role == "user" && text.trim().starts_with("<local-command-caveat>") {
        return Some("caveat");
    }
    None
}

fn is_claude_slash_command(text: &str) -> bool {
    text.trim().starts_with("<command-name>")
}

fn claude_named_title(kind: &str, value: &Value) -> Option<String> {
    match kind {
        "custom-title" => optional_text(value, &["customTitle"]),
        "ai-title" => optional_text(value, &["aiTitle"]),
        _ => None,
    }
}
