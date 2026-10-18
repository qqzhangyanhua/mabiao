//! Cherry Studio 对话记录：`cherrystudio.sqlite` 的 `topic` + `message`。
//!
//! `message.data` 是 AI SDK UIMessage JSON（`parts`）。`role = root` 不投影。
//! `deleted_at` 非空的行跳过。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde_json::Value;

use super::toolbox::*;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::cherry::millis_to_rfc3339;
use crate::adapters::goose::table_columns;
use crate::domain::Source;
use crate::ingest;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::cherry::discover(roots)
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
        .ok_or_else(|| "Cherry Studio 数据库中未找到该会话".to_string())
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    super::discover::wal_source_revision(path, "Cherry Studio")
}

fn fatal(path: &Path, message: String) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: Some("cherry_schema".to_string()),
        line: None,
    }
}

struct TopicRow {
    id: String,
    name: String,
    last_activity_at: i64,
    created_at: i64,
}

struct MessageRow {
    role: String,
    data: String,
    searchable_text: String,
    created_at: i64,
}

fn project(
    path: &Path,
    session_filter: Option<&str>,
    include_deferred_content: bool,
) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let db = ingest::open_readonly_uri(path)
        .map_err(|error| fatal(path, format!("只读打开 Cherry Studio 数据库失败：{error}")))?;
    let topics = table_columns(&db, "topic").map_err(|message| fatal(path, message))?;
    if !topics.contains("id") {
        return Ok(ConversationIndexBatch {
            conversations: Vec::new(),
            diagnostics: Vec::new(),
        });
    }
    let messages = table_columns(&db, "message").map_err(|message| fatal(path, message))?;
    if !messages.contains("topic_id") || !messages.contains("role") {
        return Ok(ConversationIndexBatch {
            conversations: Vec::new(),
            diagnostics: Vec::new(),
        });
    }
    let conversations = read_topics(&db, &topics, session_filter)
        .map_err(|message| fatal(path, message))?
        .into_iter()
        .map(|topic| project_topic(path, &db, &messages, topic, include_deferred_content))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|message| fatal(path, message))?;
    Ok(ConversationIndexBatch {
        conversations,
        diagnostics: Vec::new(),
    })
}

fn read_topics(
    db: &Connection,
    columns: &std::collections::BTreeSet<String>,
    session_filter: Option<&str>,
) -> Result<Vec<TopicRow>, String> {
    let deleted = if columns.contains("deleted_at") {
        "AND deleted_at IS NULL"
    } else {
        ""
    };
    let sql = format!(
        "SELECT {id}, {name}, {last_activity}, {created_at}
         FROM topic
         WHERE (?1 IS NULL OR id = ?1) {deleted}
         ORDER BY id",
        id = text_column(columns, "id"),
        name = text_column(columns, "name"),
        last_activity = int_column(columns, "last_activity_at"),
        created_at = int_column(columns, "created_at"),
        deleted = deleted,
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![session_filter], |row| {
            Ok(TopicRow {
                id: row.get(0)?,
                name: row.get(1)?,
                last_activity_at: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

fn read_messages(
    db: &Connection,
    columns: &std::collections::BTreeSet<String>,
    topic_id: &str,
) -> Result<Vec<MessageRow>, String> {
    let deleted = if columns.contains("deleted_at") {
        "AND deleted_at IS NULL"
    } else {
        ""
    };
    let sql = format!(
        "SELECT role, {data}, {searchable}, {created_at}
         FROM message
         WHERE topic_id = ?1 {deleted}
         ORDER BY created_at, id",
        data = text_column(columns, "data"),
        searchable = text_column(columns, "searchable_text"),
        created_at = int_column(columns, "created_at"),
        deleted = deleted,
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([topic_id], |row| {
            Ok(MessageRow {
                role: row.get(0)?,
                data: row.get(1)?,
                searchable_text: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

fn project_topic(
    path: &Path,
    db: &Connection,
    message_columns: &std::collections::BTreeSet<String>,
    topic: TopicRow,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let rows = read_messages(db, message_columns, &topic.id)?;
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut started_at = millis_to_rfc3339(topic.created_at);
    let mut ended_at = millis_to_rfc3339(topic.last_activity_at);
    let mut model = String::new();
    for (index, row) in rows.into_iter().enumerate() {
        if row.role == "root" {
            continue;
        }
        let timestamp = millis_to_rfc3339(row.created_at);
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        project_content(
            &MessageProjection {
                index,
                timestamp: &timestamp,
                role: &row.role,
                data_json: &row.data,
                searchable_text: &row.searchable_text,
            },
            include_deferred_content,
            &mut messages,
            &mut events,
            &mut model,
        );
    }
    finish_source_conversation(
        Source::Cherry,
        path,
        topic.id,
        topic.name,
        String::new(),
        model,
        started_at,
        ended_at,
        messages,
        events,
        true,
        ConversationFinishPrep::NONE,
    )
}

struct MessageProjection<'a> {
    index: usize,
    timestamp: &'a str,
    role: &'a str,
    data_json: &'a str,
    searchable_text: &'a str,
}

fn project_content(
    message: &MessageProjection<'_>,
    include_deferred_content: bool,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
    model: &mut String,
) {
    let payload: Value = serde_json::from_str(message.data_json).unwrap_or(Value::Null);
    if model.is_empty() {
        if let Some(name) = optional_text(&payload, &["modelId", "model", "model_id"]) {
            *model = name;
        }
    }
    let parts = payload
        .get("parts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut texts = Vec::new();
    for part in &parts {
        let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "text" => {
                if let Some(text) = optional_text(part, &["text"]) {
                    texts.push(text);
                }
            }
            "reasoning" | "thinking" => {
                if let Some(text) = optional_text(part, &["text"]) {
                    events.push(semantic_event(
                        message.index,
                        EventKind::Plan,
                        message.timestamp,
                        Some(EventActor::Assistant),
                        None,
                        Some(text.clone()),
                        serde_json::json!({ "type": kind, "text": text }),
                    ));
                }
            }
            "tool-call" | "tool_call" | "dynamic-tool" => {
                let name = optional_text(part, &["toolName", "name"]);
                let arguments =
                    part.get("input")
                        .or_else(|| part.get("args"))
                        .map(|value| match value {
                            Value::String(text) => text.clone(),
                            other => other.to_string(),
                        });
                events.push(semantic_event(
                    message.index,
                    EventKind::ToolCall,
                    message.timestamp,
                    Some(EventActor::Assistant),
                    name,
                    arguments,
                    normalize_tool_call_details(part),
                ));
            }
            "tool-result" | "tool_result" => {
                let output = optional_text(part, &["output", "result", "text"]).unwrap_or_default();
                let mut payload = serde_json::json!({
                    "call_id": part.get("toolCallId").or_else(|| part.get("tool_call_id")).cloned().unwrap_or(Value::Null),
                    "output": output,
                });
                if part
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    payload["isError"] = Value::Bool(true);
                }
                events.push(tool_result_event(
                    message.index,
                    message.timestamp,
                    &payload,
                    include_deferred_content,
                ));
            }
            _ => {}
        }
    }
    if texts.is_empty() {
        let fallback = message.searchable_text.trim();
        if !fallback.is_empty() {
            texts.push(fallback.to_string());
        }
    }
    if !texts.is_empty() {
        let projected_role = if message.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        push_projected_message(
            message.index,
            message.timestamp,
            projected_role,
            &Value::String(texts.join("\n")),
            serde_json::json!({ "role": message.role }),
            messages,
            events,
        );
    }
}

fn text_column(columns: &std::collections::BTreeSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("COALESCE({name}, '')")
    } else {
        "''".to_string()
    }
}

fn int_column(columns: &std::collections::BTreeSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("COALESCE({name}, 0)")
    } else {
        "0".to_string()
    }
}
