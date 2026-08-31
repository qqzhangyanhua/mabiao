use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use super::toolbox::*;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::ingest;

#[cfg(test)]
#[path = "opencode_test.rs"]
mod tests;

#[derive(Debug)]
struct SessionRow {
    id: String,
    title: String,
    directory: String,
    created_at: Option<i64>,
    updated_at: Option<i64>,
}

#[derive(Debug)]
struct MessageRow {
    id: String,
    session_id: String,
    created_at: Option<i64>,
    data: Option<Value>,
}

#[derive(Debug)]
struct PartRow {
    id: String,
    message_id: String,
    session_id: String,
    created_at: Option<i64>,
    data: Option<Value>,
}

struct DatabaseSnapshot {
    sessions: Vec<SessionRow>,
    messages: Vec<MessageRow>,
    parts: Vec<PartRow>,
    capabilities: Vec<String>,
    diagnostics: Vec<ConversationIndexIssue>,
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let snapshot = read_snapshot(path).map_err(|message| fatal_issue(path, message))?;
    let diagnostics = snapshot.diagnostics.clone();
    let conversations = project_snapshot(path, snapshot, None, false)
        .map_err(|message| fatal_issue(path, message))?;
    Ok(ConversationIndexBatch {
        conversations,
        diagnostics,
    })
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let snapshot = read_snapshot(path)?;
    project_snapshot(path, snapshot, Some(session_id), include_deferred_content)?
        .into_iter()
        .next()
        .ok_or_else(|| "OpenCode 数据库中未找到该会话".to_string())
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    fs::metadata(path).map_err(|error| format!("读取 OpenCode 数据库元数据失败：{error}"))?;
    let main = ingest::metadata_fingerprint(path);
    let wal_path = sidecar_path(path, "-wal");
    let wal = match fs::metadata(&wal_path) {
        Ok(_) => ingest::metadata_fingerprint(&wal_path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "missing".to_string(),
        Err(error) => return Err(format!("读取 OpenCode WAL 元数据失败：{error}")),
    };
    serde_json::to_string(&(main, wal)).map_err(|error| error.to_string())
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn fatal_issue(path: &Path, message: String) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: Some("opencode_schema".to_string()),
        line: None,
    }
}

fn diagnostic(path: &Path, message: &str, event_type: &str) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message: message.to_string(),
        event_type: Some(event_type.to_string()),
        line: None,
    }
}

fn read_snapshot(path: &Path) -> Result<DatabaseSnapshot, String> {
    let mut source_db = ingest::open_readonly(path)
        .map_err(|error| format!("只读打开 OpenCode 数据库失败：{error}"))?;
    source_db
        .busy_timeout(std::time::Duration::from_millis(250))
        .map_err(|error| error.to_string())?;
    let transaction = source_db
        .transaction()
        .map_err(|error| format!("开始 OpenCode 只读快照失败：{error}"))?;
    let result = read_transaction(path, &transaction);
    if result.is_ok() {
        transaction
            .commit()
            .map_err(|error| format!("结束 OpenCode 只读快照失败：{error}"))?;
    }
    result
}

fn read_transaction(path: &Path, db: &Connection) -> Result<DatabaseSnapshot, String> {
    let tables = table_names(db)?;
    if !tables.contains("session") {
        return Err("OpenCode 数据库缺少必需的 session 表".to_string());
    }
    let session_columns = table_columns(db, "session")?;
    if !session_columns.contains("id") {
        return Err("OpenCode session 表缺少必需的 id 列".to_string());
    }

    let sessions = read_sessions(db, &session_columns)?;
    let mut diagnostics = Vec::new();
    let mut message_schema_available = false;
    let messages = if !tables.contains("message") {
        diagnostics.push(diagnostic(
            path,
            "OpenCode message 表不可用；消息与事件能力已降级",
            "message_schema",
        ));
        Vec::new()
    } else {
        let columns = table_columns(db, "message")?;
        if ["id", "session_id", "data"]
            .iter()
            .all(|column| columns.contains(*column))
        {
            message_schema_available = true;
            read_messages(path, db, &columns, &mut diagnostics)?
        } else {
            diagnostics.push(diagnostic(
                path,
                "OpenCode message 表缺少必需列；消息与事件能力已降级",
                "message_schema",
            ));
            Vec::new()
        }
    };
    let mut part_schema_available = false;
    let parts = if !tables.contains("part") {
        diagnostics.push(diagnostic(
            path,
            "OpenCode part 表不可用；消息正文与工具事件能力已降级",
            "part_schema",
        ));
        Vec::new()
    } else {
        let columns = table_columns(db, "part")?;
        if ["id", "message_id", "data"]
            .iter()
            .all(|column| columns.contains(*column))
        {
            part_schema_available = true;
            if messages.is_empty() {
                Vec::new()
            } else {
                read_parts(path, db, &columns, &mut diagnostics)?
            }
        } else {
            diagnostics.push(diagnostic(
                path,
                "OpenCode part 表缺少必需列；消息正文与工具事件能力已降级",
                "part_schema",
            ));
            Vec::new()
        }
    };

    let mut capabilities = Vec::new();
    if message_schema_available && part_schema_available {
        capabilities.push(CAPABILITY_MESSAGES.to_string());
        capabilities.push(CAPABILITY_EVENTS.to_string());
    }
    if message_schema_available {
        capabilities.push(CAPABILITY_USAGE.to_string());
    }
    Ok(DatabaseSnapshot {
        sessions,
        messages,
        parts,
        capabilities,
        diagnostics,
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

fn table_columns(db: &Connection, table: &str) -> Result<BTreeSet<String>, String> {
    let sql = match table {
        "session" => "PRAGMA table_info(session)",
        "message" => "PRAGMA table_info(message)",
        "part" => "PRAGMA table_info(part)",
        _ => return Err("不支持的 OpenCode 表".to_string()),
    };
    let mut statement = db.prepare(sql).map_err(|error| error.to_string())?;
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

fn read_sessions(db: &Connection, columns: &BTreeSet<String>) -> Result<Vec<SessionRow>, String> {
    let sql = format!(
        "SELECT id, {}, {}, {}, {} FROM session ORDER BY id",
        optional_column(columns, "title", "''"),
        optional_column(columns, "directory", "''"),
        optional_column(columns, "time_created", "NULL"),
        optional_column(columns, "time_updated", "NULL"),
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                directory: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
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
    path: &Path,
    db: &Connection,
    columns: &BTreeSet<String>,
    diagnostics: &mut Vec<ConversationIndexIssue>,
) -> Result<Vec<MessageRow>, String> {
    let sql = format!(
        "SELECT id, session_id, {}, data FROM message ORDER BY {}, id",
        optional_column(columns, "time_created", "NULL"),
        optional_column(columns, "time_created", "id"),
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
        .map(|(id, session_id, created_at, raw)| {
            let data = serde_json::from_str(&raw).map_err(|error| {
                diagnostics.push(diagnostic(
                    path,
                    &format!(
                        "OpenCode message 行 {id} 的 JSON 无效（line {}, column {}）",
                        error.line(),
                        error.column()
                    ),
                    "message_json",
                ));
            });
            MessageRow {
                id,
                session_id,
                created_at,
                data: data.ok(),
            }
        })
        .collect())
}

fn read_parts(
    path: &Path,
    db: &Connection,
    columns: &BTreeSet<String>,
    diagnostics: &mut Vec<ConversationIndexIssue>,
) -> Result<Vec<PartRow>, String> {
    let has_session_id = columns.contains("session_id");
    let session_expression = if has_session_id {
        "p.session_id"
    } else {
        "m.session_id"
    };
    let join = if has_session_id {
        ""
    } else {
        " JOIN message AS m ON m.id = p.message_id"
    };
    let created_expression = if columns.contains("time_created") {
        "p.time_created"
    } else {
        "NULL"
    };
    let sql = format!(
        "SELECT p.id, p.message_id, {session_expression}, {created_expression}, p.data FROM part AS p{join} ORDER BY {created_expression}, p.id"
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(id, message_id, session_id, created_at, raw)| {
            let data = match serde_json::from_str::<Value>(&raw) {
                Ok(data) => {
                    let kind = safe_kind(
                        data.get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown"),
                    );
                    if !is_known_part_kind(&kind) {
                        diagnostics.push(diagnostic(
                            path,
                            &format!("OpenCode part 行 {id} 的类型尚未适配"),
                            "part_type",
                        ));
                    }
                    Some(data)
                }
                Err(error) => {
                    diagnostics.push(diagnostic(
                        path,
                        &format!(
                            "OpenCode part 行 {id} 的 JSON 无效（line {}, column {}）",
                            error.line(),
                            error.column()
                        ),
                        "part_json",
                    ));
                    None
                }
            };
            PartRow {
                id,
                message_id,
                session_id,
                created_at,
                data,
            }
        })
        .collect())
}

fn project_snapshot(
    path: &Path,
    snapshot: DatabaseSnapshot,
    session_filter: Option<&str>,
    include_deferred_content: bool,
) -> Result<Vec<ParsedConversation>, String> {
    let capabilities = snapshot.capabilities;
    let mut messages_by_session = BTreeMap::<String, Vec<MessageRow>>::new();
    for message in snapshot.messages {
        messages_by_session
            .entry(message.session_id.clone())
            .or_default()
            .push(message);
    }
    let mut parts_by_message = BTreeMap::<String, Vec<PartRow>>::new();
    for part in snapshot.parts {
        parts_by_message
            .entry(part.message_id.clone())
            .or_default()
            .push(part);
    }

    snapshot
        .sessions
        .into_iter()
        .filter(|session| session_filter.is_none_or(|filter| session.id == filter))
        .map(|session| {
            let source_messages = messages_by_session.remove(&session.id).unwrap_or_default();
            project_session(
                path,
                session,
                source_messages,
                &mut parts_by_message,
                &capabilities,
                include_deferred_content,
            )
        })
        .collect()
}

fn project_session(
    path: &Path,
    session: SessionRow,
    mut source_messages: Vec<MessageRow>,
    parts_by_message: &mut BTreeMap<String, Vec<PartRow>>,
    capabilities: &[String],
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    if source_messages.is_empty() {
        source_messages = Vec::new();
    }
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
            let mut event = unadapted_event(
                sequence,
                &millis_timestamp(source_message.created_at),
                "invalid_message_json",
                serde_json::json!({"table":"message","row_id":source_message.id}),
            );
            set_native_event_id(path, &mut event, "message", &source_message.id, "invalid");
            events.push(event);
            sequence += 1;
            continue;
        };
        let role = data.get("role").and_then(Value::as_str).unwrap_or("");
        let message_time = data
            .get("time")
            .and_then(|time| time.get("created"))
            .and_then(Value::as_i64)
            .or(source_message.created_at);
        let timestamp = millis_timestamp(message_time);
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        if role == "assistant" {
            let next_model = first_text(data, &["modelID", "modelId"]);
            if !next_model.is_empty() {
                model = next_model;
            }
            if project.is_empty() {
                let path_data = data.get("path").unwrap_or(&Value::Null);
                project = first_text(path_data, &["root", "cwd"]);
            }
        }

        let mut parts = parts_by_message
            .remove(&source_message.id)
            .unwrap_or_default();
        parts.retain(|part| part.session_id == session.id);
        parts.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        let text = parts
            .iter()
            .filter_map(|part| part.data.as_ref())
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if matches!(role, "user" | "assistant") && !text.is_empty() {
            let message = ConversationMessage {
                role: role.to_string(),
                occurred_at: timestamp.clone(),
                text,
            };
            let mut event = message_event(
                sequence,
                &message,
                serde_json::json!({"message_id":source_message.id,"role":role}),
            );
            set_native_event_id(path, &mut event, "message", &source_message.id, "text");
            events.push(event);
            messages.push(message);
            sequence += 1;
        }
        for part in parts {
            let part_timestamp = millis_timestamp(part.created_at.or(message_time));
            update_time_bounds(&part_timestamp, &mut started_at, &mut ended_at);
            project_part(
                path,
                &part,
                &part_timestamp,
                &mut sequence,
                &mut events,
                include_deferred_content,
            );
        }
    }

    let mut parsed = finish_source_conversation(
        Source::Opencode,
        path,
        session.id,
        session.title,
        project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        true,
    )?;
    parsed.session.capabilities = capabilities.to_vec();
    Ok(parsed)
}

fn project_part(
    path: &Path,
    part: &PartRow,
    timestamp: &str,
    sequence: &mut usize,
    events: &mut Vec<ConversationEvent>,
    include_deferred_content: bool,
) {
    let Some(data) = part.data.as_ref() else {
        let mut event = unadapted_event(
            *sequence,
            timestamp,
            "invalid_part_json",
            serde_json::json!({"table":"part","row_id":part.id,"message_id":part.message_id}),
        );
        set_native_event_id(path, &mut event, "part", &part.id, "invalid");
        events.push(event);
        *sequence += 1;
        return;
    };
    let kind = safe_kind(
        data.get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
    );
    match kind.as_str() {
        "text" => {}
        "reasoning" => {
            let mut event = semantic_event(
                *sequence,
                EventKind::Plan,
                timestamp,
                Some(EventActor::Assistant),
                None,
                optional_text(data, &["text"]),
                serde_json::json!({"part_id":part.id,"type":"reasoning"}),
            );
            set_native_event_id(path, &mut event, "part", &part.id, "reasoning");
            events.push(event);
            *sequence += 1;
        }
        "tool" => {
            project_tool_part(
                path,
                part,
                data,
                timestamp,
                sequence,
                events,
                include_deferred_content,
            );
        }
        "step-start" | "step-finish" | "snapshot" | "patch" | "compaction" => {
            let mut event = semantic_event(
                *sequence,
                EventKind::SystemStatus,
                timestamp,
                None,
                Some(kind.clone()),
                None,
                serde_json::json!({"part_id":part.id,"type":kind}),
            );
            set_native_event_id(path, &mut event, "part", &part.id, "status");
            events.push(event);
            *sequence += 1;
        }
        _ => {
            let mut event = unadapted_event(
                *sequence,
                timestamp,
                &kind,
                serde_json::json!({"table":"part","row_id":part.id,"message_id":part.message_id,"type":kind}),
            );
            set_native_event_id(path, &mut event, "part", &part.id, "unadapted");
            events.push(event);
            *sequence += 1;
        }
    }
}

fn project_tool_part(
    path: &Path,
    part: &PartRow,
    data: &Value,
    timestamp: &str,
    sequence: &mut usize,
    events: &mut Vec<ConversationEvent>,
    include_deferred_content: bool,
) {
    let state = data.get("state").unwrap_or(&Value::Null);
    let call_id = first_text(data, &["callID", "callId"]);
    let name = first_text(data, &["tool", "name"]);
    let mut call = semantic_event(
        *sequence,
        EventKind::ToolCall,
        timestamp,
        Some(EventActor::Assistant),
        (!name.is_empty()).then_some(name.clone()),
        state.get("input").map(Value::to_string),
        serde_json::json!({"part_id":part.id,"call_id":call_id,"name":name,"input":state.get("input")}),
    );
    set_native_event_id(path, &mut call, "part", &part.id, "tool_call");
    events.push(call);
    *sequence += 1;

    let status = state.get("status").and_then(Value::as_str).unwrap_or("");
    if matches!(status, "completed" | "error") {
        let output = if status == "completed" {
            state.get("output")
        } else {
            state.get("error")
        };
        let details = serde_json::json!({
            "part_id":part.id,
            "call_id":call_id,
            "name":name,
            "output":output.map(content_text).unwrap_or_default(),
            "status":status
        });
        let mut result =
            tool_result_event(*sequence, timestamp, &details, include_deferred_content);
        set_native_event_id(path, &mut result, "part", &part.id, "tool_result");
        events.push(result);
        *sequence += 1;
    } else if matches!(status, "pending" | "running") {
        let mut state_event = semantic_event(
            *sequence,
            EventKind::SystemStatus,
            timestamp,
            None,
            Some(format!("tool_{status}")),
            None,
            serde_json::json!({"part_id":part.id,"call_id":call_id,"status":status}),
        );
        set_native_event_id(path, &mut state_event, "part", &part.id, "tool_status");
        events.push(state_event);
        *sequence += 1;
    }
}

fn set_native_event_id(
    path: &Path,
    event: &mut ConversationEvent,
    table: &str,
    row_id: &str,
    projection: &str,
) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    table.hash(&mut hasher);
    row_id.hash(&mut hasher);
    projection.hash(&mut hasher);
    event.event_id = format!("opencode:{:016x}", hasher.finish());
}

fn is_known_part_kind(kind: &str) -> bool {
    matches!(
        kind,
        "text"
            | "reasoning"
            | "tool"
            | "step-start"
            | "step-finish"
            | "snapshot"
            | "patch"
            | "compaction"
    )
}

fn safe_kind(kind: &str) -> String {
    if !kind.is_empty()
        && kind.len() <= 64
        && kind
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        kind.to_string()
    } else {
        "unknown".to_string()
    }
}

fn millis_timestamp(value: Option<i64>) -> String {
    value
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|timestamp| timestamp.to_rfc3339())
        .unwrap_or_default()
}
