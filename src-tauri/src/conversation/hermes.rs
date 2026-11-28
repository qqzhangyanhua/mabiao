//! Hermes 对话记录：`<root>/state.db`（含 `profiles/*/state.db`）里的 `sessions` + `messages`。
//!
//! 消息是 OpenAI 形态：assistant 的 `tool_calls` 是 JSON 数组，工具结果是独立的 `role = tool` 行，
//! 靠 `tool_call_id` 配对。`active = 0` 的行是被回退（rewind）掉的，不进对话记录。
//! 有 `parent_session_id` 的会话（压缩续接、委派）不算顶层。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde_json::Value;

use super::toolbox::*;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::hermes::unix_seconds_to_rfc3339;
use crate::source_sqlite::{self, open_wal_readonly, sql_nullable, sql_text, wal_source_revision};

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::hermes::discover(roots)
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
        .ok_or_else(|| "Hermes 数据库中未找到该会话".to_string())
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    wal_source_revision(path, "Hermes")
}

fn fatal(path: &Path, message: String) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: Some("hermes_schema".to_string()),
        line: None,
    }
}

struct SessionRow {
    id: String,
    title: String,
    model: String,
    project: String,
    parent_id: String,
    started_at: String,
    ended_at: String,
}

struct MessageRow {
    role: String,
    content: String,
    tool_call_id: String,
    tool_calls: String,
    tool_name: String,
    timestamp: String,
    reasoning: String,
}

fn project(
    path: &Path,
    session_filter: Option<&str>,
    include_deferred_content: bool,
) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let db = open_wal_readonly(path)
        .map_err(|error| fatal(path, format!("只读打开 Hermes 数据库失败：{error}")))?;
    let sessions =
        required_table_columns(&db, "sessions").map_err(|message| fatal(path, message))?;
    if !sessions.contains("id") {
        return Err(fatal(path, "Hermes sessions 表缺少 id".to_string()));
    }
    let messages =
        required_table_columns(&db, "messages").map_err(|message| fatal(path, message))?;
    for required in ["session_id", "role"] {
        if !messages.contains(required) {
            return Err(fatal(path, format!("Hermes messages 表缺少 {required}")));
        }
    }
    let conversations = read_sessions(&db, &sessions, session_filter)
        .map_err(|message| fatal(path, message))?
        .into_iter()
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
    let ended = if columns.contains("ended_at") && columns.contains("last_activity_at") {
        "COALESCE(ended_at, last_activity_at)".to_string()
    } else if columns.contains("ended_at") {
        "ended_at".to_string()
    } else {
        sql_nullable(columns, "last_activity_at")
    };
    let sql = format!(
        "SELECT id, {title}, {model}, {cwd}, {git_root}, {parent}, {started}, {ended}
         FROM sessions
         WHERE (?1 IS NULL OR id = ?1)
         ORDER BY id",
        title = sql_text(columns, "title"),
        model = sql_text(columns, "model"),
        cwd = sql_text(columns, "cwd"),
        git_root = sql_text(columns, "git_repo_root"),
        parent = sql_text(columns, "parent_session_id"),
        started = sql_nullable(columns, "started_at"),
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![session_filter], |row| {
            let cwd: String = row.get(3)?;
            let git_root: String = row.get(4)?;
            Ok(SessionRow {
                id: row.get(0)?,
                title: row.get(1)?,
                model: row.get(2)?,
                project: if cwd.is_empty() { git_root } else { cwd },
                parent_id: row.get(5)?,
                started_at: unix_seconds_to_rfc3339(row.get::<_, Option<f64>>(6)?.unwrap_or(0.0)),
                ended_at: unix_seconds_to_rfc3339(row.get::<_, Option<f64>>(7)?.unwrap_or(0.0)),
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
    let reasoning = match (
        columns.contains("reasoning_content"),
        columns.contains("reasoning"),
    ) {
        (true, true) => "COALESCE(NULLIF(reasoning_content, ''), NULLIF(reasoning, ''), '')",
        (true, false) => "COALESCE(reasoning_content, '')",
        (false, true) => "COALESCE(reasoning, '')",
        (false, false) => "''",
    };
    let active = if columns.contains("active") {
        "AND COALESCE(active, 1) <> 0"
    } else {
        ""
    };
    let timestamp = sql_nullable(columns, "timestamp");
    let order = if columns.contains("timestamp") {
        "timestamp, rowid"
    } else {
        "rowid"
    };
    let sql = format!(
        "SELECT role, {content}, {tool_call_id}, {tool_calls}, {tool_name}, {timestamp}, {reasoning}
         FROM messages WHERE session_id = ?1 {active} ORDER BY {order}",
        content = sql_text(columns, "content"),
        tool_call_id = sql_text(columns, "tool_call_id"),
        tool_calls = sql_text(columns, "tool_calls"),
        tool_name = sql_text(columns, "tool_name"),
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([session_id], |row| {
            Ok(MessageRow {
                role: row.get(0)?,
                content: row.get(1)?,
                tool_call_id: row.get(2)?,
                tool_calls: row.get(3)?,
                tool_name: row.get(4)?,
                timestamp: unix_seconds_to_rfc3339(row.get::<_, Option<f64>>(5)?.unwrap_or(0.0)),
                reasoning: row.get(6)?,
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
        update_time_bounds(&row.timestamp, &mut started_at, &mut ended_at);
        match row.role.as_str() {
            "user" | "assistant" => {
                let content = message_content(&row.content);
                if row.role == "assistant" && !row.reasoning.trim().is_empty() {
                    events.push(semantic_event(
                        index,
                        EventKind::Plan,
                        &row.timestamp,
                        Some(EventActor::Assistant),
                        None,
                        Some(row.reasoning.clone()),
                        serde_json::json!({ "type": "reasoning", "text": row.reasoning }),
                    ));
                }
                push_projected_message(
                    index,
                    &row.timestamp,
                    &row.role,
                    &content,
                    serde_json::json!({ "role": row.role }),
                    &mut messages,
                    &mut events,
                );
                if row.role == "assistant" {
                    push_tool_calls(index, &row, &mut events);
                }
            }
            "tool" => {
                let mut payload = serde_json::json!({
                    "call_id": row.tool_call_id,
                    "output": row.content,
                });
                if !row.tool_name.is_empty() {
                    payload["name"] = Value::String(row.tool_name.clone());
                }
                events.push(tool_result_event(
                    index,
                    &row.timestamp,
                    &payload,
                    include_deferred_content,
                ));
            }
            _ => {}
        }
    }
    let is_top_level = session.parent_id.is_empty();
    if !is_top_level {
        events.push(semantic_event(
            0,
            EventKind::SystemStatus,
            &started_at,
            None,
            Some("session_started".to_string()),
            None,
            serde_json::json!({ "parent_id": session.parent_id }),
        ));
    }
    finish_source_conversation(
        Source::Hermes,
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

/// 多模态内容在库里是 JSON 字符串；解析不出来就当纯文本。
fn message_content(raw: &str) -> Value {
    let trimmed = raw.trim_start();
    if trimmed.starts_with('[') {
        if let Ok(value @ Value::Array(_)) = serde_json::from_str::<Value>(trimmed) {
            return value;
        }
    }
    Value::String(raw.to_string())
}

fn push_tool_calls(index: usize, row: &MessageRow, events: &mut Vec<ConversationEvent>) {
    let Ok(Value::Array(calls)) = serde_json::from_str::<Value>(&row.tool_calls) else {
        return;
    };
    for call in calls {
        let function = call.get("function").unwrap_or(&call);
        let name = optional_text(function, &["name"]);
        let arguments = function.get("arguments").map(|arguments| match arguments {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        });
        events.push(semantic_event(
            index,
            EventKind::ToolCall,
            &row.timestamp,
            Some(EventActor::Assistant),
            name,
            arguments,
            normalize_tool_call_details(&call),
        ));
    }
}

fn required_table_columns(db: &Connection, table: &str) -> Result<BTreeSet<String>, String> {
    let rows = source_sqlite::table_columns(db, table)?;
    if rows.is_empty() {
        return Err(format!("Hermes 数据库缺少 {table} 表"));
    }
    Ok(rows)
}
