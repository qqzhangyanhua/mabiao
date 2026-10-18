//! Crush 对话记录：`crush.db` 的 `sessions` + `messages`。
//!
//! `parts` 是 `{type, data}` 包装数组：`text` / `reasoning` / `tool_call` / `tool_result`。
//! `parent_session_id` 非空的任务 / 标题子会话不进目录（对齐 Crush `ListSessions`）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::Value;

use super::toolbox::*;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::crush::{
    is_root_session, project_for_db, table_columns, unix_seconds_to_rfc3339,
};
use crate::domain::Source;
use crate::ingest;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::crush::discover(roots)
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
        .ok_or_else(|| "Crush 数据库中未找到该会话".to_string())
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    super::discover::wal_source_revision(path, "Crush")
}

fn fatal(path: &Path, message: String) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: Some("crush_schema".to_string()),
        line: None,
    }
}

struct SessionRow {
    id: String,
    title: String,
    parent_session_id: String,
    created_at: i64,
    updated_at: i64,
}

struct MessageRow {
    role: String,
    parts_json: String,
    model: String,
    created_at: i64,
}

fn project(
    path: &Path,
    session_filter: Option<&str>,
    include_deferred_content: bool,
) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let db = ingest::open_readonly_uri(path)
        .map_err(|error| fatal(path, format!("只读打开 Crush 数据库失败：{error}")))?;
    let sessions = table_columns(&db, "sessions").map_err(|message| fatal(path, message))?;
    if !sessions.contains("id") {
        return Err(fatal(path, "Crush sessions 表缺少 id".to_string()));
    }
    let messages = table_columns(&db, "messages").map_err(|message| fatal(path, message))?;
    if !messages.contains("session_id") || !messages.contains("role") {
        return Err(fatal(
            path,
            "Crush messages 表缺少 session_id 或 role".to_string(),
        ));
    }
    let project_name = project_for_db(path, path.parent().unwrap_or(path));
    let conversations = read_sessions(&db, &sessions, session_filter)
        .map_err(|message| fatal(path, message))?
        .into_iter()
        .filter(|session| is_root_session(&session.parent_session_id))
        .map(|session| {
            project_session(
                path,
                &db,
                &messages,
                session,
                &project_name,
                include_deferred_content,
            )
        })
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
        "SELECT {id}, {title}, {parent}, {created_at}, {updated_at}
         FROM sessions
         WHERE (?1 IS NULL OR id = ?1)
         ORDER BY id",
        id = text_column(columns, "id"),
        title = text_column(columns, "title"),
        parent = text_column(columns, "parent_session_id"),
        created_at = int_column(columns, "created_at"),
        updated_at = int_column(columns, "updated_at"),
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![session_filter], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                title: row.get(1)?,
                parent_session_id: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
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
    let parts = if columns.contains("parts") {
        "COALESCE(parts, '[]')"
    } else {
        "'[]'"
    };
    let model = text_column(columns, "model");
    let timestamp = int_column(columns, "created_at");
    let order = if columns.contains("created_at") {
        "created_at, id"
    } else {
        "id"
    };
    let sql = format!(
        "SELECT role, {parts}, {model}, {timestamp}
         FROM messages
         WHERE session_id = ?1
         ORDER BY {order}"
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([session_id], |row| {
            Ok(MessageRow {
                role: row.get(0)?,
                parts_json: row.get(1)?,
                model: row.get(2)?,
                created_at: row.get(3)?,
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
    project: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let rows = read_messages(db, message_columns, &session.id)?;
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut started_at = unix_seconds_to_rfc3339(session.created_at);
    let mut ended_at = unix_seconds_to_rfc3339(session.updated_at);
    let mut model = String::new();
    for (index, row) in rows.into_iter().enumerate() {
        if model.is_empty() && !row.model.trim().is_empty() {
            model = row.model.trim().to_string();
        }
        let timestamp = unix_seconds_to_rfc3339(row.created_at);
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        project_parts(
            index,
            &timestamp,
            &row.role,
            &row.parts_json,
            include_deferred_content,
            &mut messages,
            &mut events,
        );
    }
    finish_source_conversation(
        Source::Crush,
        path,
        session.id,
        session.title,
        project.to_string(),
        model,
        started_at,
        ended_at,
        messages,
        events,
        true,
        ConversationFinishPrep::NONE,
    )
}

fn project_parts(
    index: usize,
    timestamp: &str,
    role: &str,
    parts_json: &str,
    include_deferred_content: bool,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
) {
    let parts = parse_parts(parts_json);
    let mut texts = Vec::new();
    for part in &parts {
        let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
        let data = part.get("data").unwrap_or(part);
        match kind {
            "tool_call" => {
                let name = optional_text(data, &["name"]);
                let arguments = data.get("input").map(|value| match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                });
                let mut details = normalize_tool_call_details(data);
                if let Some(id) = optional_text(data, &["id"]) {
                    details["call_id"] = Value::String(id);
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
            "tool_result" => {
                let output = optional_text(data, &["content", "data"]).unwrap_or_default();
                let failed = data
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut payload = serde_json::json!({
                    "call_id": data.get("tool_call_id").cloned().unwrap_or(Value::Null),
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
            "reasoning" => {
                if let Some(text) = optional_text(data, &["thinking", "text"]) {
                    events.push(semantic_event(
                        index,
                        EventKind::Plan,
                        timestamp,
                        Some(EventActor::Assistant),
                        None,
                        Some(text.clone()),
                        serde_json::json!({ "type": "reasoning", "text": text }),
                    ));
                }
            }
            "shell_command" => {
                let command = optional_text(data, &["command"]).unwrap_or_default();
                let output = optional_text(data, &["output"]).unwrap_or_default();
                let code = data.get("exit_code").and_then(Value::as_i64).unwrap_or(0);
                let text = format!("$ {command}\n{output}\n(exit code {code})");
                texts.push(text);
            }
            "text" => {
                let hidden = data.get("hidden").and_then(Value::as_bool).unwrap_or(false);
                if hidden {
                    continue;
                }
                if let Some(text) = optional_text(data, &["text"]) {
                    texts.push(text);
                }
            }
            "finish" | "image_url" | "binary" => {}
            _ => {
                if let Some(text) = optional_text(data, &["text", "thinking", "content"]) {
                    texts.push(text);
                }
            }
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

#[derive(Deserialize)]
struct PartWrapper {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    data: Value,
}

fn parse_parts(parts_json: &str) -> Vec<Value> {
    let trimmed = parts_json.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Ok(wrapped) = serde_json::from_str::<Vec<PartWrapper>>(trimmed) {
        return wrapped
            .into_iter()
            .map(|part| {
                serde_json::json!({
                    "type": part.kind,
                    "data": part.data,
                })
            })
            .collect();
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

fn int_column(columns: &BTreeSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("COALESCE({name}, 0)")
    } else {
        "0".to_string()
    }
}
