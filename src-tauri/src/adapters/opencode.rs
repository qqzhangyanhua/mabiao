use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::adapters::{finish, has_billable_tokens, i64_field, text_field};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(
        overrides,
        home,
        "OPENCODE_DATA_DIR",
        ".local/share/opencode",
        "opencode.db",
    )
}

/// 扫描目录解析出来的就是数据库文件本身；发现退化成「这个文件存在吗」。
pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(roots.iter().filter(|path| path.exists()).cloned().collect())
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    let wal = PathBuf::from(format!("{}-wal", path.to_string_lossy()));
    ingest::metadata_fingerprint(&wal)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let source_db = ingest::open_readonly(path)?;
    let loc = path.to_string_lossy().into_owned();
    let mut messages = Vec::new();
    if table_exists(&source_db, "session_message") {
        messages.extend(read_v2_messages(&source_db, &loc)?);
    }
    if table_exists(&source_db, "message") {
        let skip = v1_session_ids_to_skip(&source_db)?;
        messages.extend(read_v1_messages(&source_db, &loc, &skip)?);
    }
    Ok(parse_opencode_messages(&messages))
}

pub fn parse_opencode_messages(rows: &[OpencodeMessage]) -> Vec<UsageRecord> {
    rows.iter().filter_map(parse_one).collect()
}

#[derive(Debug, Clone)]
pub struct OpencodeMessage {
    pub session_id: String,
    pub source_file: String,
    pub data: Value,
}

fn parse_one(row: &OpencodeMessage) -> Option<UsageRecord> {
    let role = row.data.get("role").and_then(|v| v.as_str());
    let compaction = row.data.get("type").and_then(|v| v.as_str()) == Some("compaction");
    if role != Some("assistant") && !compaction {
        return None;
    }
    let tokens = row.data.get("tokens").cloned().unwrap_or_default();
    if !tokens.is_object() {
        return None;
    }
    let time = row.data.get("time").cloned().unwrap_or(Value::Null);
    // 进行中的助手消息只有半截 token；compaction 往往只有 created。
    if time.get("completed").is_none() && !compaction {
        return None;
    }
    let cache = tokens.get("cache").cloned().unwrap_or_default();
    let path = row.data.get("path").cloned().unwrap_or_default();
    let project = text_field(&path, &["root", "cwd"]);
    let model = {
        let nested = row.data.get("model").cloned().unwrap_or(Value::Null);
        let id = text_field(&row.data, &["modelID", "modelId"]);
        if id.is_empty() {
            text_field(&nested, &["id"])
        } else {
            id
        }
    };
    let provider = {
        let nested = row.data.get("model").cloned().unwrap_or(Value::Null);
        let id = text_field(&row.data, &["providerID", "providerId"]);
        if id.is_empty() {
            text_field(&nested, &["providerID", "providerId"])
        } else {
            id
        }
    };
    let occurred = time
        .get("created")
        .and_then(|v| v.as_i64())
        .map(millis_to_rfc3339)
        .unwrap_or_default();
    let native_cost = row
        .data
        .get("cost")
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_i64().map(|n| n as f64))
                .or_else(|| v.get("total").and_then(|n| n.as_f64()))
        })
        .filter(|amount| *amount > 0.0);
    let record = finish(UsageRecord {
        occurred_at: occurred,
        source: Source::Opencode,
        model,
        provider,
        project,
        session_id: row.session_id.clone(),
        source_file: row.source_file.clone(),
        input_tokens: i64_field(&tokens, &["input"]),
        output_tokens: i64_field(&tokens, &["output"]),
        cache_read_tokens: i64_field(&cache, &["read"]),
        cache_creation_tokens: i64_field(&cache, &["write"]),
        reasoning_tokens: i64_field(&tokens, &["reasoning"]),
        total_tokens: 0,
        native_cost,
    });
    has_billable_tokens(&record).then_some(record)
}

fn read_v1_messages(
    db: &Connection,
    loc: &str,
    skip: &HashSet<String>,
) -> Result<Vec<OpencodeMessage>, String> {
    let mut stmt = db
        .prepare("SELECT session_id, data FROM message")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut messages = Vec::new();
    for row in rows {
        let (session_id, data) = row.map_err(|e| e.to_string())?;
        if skip.contains(&session_id) {
            continue;
        }
        let data = serde_json::from_str(&data)
            .map_err(|error| format!("OpenCode message JSON 无效：{error}"))?;
        messages.push(OpencodeMessage {
            session_id,
            source_file: loc.to_string(),
            data,
        });
    }
    Ok(messages)
}

fn read_v2_messages(db: &Connection, loc: &str) -> Result<Vec<OpencodeMessage>, String> {
    let has_session_v2 = table_exists(db, "session_v2");
    let sql = if has_session_v2 {
        "SELECT m.session_id, m.type, m.data, COALESCE(s.directory, '')
         FROM session_message m
         LEFT JOIN session_v2 s ON s.id = m.session_id
         WHERE m.type IN ('assistant', 'compaction')"
    } else {
        "SELECT session_id, type, data, '' FROM session_message
         WHERE type IN ('assistant', 'compaction')"
    };
    let mut stmt = db.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut messages = Vec::new();
    for row in rows {
        let (session_id, typ, raw, directory) = row.map_err(|e| e.to_string())?;
        let mut data: Value = serde_json::from_str(&raw)
            .map_err(|error| format!("OpenCode session_message JSON 无效：{error}"))?;
        if let Some(object) = data.as_object_mut() {
            object.entry("role").or_insert(json!("assistant"));
            object.insert("type".into(), json!(typ));
            if !directory.is_empty() && !object.contains_key("path") {
                object.insert(
                    "path".into(),
                    json!({ "cwd": directory, "root": directory }),
                );
            }
        }
        messages.push(OpencodeMessage {
            session_id,
            source_file: loc.to_string(),
            data,
        });
    }
    Ok(messages)
}

fn v1_session_ids_to_skip(db: &Connection) -> Result<HashSet<String>, String> {
    if !table_exists(db, "session_v2") {
        return Ok(HashSet::new());
    }
    if migration_completed(db) {
        let mut stmt = db
            .prepare("SELECT DISTINCT session_id FROM message")
            .map_err(|e| e.to_string())?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        return ids
            .collect::<Result<HashSet<_>, _>>()
            .map_err(|e| e.to_string());
    }
    let mut stmt = db
        .prepare("SELECT id FROM session_v2")
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    ids.collect::<Result<HashSet<_>, _>>()
        .map_err(|e| e.to_string())
}

fn table_exists(db: &Connection, name: &str) -> bool {
    db.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .ok()
    .is_some_and(|n| n > 0)
}

fn migration_completed(db: &Connection) -> bool {
    if !table_exists(db, "kv") {
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

fn millis_to_rfc3339(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}
