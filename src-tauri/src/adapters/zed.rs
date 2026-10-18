use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use crate::adapters::goose::{parse_sql_timestamp, table_columns};
use crate::adapters::{finish, has_billable_tokens};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// Zed 把内置 Agent 线程写进一份 SQLite：`threads/threads.db`。
///
/// `$ZED_DATA_DIR` 覆盖 Zed 数据根并拼 `threads/`（对齐上游 `paths::data_dir()`）。
/// 未覆盖时扫 Linux XDG、macOS Application Support、Windows LocalAppData。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides
        .get("ZED_DATA_DIR")
        .cloned()
        .map(|roots| roots.into_iter().map(|root| root.join("threads")).collect())
        .unwrap_or_else(|| {
            vec![
                home.join(".local/share/zed/threads"),
                home.join("Library/Application Support/Zed/threads"),
                home.join("AppData/Local/Zed/threads"),
            ]
        })
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(roots
        .iter()
        .map(|root| root.join("threads.db"))
        .filter(|path| path.exists())
        .collect())
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    ingest::wal_shm_fingerprint(path)
}

pub fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let mut records = Vec::new();
    for thread in load_threads(path)? {
        records.extend(records_from_thread(&thread, path));
    }
    Ok(records)
}

pub(crate) struct LoadedThread {
    pub id: String,
    pub parent_id: String,
    pub summary: String,
    pub created_at: String,
    pub updated_at: String,
    pub json: Value,
}

pub(crate) fn load_threads(path: &Path) -> Result<Vec<LoadedThread>, String> {
    let db = ingest::open_readonly_uri(path)?;
    load_threads_from(&db, None)
}

pub(crate) fn load_thread(path: &Path, session_id: &str) -> Result<Option<LoadedThread>, String> {
    let db = ingest::open_readonly_uri(path)?;
    Ok(load_threads_from(&db, Some(session_id))?.into_iter().next())
}

fn load_threads_from(
    db: &Connection,
    session_filter: Option<&str>,
) -> Result<Vec<LoadedThread>, String> {
    let columns = table_columns(db, "threads")?;
    if !columns.contains("id") || !columns.contains("data") || !columns.contains("data_type") {
        return Ok(Vec::new());
    }
    let sql = format!(
        "
        SELECT
            {id},
            {summary},
            {updated_at},
            {created_at},
            {parent_id},
            data_type,
            data
        FROM threads
        WHERE (?1 IS NULL OR id = ?1)
        ORDER BY updated_at ASC, id ASC
        ",
        id = sql_text(&columns, "id"),
        summary = sql_text(&columns, "summary"),
        updated_at = sql_text(&columns, "updated_at"),
        created_at = sql_text(&columns, "created_at"),
        parent_id = sql_text(&columns, "parent_id"),
    );
    let mut stmt = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([session_filter], |row| {
            Ok(RawThreadRow {
                id: row.get(0)?,
                summary: row.get(1)?,
                updated_at: row.get(2)?,
                created_at: row.get(3)?,
                parent_id: row.get(4)?,
                data_type: row.get(5)?,
                data: blob_or_text(row, 6)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let mut threads = Vec::new();
    for row in rows {
        let raw = row.map_err(|error| error.to_string())?;
        let Some(json) = decode_thread_blob(&raw.data_type, &raw.data) else {
            continue;
        };
        threads.push(LoadedThread {
            id: raw.id,
            parent_id: raw.parent_id,
            summary: raw.summary,
            created_at: raw.created_at,
            updated_at: raw.updated_at,
            json,
        });
    }
    Ok(threads)
}

struct RawThreadRow {
    id: String,
    summary: String,
    updated_at: String,
    created_at: String,
    parent_id: String,
    data_type: String,
    data: Vec<u8>,
}

pub(crate) fn decode_thread_blob(data_type: &str, data: &[u8]) -> Option<Value> {
    let bytes = match data_type {
        "zstd" => zstd::decode_all(data).ok()?,
        "json" => data.to_vec(),
        _ => return None,
    };
    serde_json::from_slice(&bytes).ok()
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ZedTokens {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
}

impl ZedTokens {
    fn from_usage(usage: &TokenUsage) -> Self {
        Self {
            input_tokens: usage.input_tokens as i64,
            output_tokens: usage.output_tokens as i64,
            cache_creation_tokens: usage.cache_creation_input_tokens as i64,
            cache_read_tokens: usage.cache_read_input_tokens as i64,
        }
    }

    fn saturating_sub(self, other: Self) -> Self {
        Self {
            input_tokens: (self.input_tokens - other.input_tokens).max(0),
            output_tokens: (self.output_tokens - other.output_tokens).max(0),
            cache_creation_tokens: (self.cache_creation_tokens - other.cache_creation_tokens)
                .max(0),
            cache_read_tokens: (self.cache_read_tokens - other.cache_read_tokens).max(0),
        }
    }

    fn saturating_add(&mut self, other: Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
    }

    fn is_empty(self) -> bool {
        self.input_tokens <= 0
            && self.output_tokens <= 0
            && self.cache_creation_tokens <= 0
            && self.cache_read_tokens <= 0
    }
}

#[derive(Debug, Default, Deserialize)]
struct TokenUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

#[derive(Default, Deserialize)]
struct ThreadUsage {
    #[serde(default)]
    request_token_usage: BTreeMap<String, TokenUsage>,
    #[serde(default)]
    cumulative_token_usage: TokenUsage,
}

pub(crate) fn usage_entries(thread: &Value) -> Vec<(String, ZedTokens)> {
    let parsed: ThreadUsage = serde_json::from_value(thread.clone()).unwrap_or_default();
    let mut entries = Vec::new();
    let mut summed = ZedTokens::default();
    for (key, usage) in parsed.request_token_usage {
        let tokens = ZedTokens::from_usage(&usage);
        if tokens.is_empty() {
            continue;
        }
        summed.saturating_add(tokens);
        entries.push((key, tokens));
    }
    let remainder = ZedTokens::from_usage(&parsed.cumulative_token_usage).saturating_sub(summed);
    if !remainder.is_empty() {
        entries.push(("cumulative-remainder".to_string(), remainder));
    }
    entries
}

fn records_from_thread(thread: &LoadedThread, path: &Path) -> Vec<UsageRecord> {
    if !thread.parent_id.trim().is_empty() {
        return Vec::new();
    }
    let fallback_at = parse_sql_timestamp(&thread.updated_at);
    if fallback_at.is_empty() {
        return Vec::new();
    }
    let request_times = request_timestamps(&thread.json);
    let model = model_from_thread(&thread.json);
    let provider = model_provider(&thread.json);
    let project = project_from_thread(&thread.json);
    let source_file = path.to_string_lossy().into_owned();
    let mut records = Vec::new();
    for (request_key, usage) in usage_entries(&thread.json) {
        if usage.is_empty() {
            continue;
        }
        let occurred_at = request_times
            .get(&request_key)
            .cloned()
            .unwrap_or_else(|| fallback_at.clone());
        let record = finish(UsageRecord {
            occurred_at,
            source: Source::Zed,
            model: model.clone(),
            provider: provider.clone(),
            project: project.clone(),
            session_id: thread.id.clone(),
            source_file: source_file.clone(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_creation_tokens: usage.cache_creation_tokens,
            cache_creation_1h_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            native_cost: None,
        });
        if has_billable_tokens(&record) {
            records.push(record);
        }
    }
    records
}

fn request_timestamps(thread: &Value) -> BTreeMap<String, String> {
    let mut times = BTreeMap::new();
    let Some(messages) = thread.get("messages").and_then(Value::as_array) else {
        return times;
    };
    for message in messages {
        let inner = message
            .get("User")
            .or_else(|| message.get("Agent"))
            .or_else(|| message.get("Resume"));
        let Some(inner) = inner else {
            continue;
        };
        let Some(id) = inner.get("id").and_then(Value::as_str) else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        if let Some(ts) = message_timestamp(inner).or_else(|| message_timestamp(message)) {
            times.insert(id.to_string(), ts);
        }
    }
    times
}

fn message_timestamp(value: &Value) -> Option<String> {
    for key in ["created_at", "updated_at", "timestamp"] {
        if let Some(raw) = value.get(key).and_then(Value::as_str) {
            let ts = parse_sql_timestamp(raw);
            if !ts.is_empty() {
                return Some(ts);
            }
        }
    }
    None
}

pub(crate) fn model_from_thread(thread: &Value) -> String {
    let Some(model) = thread.get("model") else {
        return String::new();
    };
    for key in ["model", "id", "name"] {
        if let Some(name) = model.get(key).and_then(Value::as_str) {
            let name = name.trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    model
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn model_provider(thread: &Value) -> String {
    thread
        .get("model")
        .and_then(|model| model.get("provider"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

pub(crate) fn project_from_thread(thread: &Value) -> String {
    first_path(thread.get("initial_project_snapshot")).unwrap_or_default()
}

fn first_path(value: Option<&Value>) -> Option<String> {
    let value = value?;
    match value {
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        }
        Value::Object(map) => {
            for key in ["abs_path", "worktree_root", "root_path", "path", "cwd"] {
                if let Some(found) = first_path(map.get(key)) {
                    return Some(found);
                }
            }
            for key in ["worktrees", "folders"] {
                if let Some(found) = first_path(map.get(key)) {
                    return Some(found);
                }
            }
            map.values().find_map(|item| first_path(Some(item)))
        }
        Value::Array(items) => items.iter().find_map(|item| first_path(Some(item))),
        _ => None,
    }
}

fn blob_or_text(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Vec<u8>> {
    match row.get_ref(idx)? {
        rusqlite::types::ValueRef::Blob(bytes) | rusqlite::types::ValueRef::Text(bytes) => {
            Ok(bytes.to_vec())
        }
        rusqlite::types::ValueRef::Null => Ok(Vec::new()),
        value => Err(rusqlite::Error::InvalidColumnType(
            idx,
            "data".to_string(),
            value.data_type(),
        )),
    }
}

fn sql_text(columns: &BTreeSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("COALESCE({name}, '')")
    } else {
        "''".to_string()
    }
}
