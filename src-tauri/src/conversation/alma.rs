use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use super::toolbox::*;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::ingest;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(roots
        .iter()
        .map(|root| root.join("chat_threads.db"))
        .filter(|path| path.is_file())
        .collect())
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
        .ok_or_else(|| "Alma 数据库中未找到该会话".to_string())
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    super::regular_source_revision(path)
}

fn fatal(path: &Path, message: String) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: Some("alma_schema".to_string()),
        line: None,
    }
}

fn project(
    path: &Path,
    session_filter: Option<&str>,
    include_deferred_content: bool,
) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let db = ingest::open_readonly_uri(path)
        .map_err(|error| fatal(path, format!("只读打开 Alma 数据库失败：{error}")))?;
    let threads = table_columns(&db, "chat_threads").map_err(|message| fatal(path, message))?;
    if !threads.contains("id") {
        return Err(fatal(path, "Alma chat_threads 表缺少 id".to_string()));
    }
    let messages = table_columns(&db, "chat_messages").map_err(|message| fatal(path, message))?;
    if !messages.contains("thread_id") || !messages.contains("message") {
        return Err(fatal(
            path,
            "Alma chat_messages 表缺少 thread_id / message".to_string(),
        ));
    }
    let workspaces = table_columns(&db, "workspaces").unwrap_or_default();
    let conversations = read_threads(&db, &threads, &workspaces, session_filter)
        .map_err(|message| fatal(path, message))?
        .into_iter()
        .map(|thread| project_thread(path, &db, &messages, thread, include_deferred_content))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|message| fatal(path, message))?;
    Ok(ConversationIndexBatch {
        conversations,
        diagnostics: Vec::new(),
    })
}

struct ThreadRow {
    id: String,
    title: String,
    model: String,
    project: String,
    created_at: String,
    updated_at: String,
}

fn read_threads(
    db: &Connection,
    threads: &BTreeSet<String>,
    workspaces: &BTreeSet<String>,
    session_filter: Option<&str>,
) -> Result<Vec<ThreadRow>, String> {
    let cwd = if threads.contains("workspace_id")
        && workspaces.contains("id")
        && workspaces.contains("path")
    {
        "COALESCE(w.path, '')"
    } else {
        "''"
    };
    let join = if cwd == "''" {
        String::new()
    } else {
        " LEFT JOIN workspaces w ON w.id = t.workspace_id".to_string()
    };
    let filter = if let Some(session_id) = session_filter {
        format!(" AND t.id = '{}'", session_id.replace('\'', "''"))
    } else {
        String::new()
    };
    let sql = format!(
        "SELECT t.id, {title}, {model}, {created}, {updated}, {cwd}
         FROM chat_threads t{join}
         WHERE {skip}{filter}
         ORDER BY t.id",
        title = coalesce(threads, "t", "title", "''"),
        model = coalesce(threads, "t", "model", "''"),
        created = coalesce(threads, "t", "created_at", "''"),
        updated = coalesce(threads, "t", "updated_at", "''"),
        skip = skip_thread_sql(threads),
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(ThreadRow {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                model: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                created_at: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                updated_at: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                project: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

fn project_thread(
    path: &Path,
    db: &Connection,
    message_cols: &BTreeSet<String>,
    thread: ThreadRow,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let order = if message_cols.contains("timestamp") {
        "timestamp"
    } else if message_cols.contains("created_at") {
        "created_at"
    } else {
        "rowid"
    };
    let parent_tool = if message_cols.contains("parent_tool_call_id") {
        "COALESCE(parent_tool_call_id, '')"
    } else {
        "''"
    };
    let timestamp = if message_cols.contains("timestamp") {
        "COALESCE(timestamp, '')"
    } else {
        "''"
    };
    let metadata = if message_cols.contains("metadata") {
        "COALESCE(metadata, '{}')"
    } else {
        "'{}'"
    };
    let sql = format!(
        "SELECT message, {timestamp}, {metadata}, {parent_tool}
         FROM chat_messages WHERE thread_id = ?1 ORDER BY {order}, rowid"
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([&thread.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;

    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut started_at = thread.created_at.clone();
    let mut ended_at = thread.updated_at.clone();
    let model = crate::adapters::alma::alma_model(&thread.model, "");
    let mut title = thread.title.clone();
    if title == "New Chat" {
        title.clear();
    }
    for (index, (raw, timestamp, metadata, parent_tool)) in rows.into_iter().enumerate() {
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        if serde_json::from_str::<Value>(&metadata)
            .ok()
            .and_then(|value| value.get("isCompactionIndicator")?.as_bool())
            == Some(true)
        {
            continue;
        }
        let payload: Value = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !parent_tool.is_empty() {
            continue;
        }
        let role = first_text(&payload, &["role"]);
        let parts = payload
            .get("parts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let text = parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if matches!(role.as_str(), "user" | "assistant") && !text.is_empty() {
            push_projected_message(
                index,
                &timestamp,
                &role,
                &Value::String(text),
                payload.clone(),
                &mut messages,
                &mut events,
            );
        }
        for part in &parts {
            let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
            if kind == "reasoning" {
                events.push(semantic_event(
                    index,
                    EventKind::Plan,
                    &timestamp,
                    Some(EventActor::Assistant),
                    None,
                    optional_text(part, &["text"]),
                    part.clone(),
                ));
            } else if let Some(name) = kind.strip_prefix("tool-") {
                events.push(semantic_event(
                    index,
                    EventKind::ToolCall,
                    &timestamp,
                    Some(EventActor::Assistant),
                    Some(name.to_string()),
                    part.get("input").map(Value::to_string),
                    normalize_tool_call_details(part),
                ));
                if part.get("output").is_some() {
                    events.push(tool_result_event(
                        index,
                        &timestamp,
                        &normalize_tool_result_details(part),
                        include_deferred_content,
                    ));
                }
            } else if kind == "dynamic-tool" {
                events.push(semantic_event(
                    index,
                    EventKind::ToolCall,
                    &timestamp,
                    Some(EventActor::Assistant),
                    optional_text(part, &["toolName"]),
                    part.get("input").map(Value::to_string),
                    normalize_tool_call_details(part),
                ));
            }
        }
    }
    finish_source_conversation(
        Source::Alma,
        path,
        thread.id,
        title,
        thread.project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        true,
        ConversationFinishPrep::NONE,
    )
}

fn skip_thread_sql(cols: &BTreeSet<String>) -> String {
    let mut conds = Vec::new();
    if cols.contains("is_incognito") {
        conds.push("COALESCE(t.is_incognito, 0) = 0".to_string());
    }
    if cols.contains("metadata") {
        conds.push(
            "COALESCE(json_extract(CASE WHEN json_valid(t.metadata) THEN t.metadata END, '$.isCron'), 0) = 0"
                .to_string(),
        );
    }
    if cols.contains("title") {
        conds.push("instr(COALESCE(t.title, ''), '⏰ Cron:') <> 1".to_string());
    }
    if conds.is_empty() {
        "1 = 1".to_string()
    } else {
        conds.join(" AND ")
    }
}

fn table_columns(db: &Connection, table: &str) -> Result<BTreeSet<String>, String> {
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

fn coalesce(columns: &BTreeSet<String>, alias: &str, name: &str, fallback: &str) -> String {
    if columns.contains(name) {
        if alias.is_empty() {
            format!("COALESCE({name}, {fallback})")
        } else {
            format!("COALESCE({alias}.{name}, {fallback})")
        }
    } else {
        fallback.to_string()
    }
}
