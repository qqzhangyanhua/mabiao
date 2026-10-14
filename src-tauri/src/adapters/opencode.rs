use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::adapters::{finish, has_billable_tokens, i64_field, text_field};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    let roots = overrides
        .get("OPENCODE_DATA_DIR")
        .cloned()
        .unwrap_or_else(|| vec![home.join(".local/share/opencode")]);
    let db = overrides.get("OPENCODE_DB").and_then(|paths| paths.first());
    let mut planned: Vec<PathBuf> = roots.iter().map(|root| db_file(root, db)).collect();
    planned.sort();
    planned.dedup();
    let existing: Vec<PathBuf> = planned
        .iter()
        .filter(|path| path.is_file())
        .cloned()
        .collect();
    if !existing.is_empty() {
        return existing;
    }
    // 绝对路径的库还不存在时，旧 JSON 仍在数据目录的 storage/ 下，不在那个绝对路径旁边。
    if db.is_some_and(|path| path.is_absolute()) {
        let mut fallback: Vec<PathBuf> =
            roots.iter().map(|root| root.join("opencode.db")).collect();
        fallback.sort();
        fallback.dedup();
        return fallback;
    }
    planned
}

/// 数据目录的环境变量补进覆盖表。`OPENCODE_DATA_DIR` 已有（设置页或同名环境变量）时
/// 不再用 `XDG_DATA_HOME`。`OPENCODE_DB` 是库文件名或绝对路径，不是另一个数据根。
pub(crate) fn apply_env(overrides: &mut PathOverrides) {
    let xdg = trimmed_env("XDG_DATA_HOME");
    let db = trimmed_env("OPENCODE_DB").filter(|value| value != ":memory:");
    apply_env_values(overrides, xdg.as_deref(), db.as_deref());
}

pub(crate) fn apply_env_values(
    overrides: &mut PathOverrides,
    xdg_data_home: Option<&str>,
    opencode_db: Option<&str>,
) {
    if !overrides.contains_key("OPENCODE_DATA_DIR") {
        if let Some(xdg) = xdg_data_home
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            overrides.insert(
                "OPENCODE_DATA_DIR",
                vec![PathBuf::from(xdg).join("opencode")],
            );
        }
    }
    if let Some(db) = opencode_db
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != ":memory:")
    {
        overrides.insert("OPENCODE_DB", vec![PathBuf::from(db)]);
    }
}

/// 有 `opencode.db`（或 `OPENCODE_DB`）就只读它。库不在时才读 1.2 以前留在
/// `storage/session/<项目>/<id>.json` 的会话；库是从这些文件迁出来的，两边都在时只读库。
pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        if root.is_file() {
            paths.push(root.clone());
            continue;
        }
        let Some(data_dir) = root.parent() else {
            continue;
        };
        paths.extend(legacy_sessions(&data_dir.join("storage"))?);
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    if is_legacy_session(path) {
        return legacy_sidecar(path);
    }
    let wal = PathBuf::from(format!("{}-wal", path.to_string_lossy()));
    ingest::metadata_fingerprint(&wal)
}

pub(crate) fn is_legacy_session(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("json")
        && ancestor_name(path, 2) == Some("session")
        && ancestor_name(path, 3) == Some("storage")
}

pub(crate) fn legacy_sidecar(path: &Path) -> String {
    let mut prints = Vec::new();
    let Some(storage) = legacy_storage(path) else {
        return String::new();
    };
    let session_id = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    collect_json_fingerprints(&storage.join("message").join(session_id), &mut prints);
    if let Ok(entries) = fs::read_dir(storage.join("message").join(session_id)) {
        let mut messages: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        messages.sort();
        for message in messages {
            if let Some(id) = message.file_stem().and_then(|name| name.to_str()) {
                collect_json_fingerprints(&storage.join("part").join(id), &mut prints);
            }
        }
    }
    prints.join("|")
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    if is_legacy_session(path) {
        return parse_legacy_session(path);
    }
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

fn db_file(root: &Path, db: Option<&PathBuf>) -> PathBuf {
    match db {
        Some(path) if path.is_absolute() => path.clone(),
        Some(path) => root.join(path),
        None => root.join("opencode.db"),
    }
}

fn trimmed_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn ancestor_name(path: &Path, generations: usize) -> Option<&str> {
    let mut current = path;
    for _ in 0..generations {
        current = current.parent()?;
    }
    current.file_name().and_then(|name| name.to_str())
}

fn legacy_storage(path: &Path) -> Option<PathBuf> {
    path.parent()?.parent()?.parent().map(Path::to_path_buf)
}

fn legacy_sessions(storage: &Path) -> Result<Vec<PathBuf>, String> {
    let session_root = storage.join("session");
    if !session_root.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    let projects = fs::read_dir(&session_root).map_err(|error| {
        format!(
            "扫描 OpenCode 旧会话目录 {} 失败：{error}",
            session_root.display()
        )
    })?;
    for project in projects {
        let project = project.map_err(|error| {
            format!(
                "读取 OpenCode 旧会话目录 {} 失败：{error}",
                session_root.display()
            )
        })?;
        if !project.path().is_dir() {
            continue;
        }
        let files = fs::read_dir(project.path()).map_err(|error| {
            format!(
                "扫描 OpenCode 旧会话目录 {} 失败：{error}",
                project.path().display()
            )
        })?;
        for file in files {
            let file = file.map_err(|error| error.to_string())?;
            let path = file.path();
            if path.extension() == Some(OsStr::new("json")) {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

fn collect_json_fingerprints(dir: &Path, prints: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension() == Some(OsStr::new("json")))
        .collect();
    files.sort();
    for file in files {
        prints.push(ingest::metadata_fingerprint(&file));
    }
}

fn parse_legacy_session(path: &Path) -> Result<Vec<UsageRecord>, String> {
    let session: Value = read_json(path)?;
    let session_id = session
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if session_id.is_empty() {
        return Err("OpenCode 会话文件缺少 id".to_string());
    }
    let Some(storage) = legacy_storage(path) else {
        return Err("OpenCode 旧会话不在 storage/session 下".to_string());
    };
    let message_dir = storage.join("message").join(&session_id);
    if !message_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&message_dir)
        .map_err(|error| format!("读取 OpenCode 旧消息目录失败：{error}"))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    files.sort();
    let loc = path.to_string_lossy().into_owned();
    let mut messages = Vec::new();
    for file in files {
        if file.extension() != Some(OsStr::new("json")) {
            continue;
        }
        messages.push(OpencodeMessage {
            session_id: session_id.clone(),
            source_file: loc.clone(),
            data: read_json(&file)?,
        });
    }
    Ok(parse_opencode_messages(&messages))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let raw =
        fs::read_to_string(path).map_err(|error| format!("读取 OpenCode 会话文件失败：{error}"))?;
    serde_json::from_str(&raw).map_err(|error| format!("OpenCode 会话 JSON 无效：{error}"))
}
