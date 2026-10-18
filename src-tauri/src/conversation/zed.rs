//! Zed 对话记录：`threads.db` 里每行一场线程，`data` 为 zstd / json。
//!
//! 消息是 serde 外部标签：`User` / `Agent` / `Resume` / `Compaction`。
//! `parent_id` 非空的子线程不算顶层。

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::toolbox::*;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::goose::parse_sql_timestamp;
use crate::adapters::zed::{
    load_thread, load_threads, model_from_thread, project_from_thread, LoadedThread,
};
use crate::domain::Source;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::zed::discover(roots)
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    project(path, None, false)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    project(path, Some(session_id), include_deferred_content)
        .map_err(|issue| issue.message)?
        .conversations
        .into_iter()
        .next()
        .ok_or_else(|| "Zed 数据库中未找到该会话".to_string())
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    super::discover::wal_source_revision(path, "Zed")
}

fn fatal(path: &Path, message: String) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: Some("zed_schema".to_string()),
        line: None,
    }
}

fn project(
    path: &Path,
    session_filter: Option<&str>,
    include_deferred_content: bool,
) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let threads = match session_filter {
        Some(session_id) => load_thread(path, session_id)
            .map_err(|message| fatal(path, message))?
            .into_iter()
            .collect(),
        None => load_threads(path).map_err(|message| fatal(path, message))?,
    };
    let conversations = threads
        .into_iter()
        .map(|thread| project_thread(path, thread, include_deferred_content))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|message| fatal(path, message))?;
    Ok(ConversationIndexBatch {
        conversations,
        diagnostics: Vec::new(),
    })
}

fn project_thread(
    path: &Path,
    thread: LoadedThread,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let timestamp = parse_sql_timestamp(if thread.updated_at.trim().is_empty() {
        &thread.created_at
    } else {
        &thread.updated_at
    });
    let mut started_at = parse_sql_timestamp(&thread.created_at);
    let mut ended_at = timestamp.clone();
    if started_at.is_empty() {
        started_at = ended_at.clone();
    }
    update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
    let model = model_from_thread(&thread.json);
    let project = project_from_thread(&thread.json);
    let title = if thread.summary.trim().is_empty() {
        thread
            .json
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    } else {
        thread.summary.clone()
    };
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let raw_messages = thread
        .json
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (index, message) in raw_messages.iter().enumerate() {
        project_message(
            index,
            &timestamp,
            message,
            include_deferred_content,
            &mut messages,
            &mut events,
        );
    }
    finish_source_conversation(
        Source::Zed,
        path,
        thread.id,
        title,
        project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        thread.parent_id.trim().is_empty(),
        ConversationFinishPrep::NONE,
    )
}

fn project_message(
    index: usize,
    timestamp: &str,
    message: &Value,
    include_deferred_content: bool,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
) {
    if let Some(user) = message.get("User") {
        project_user(index, timestamp, user, messages, events);
        return;
    }
    if let Some(agent) = message.get("Agent") {
        project_agent(
            index,
            timestamp,
            agent,
            include_deferred_content,
            messages,
            events,
        );
    }
}

fn project_user(
    index: usize,
    timestamp: &str,
    user: &Value,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
) {
    let texts = content_texts(user.get("content"));
    if texts.is_empty() {
        return;
    }
    push_projected_message(
        index,
        timestamp,
        "user",
        &Value::String(texts.join("\n")),
        serde_json::json!({ "role": "user", "id": user.get("id") }),
        messages,
        events,
    );
}

fn project_agent(
    index: usize,
    timestamp: &str,
    agent: &Value,
    include_deferred_content: bool,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
) {
    let mut texts = Vec::new();
    if let Some(blocks) = agent.get("content").and_then(Value::as_array) {
        for block in blocks {
            if let Some(text) = block.get("Text").and_then(Value::as_str) {
                if !text.trim().is_empty() {
                    texts.push(text.to_string());
                }
                continue;
            }
            if let Some(thinking) = block.get("Thinking") {
                if let Some(text) = optional_text(thinking, &["text"]) {
                    events.push(semantic_event(
                        index,
                        EventKind::Plan,
                        timestamp,
                        Some(EventActor::Assistant),
                        None,
                        Some(text.clone()),
                        serde_json::json!({ "type": "thinking", "text": text }),
                    ));
                }
                continue;
            }
            if let Some(tool) = block.get("ToolUse") {
                let name = optional_text(tool, &["name"]);
                let call_id = optional_text(tool, &["id", "tool_use_id"]);
                let arguments = tool.get("input").map(|value| match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                });
                let mut details = normalize_tool_call_details(tool);
                if let Some(call_id) = &call_id {
                    details["call_id"] = Value::String(call_id.clone());
                }
                events.push(semantic_event(
                    index,
                    EventKind::ToolCall,
                    timestamp,
                    Some(EventActor::Assistant),
                    name,
                    arguments,
                    details,
                ));
            }
        }
    }
    if let Some(results) = agent.get("tool_results") {
        project_tool_results(index, timestamp, results, include_deferred_content, events);
    }
    if !texts.is_empty() {
        push_projected_message(
            index,
            timestamp,
            "assistant",
            &Value::String(texts.join("\n")),
            serde_json::json!({ "role": "assistant" }),
            messages,
            events,
        );
    }
}

fn project_tool_results(
    index: usize,
    timestamp: &str,
    results: &Value,
    include_deferred_content: bool,
    events: &mut Vec<ConversationEvent>,
) {
    let items: Vec<(Option<String>, &Value)> = match results {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| (Some(key.clone()), value))
            .collect(),
        Value::Array(items) => items.iter().map(|value| (None, value)).collect(),
        _ => return,
    };
    for (key, result) in items {
        let call_id = optional_text(result, &["tool_use_id", "id", "call_id"]).or(key);
        let output = tool_result_output(result);
        let failed = result
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut payload = serde_json::json!({
            "call_id": call_id,
            "output": output,
        });
        if failed {
            payload["isError"] = Value::Bool(true);
        }
        events.push(tool_result_event(
            index,
            timestamp,
            &payload,
            include_deferred_content,
        ));
    }
}

fn content_texts(content: Option<&Value>) -> Vec<String> {
    let Some(content) = content else {
        return Vec::new();
    };
    match content {
        Value::String(text) => {
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![text.clone()]
            }
        }
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.get("Text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .filter(|text| !text.trim().is_empty())
                    .or_else(|| {
                        optional_text(item, &["text", "content"])
                            .filter(|text| !text.trim().is_empty())
                    })
            })
            .collect(),
        other => {
            let text = content_text(other);
            if text.is_empty() {
                Vec::new()
            } else {
                vec![text]
            }
        }
    }
}

fn tool_result_output(result: &Value) -> String {
    if let Some(text) = result.as_str() {
        return text.to_string();
    }
    if let Some(text) = optional_text(result, &["content", "output", "result"]) {
        return text;
    }
    if let Some(content) = result.get("content") {
        let text = content_text(content);
        if !text.is_empty() {
            return text;
        }
    }
    String::new()
}
