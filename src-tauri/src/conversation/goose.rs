//! Goose 对话记录：`sessions.db` 的 `sessions` + `messages`。
//!
//! `content_json` 是 typed-block 数组：`text` / `toolRequest` / `toolResponse`。
//! `session_type = hidden` 不进目录；`subagent` 不算顶层。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde_json::Value;

use super::toolbox::*;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::goose::{
    is_catalog_session, parse_model_name, parse_sql_timestamp, table_columns,
    unix_seconds_to_rfc3339,
};
use crate::domain::Source;
use crate::ingest;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::goose::discover(roots)
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
        .ok_or_else(|| "Goose 数据库中未找到该会话".to_string())
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    super::discover::wal_source_revision(path, "Goose")
}

fn fatal(path: &Path, message: String) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: Some("goose_schema".to_string()),
        line: None,
    }
}

struct SessionRow {
    id: String,
    title: String,
    project: String,
    model: String,
    session_type: String,
    started_at: String,
    ended_at: String,
}

struct MessageRow {
    role: String,
    content_json: String,
    created_timestamp: i64,
}

fn project(
    path: &Path,
    session_filter: Option<&str>,
    include_deferred_content: bool,
) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let db = ingest::open_readonly_uri(path)
        .map_err(|error| fatal(path, format!("只读打开 Goose 数据库失败：{error}")))?;
    let sessions = table_columns(&db, "sessions").map_err(|message| fatal(path, message))?;
    if !sessions.contains("id") {
        return Err(fatal(path, "Goose sessions 表缺少 id".to_string()));
    }
    let messages = table_columns(&db, "messages").map_err(|message| fatal(path, message))?;
    if !messages.contains("session_id") || !messages.contains("role") {
        return Err(fatal(
            path,
            "Goose messages 表缺少 session_id 或 role".to_string(),
        ));
    }
    let conversations = read_sessions(&db, &sessions, session_filter)
        .map_err(|message| fatal(path, message))?
        .into_iter()
        .filter(|session| is_catalog_session(&session.session_type))
        .map(|session| project_session(path, &db, &messages, session, include_deferred_content))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|message| fatal(path, message))?;
    Ok(ConversationIndexBatch {
        conversations,
        diagnostics: Vec::new(),
    })
}

fn read_sessions(
    db: &Connection,
    columns: &BTreeSet<String>,
    session_filter: Option<&str>,
) -> Result<Vec<SessionRow>, String> {
    let sql = format!(
        "SELECT {id}, {name}, {working_dir}, {created_at}, {updated_at}, {model_config}, {session_type}
         FROM sessions
         WHERE (?1 IS NULL OR id = ?1)
         ORDER BY id",
        id = text_column(columns, "id"),
        name = text_column(columns, "name"),
        working_dir = text_column(columns, "working_dir"),
        created_at = text_column(columns, "created_at"),
        updated_at = text_column(columns, "updated_at"),
        model_config = text_column(columns, "model_config_json"),
        session_type = text_column(columns, "session_type"),
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![session_filter], |row| {
            let created: String = row.get(3)?;
            let updated: String = row.get(4)?;
            let model_config: String = row.get(5)?;
            Ok(SessionRow {
                id: row.get(0)?,
                title: row.get(1)?,
                project: row.get(2)?,
                model: parse_model_name(&model_config).unwrap_or_default(),
                session_type: row.get(6)?,
                started_at: parse_sql_timestamp(&created),
                ended_at: parse_sql_timestamp(if updated.trim().is_empty() {
                    &created
                } else {
                    &updated
                }),
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

fn read_messages(
    db: &Connection,
    columns: &BTreeSet<String>,
    session_id: &str,
) -> Result<Vec<MessageRow>, String> {
    let content = if columns.contains("content_json") {
        "COALESCE(content_json, '')"
    } else {
        "''"
    };
    let timestamp = if columns.contains("created_timestamp") {
        "COALESCE(created_timestamp, 0)"
    } else {
        "0"
    };
    let order = if columns.contains("created_timestamp") {
        "created_timestamp, id"
    } else {
        "id"
    };
    let sql = format!(
        "SELECT role, {content}, {timestamp} FROM messages WHERE session_id = ?1 ORDER BY {order}"
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([session_id], |row| {
            Ok(MessageRow {
                role: row.get(0)?,
                content_json: row.get(1)?,
                created_timestamp: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

fn project_session(
    path: &Path,
    db: &Connection,
    message_columns: &BTreeSet<String>,
    session: SessionRow,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let rows = read_messages(db, message_columns, &session.id)?;
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut started_at = session.started_at.clone();
    let mut ended_at = session.ended_at.clone();
    for (index, row) in rows.into_iter().enumerate() {
        let timestamp = unix_seconds_to_rfc3339(row.created_timestamp);
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        project_content(
            index,
            &timestamp,
            &row.role,
            &row.content_json,
            include_deferred_content,
            &mut messages,
            &mut events,
        );
    }
    let is_top_level = is_catalog_session(&session.session_type);
    finish_source_conversation(
        Source::Goose,
        path,
        session.id,
        session.title,
        session.project,
        session.model,
        started_at,
        ended_at,
        messages,
        events,
        is_top_level,
        ConversationFinishPrep::NONE,
    )
}

fn project_content(
    index: usize,
    timestamp: &str,
    role: &str,
    content_json: &str,
    include_deferred_content: bool,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
) {
    let blocks = parse_blocks(content_json);
    let mut texts = Vec::new();
    for block in &blocks {
        let kind = block.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "toolRequest" => {
                let call = block
                    .get("toolCall")
                    .and_then(Value::as_object)
                    .and_then(|object| object.get("value"))
                    .unwrap_or(block);
                let name = optional_text(call, &["name"]);
                let arguments = call.get("arguments").map(|value| match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                });
                events.push(semantic_event(
                    index,
                    EventKind::ToolCall,
                    timestamp,
                    Some(EventActor::Assistant),
                    name,
                    arguments,
                    normalize_tool_call_details(call),
                ));
            }
            "toolResponse" => {
                let result = block
                    .get("toolResult")
                    .and_then(Value::as_object)
                    .and_then(|object| object.get("value"))
                    .cloned()
                    .unwrap_or_else(|| block.clone());
                let output = content_text(&result);
                let failed = result
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut payload = serde_json::json!({
                    "call_id": block.get("id").cloned().unwrap_or(Value::Null),
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
            "thinking" | "reasoning" => {
                let text = optional_text(block, &["thinking", "text", "reasoning"]);
                if let Some(text) = text {
                    events.push(semantic_event(
                        index,
                        EventKind::Plan,
                        timestamp,
                        Some(EventActor::Assistant),
                        None,
                        Some(text.clone()),
                        serde_json::json!({ "type": kind, "text": text }),
                    ));
                }
            }
            _ => {
                let text = content_text(block);
                if !text.is_empty() {
                    texts.push(text);
                }
            }
        }
    }
    if texts.is_empty() && blocks.is_empty() {
        let fallback = content_json.trim();
        if !fallback.is_empty() && !fallback.starts_with('[') && !fallback.starts_with('{') {
            texts.push(fallback.to_string());
        }
    }
    if !texts.is_empty() {
        push_projected_message(
            index,
            timestamp,
            role,
            &Value::String(texts.join("\n")),
            serde_json::json!({ "role": role }),
            messages,
            events,
        );
    }
}

fn parse_blocks(content_json: &str) -> Vec<Value> {
    let trimmed = content_json.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Array(items)) => items,
        Ok(value @ Value::Object(_)) => vec![value],
        _ => Vec::new(),
    }
}

fn text_column(columns: &BTreeSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("COALESCE({name}, '')")
    } else {
        "''".to_string()
    }
}
