use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use super::toolbox::*;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::ingest;

struct SessionRow {
    id: String,
    title: String,
    directory: String,
    parent_id: String,
    created_at: Option<i64>,
    updated_at: Option<i64>,
}

struct MessageRow {
    id: String,
    session_id: String,
    created_at: Option<i64>,
    data: Option<Value>,
}

struct PartRow {
    id: String,
    message_id: String,
    _session_id: String,
    _created_at: Option<i64>,
    data: Option<Value>,
}

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(roots
        .iter()
        .filter(|path| path.is_file())
        .cloned()
        .collect())
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    read_and_project(path, None, false)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    read_and_project(path, Some(session_id), include_deferred_content)
        .map_err(|issue| issue.message)?
        .conversations
        .into_iter()
        .next()
        .ok_or_else(|| "ZCode 数据库中未找到该会话".to_string())
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    super::regular_source_revision(path)
}

fn fatal(path: &Path, message: String) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: Some("zcode_schema".to_string()),
        line: None,
    }
}

fn read_and_project(
    path: &Path,
    session_filter: Option<&str>,
    include_deferred_content: bool,
) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let source_db = ingest::open_readonly(path)
        .map_err(|error| fatal(path, format!("只读打开 ZCode 数据库失败：{error}")))?;
    source_db
        .busy_timeout(std::time::Duration::from_millis(250))
        .map_err(|error| fatal(path, error.to_string()))?;
    let tables = table_names(&source_db).map_err(|message| fatal(path, message))?;
    if !tables.contains("session") || !tables.contains("message") {
        return Err(fatal(
            path,
            "ZCode 数据库缺少 session / message 表".to_string(),
        ));
    }
    let sessions = read_sessions(&source_db).map_err(|message| fatal(path, message))?;
    let messages = read_messages(&source_db).map_err(|message| fatal(path, message))?;
    let parts = if tables.contains("part") {
        read_parts(&source_db).map_err(|message| fatal(path, message))?
    } else {
        Vec::new()
    };
    let mut parts_by_message = BTreeMap::<String, Vec<PartRow>>::new();
    for part in parts {
        parts_by_message
            .entry(part.message_id.clone())
            .or_default()
            .push(part);
    }
    let mut messages_by_session = BTreeMap::<String, Vec<MessageRow>>::new();
    for message in messages {
        messages_by_session
            .entry(message.session_id.clone())
            .or_default()
            .push(message);
    }
    let conversations = sessions
        .into_iter()
        .filter(|session| session_filter.is_none_or(|filter| session.id == filter))
        .map(|session| {
            let source_messages = messages_by_session.remove(&session.id).unwrap_or_default();
            project_session(
                path,
                session,
                source_messages,
                &mut parts_by_message,
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

fn table_names(db: &Connection) -> Result<BTreeSet<String>, String> {
    let mut statement = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| row.get(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

fn read_sessions(db: &Connection) -> Result<Vec<SessionRow>, String> {
    let columns = pragma_columns(db, "session")?;
    let sql = format!(
        "SELECT id, {}, {}, {}, {}, {} FROM session ORDER BY id",
        optional_column(&columns, "title", "''"),
        optional_column(&columns, "directory", "''"),
        optional_column(&columns, "parent_id", "''"),
        optional_column(&columns, "time_created", "NULL"),
        optional_column(&columns, "time_updated", "NULL"),
    );
    let title_source = columns.contains("title_source");
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if !title_source {
        return Ok(rows
            .into_iter()
            .map(
                |(id, title, directory, parent_id, created_at, updated_at)| SessionRow {
                    id,
                    title,
                    directory,
                    parent_id,
                    created_at,
                    updated_at,
                },
            )
            .collect());
    }
    let mut sources = BTreeMap::<String, String>::new();
    if let Ok(mut statement) = db.prepare("SELECT id, COALESCE(title_source, '') FROM session") {
        if let Ok(mapped) = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }) {
            for row in mapped.flatten() {
                sources.insert(row.0, row.1);
            }
        }
    }
    Ok(rows
        .into_iter()
        .map(
            |(id, mut title, directory, parent_id, created_at, updated_at)| {
                if sources.get(&id).is_some_and(|source| source == "default") {
                    title.clear();
                }
                SessionRow {
                    id,
                    title,
                    directory,
                    parent_id,
                    created_at,
                    updated_at,
                }
            },
        )
        .collect())
}

fn read_messages(db: &Connection) -> Result<Vec<MessageRow>, String> {
    let columns = pragma_columns(db, "message")?;
    if !["id", "session_id", "data"]
        .iter()
        .all(|column| columns.contains(*column))
    {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT id, session_id, {}, data FROM message ORDER BY id",
        optional_column(&columns, "time_created", "NULL"),
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(id, session_id, created_at, raw)| MessageRow {
            id,
            session_id,
            created_at,
            data: serde_json::from_str(&raw).ok(),
        })
        .collect())
}

fn read_parts(db: &Connection) -> Result<Vec<PartRow>, String> {
    let columns = pragma_columns(db, "part")?;
    if !["id", "message_id", "data"]
        .iter()
        .all(|column| columns.contains(*column))
    {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT id, message_id, {}, {}, data FROM part ORDER BY id",
        optional_column(&columns, "session_id", "''"),
        optional_column(&columns, "time_created", "NULL"),
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(id, message_id, session_id, created_at, raw)| PartRow {
            id,
            message_id,
            _session_id: session_id,
            _created_at: created_at,
            data: serde_json::from_str(&raw).ok(),
        })
        .collect())
}

fn project_session(
    path: &Path,
    session: SessionRow,
    mut source_messages: Vec<MessageRow>,
    parts_by_message: &mut BTreeMap<String, Vec<PartRow>>,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
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
            continue;
        };
        let role = data.get("role").and_then(Value::as_str).unwrap_or("");
        let timestamp = millis_timestamp(
            data.get("time")
                .and_then(|time| time.get("created"))
                .and_then(Value::as_i64)
                .or(source_message.created_at),
        );
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        if role == "assistant" {
            let next_model = first_text(data, &["modelID", "modelId"]);
            if !next_model.is_empty() {
                model = next_model;
            }
            if project.is_empty() {
                project = first_text(data.get("path").unwrap_or(&Value::Null), &["root", "cwd"]);
            }
        }
        let mut parts = parts_by_message
            .remove(&source_message.id)
            .unwrap_or_default();
        parts.sort_by(|left, right| left.id.cmp(&right.id));
        let text = parts
            .iter()
            .filter_map(|part| part.data.as_ref())
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if matches!(role, "user" | "assistant") && !text.is_empty() {
            push_projected_message(
                sequence,
                &timestamp,
                role,
                &Value::String(text),
                serde_json::json!({"message_id": source_message.id, "role": role}),
                &mut messages,
                &mut events,
            );
            sequence += 1;
        }
        for part in parts {
            let Some(data) = part.data.as_ref() else {
                continue;
            };
            let kind = data.get("type").and_then(Value::as_str).unwrap_or("");
            match kind {
                "text" => {}
                "reasoning" => {
                    events.push(semantic_event(
                        sequence,
                        EventKind::Plan,
                        &timestamp,
                        Some(EventActor::Assistant),
                        None,
                        optional_text(data, &["text"]),
                        data.clone(),
                    ));
                    sequence += 1;
                }
                "tool" => {
                    events.push(semantic_event(
                        sequence,
                        EventKind::ToolCall,
                        &timestamp,
                        Some(EventActor::Assistant),
                        optional_text(data, &["tool", "name"]),
                        data.get("state")
                            .and_then(|state| state.get("input"))
                            .map(Value::to_string),
                        normalize_tool_call_details(data),
                    ));
                    sequence += 1;
                    let state = data.get("state").unwrap_or(&Value::Null);
                    if matches!(
                        state.get("status").and_then(Value::as_str),
                        Some("completed" | "error")
                    ) {
                        events.push(tool_result_event(
                            sequence,
                            &timestamp,
                            &normalize_tool_result_details(state),
                            include_deferred_content,
                        ));
                        sequence += 1;
                    }
                }
                _ => {}
            }
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
        Source::Zcode,
        path,
        session.id,
        session.title,
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

fn pragma_columns(db: &Connection, table: &str) -> Result<BTreeSet<String>, String> {
    let mut statement = db
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| row.get(1))
        .map_err(|error| error.to_string())?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

fn optional_column(columns: &BTreeSet<String>, column: &str, fallback: &str) -> String {
    if columns.contains(column) {
        column.to_string()
    } else {
        fallback.to_string()
    }
}

fn millis_timestamp(value: Option<i64>) -> String {
    value
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|timestamp| timestamp.to_rfc3339())
        .unwrap_or_default()
}
