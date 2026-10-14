//! OpenCode sqlite 快照。v1 的 session / message / part，以及 v2 的 session_v2 / session_message。

use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::Connection;
use serde_json::Value;

use super::super::toolbox::{CAPABILITY_EVENTS, CAPABILITY_MESSAGES, CAPABILITY_USAGE};
use super::{
    diagnostic, ConversationIndexIssue, DatabaseSnapshot, MessageRow, PartRow, SessionRow,
};
use crate::ingest;

pub(super) fn read_snapshot(path: &Path) -> Result<DatabaseSnapshot, String> {
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
    let has_v2 = tables.contains("session_v2") && tables.contains("session_message");
    let has_v1 = tables.contains("session");
    if !has_v1 && !has_v2 {
        return Err("OpenCode 数据库缺少必需的 session 表".to_string());
    }

    let mut sessions = Vec::new();
    let mut extra_messages = Vec::new();
    let mut extra_parts = Vec::new();
    if has_v2 {
        let (v2_sessions, v2_messages, v2_parts) = read_v2(path, db)?;
        sessions.extend(v2_sessions);
        extra_messages.extend(v2_messages);
        extra_parts.extend(v2_parts);
    }
    if has_v1 && !v2_migration_completed(db) {
        let session_columns = table_columns(db, "session")?;
        if !session_columns.contains("id") {
            return Err("OpenCode session 表缺少必需的 id 列".to_string());
        }
        let skip: BTreeSet<String> = sessions.iter().map(|session| session.id.clone()).collect();
        sessions.extend(
            read_sessions(db, &session_columns)?
                .into_iter()
                .filter(|session| !skip.contains(&session.id)),
        );
    } else if has_v1 && !has_v2 {
        let session_columns = table_columns(db, "session")?;
        if !session_columns.contains("id") {
            return Err("OpenCode session 表缺少必需的 id 列".to_string());
        }
        sessions = read_sessions(db, &session_columns)?;
    }
    let mut diagnostics = Vec::new();
    let mut message_schema_available = false;
    let skip_v1_rows = has_v2 && v2_migration_completed(db);
    let mut messages = if skip_v1_rows {
        Vec::new()
    } else if !tables.contains("message") {
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
    let mut parts = if skip_v1_rows {
        Vec::new()
    } else if !tables.contains("part") {
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

    if !extra_messages.is_empty() {
        message_schema_available = true;
        part_schema_available = true;
    }
    messages.extend(extra_messages);
    parts.extend(extra_parts);
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
    if !table
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err("不支持的 OpenCode 表".to_string());
    }
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

fn read_sessions(db: &Connection, columns: &BTreeSet<String>) -> Result<Vec<SessionRow>, String> {
    let sql = format!(
        "SELECT id, {}, {}, {}, {}, {} FROM session ORDER BY id",
        optional_column(columns, "title", "''"),
        optional_column(columns, "directory", "''"),
        optional_column(columns, "parent_id", "''"),
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
                parent_id: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
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
                    let kind = super::project::safe_kind(
                        data.get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown"),
                    );
                    if !super::project::is_known_part_kind(&kind) {
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

fn v2_migration_completed(db: &Connection) -> bool {
    let tables = table_names(db).unwrap_or_default();
    if !tables.contains("kv") {
        return false;
    }
    let Ok(raw) = db.query_row(
        "SELECT value FROM kv WHERE key = 'migration.v1-v2'",
        [],
        |row| row.get::<_, String>(0),
    ) else {
        return false;
    };
    serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|value| value.get("phase")?.as_str().map(str::to_string))
        .is_some_and(|phase| phase == "completed")
}

type V2Tables = (Vec<SessionRow>, Vec<MessageRow>, Vec<PartRow>);

fn read_v2(_path: &Path, db: &Connection) -> Result<V2Tables, String> {
    let columns = table_columns(db, "session_v2")?;
    let sql = format!(
        "SELECT id, {}, {}, {}, {}, {} FROM session_v2 ORDER BY id",
        optional_column(&columns, "title", "''"),
        optional_column(&columns, "directory", "''"),
        optional_column(&columns, "parent_id", "''"),
        optional_column(&columns, "time_created", "NULL"),
        optional_column(&columns, "time_updated", "NULL"),
    );
    let mut statement = db.prepare(&sql).map_err(|error| error.to_string())?;
    let sessions = statement
        .query_map([], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                directory: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                parent_id: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;

    let mut message_statement = db
        .prepare(
            "SELECT id, session_id, type, time_created, data FROM session_message
             WHERE type IN ('user', 'assistant', 'compaction')
             ORDER BY seq, id",
        )
        .map_err(|error| error.to_string())?;
    let rows = message_statement
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

    let mut messages = Vec::new();
    let mut parts = Vec::new();
    for (id, session_id, typ, created_at, raw) in rows {
        let data: Value = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(_) => continue,
        };
        match typ.as_str() {
            "user" => {
                let mut payload = data.clone();
                if let Value::Object(object) = &mut payload {
                    object.insert("role".into(), Value::String("user".into()));
                }
                messages.push(MessageRow {
                    id: id.clone(),
                    session_id: session_id.clone(),
                    created_at,
                    data: Some(payload),
                });
                if let Some(text) = data.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        parts.push(PartRow {
                            id: format!("{id}:text"),
                            message_id: id,
                            session_id,
                            created_at,
                            data: Some(serde_json::json!({"type":"text","text":text})),
                        });
                    }
                }
            }
            "assistant" => {
                let model = data
                    .get("model")
                    .and_then(|model| model.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let mut payload = data.clone();
                if let Value::Object(object) = &mut payload {
                    object.insert("role".into(), Value::String("assistant".into()));
                    if !model.is_empty() {
                        object.insert("modelID".into(), Value::String(model.to_string()));
                    }
                }
                messages.push(MessageRow {
                    id: id.clone(),
                    session_id: session_id.clone(),
                    created_at,
                    data: Some(payload),
                });
                if let Some(content) = data.get("content").and_then(Value::as_array) {
                    for (index, part) in content.iter().enumerate() {
                        parts.push(PartRow {
                            id: format!("{id}:{index}"),
                            message_id: id.clone(),
                            session_id: session_id.clone(),
                            created_at,
                            data: Some(part.clone()),
                        });
                    }
                }
            }
            "compaction" => {
                messages.push(MessageRow {
                    id: id.clone(),
                    session_id: session_id.clone(),
                    created_at,
                    data: Some(serde_json::json!({"role":"system"})),
                });
                parts.push(PartRow {
                    id: format!("{id}:compaction"),
                    message_id: id,
                    session_id,
                    created_at,
                    data: Some(serde_json::json!({"type":"compaction"})),
                });
            }
            _ => {}
        }
    }
    Ok((sessions, messages, parts))
}
