use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::adapters::{finish, has_billable_tokens};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// Alma 桌面端的 Electron `userData`：OS 配置目录下的 `alma/`，
/// **不是** XDG 数据目录 `~/.local/share/alma`。
///
/// - Linux：`~/.config/alma`
/// - macOS：`~/Library/Application Support/alma`
/// - Windows：`%APPDATA%\alma`
///
/// 默认三个平台根都列出来，Linux CI 的 tempfile home 也能落到 `.config/alma`。
/// `ALMA_HOME` 整体替换默认根，指向数据目录（其下的 `chat_threads.db`）。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides.get("ALMA_HOME").cloned().unwrap_or_else(|| {
        vec![
            home.join(".config/alma"),
            home.join("Library/Application Support/alma"),
            home.join("AppData/Roaming/alma"),
        ]
    })
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(roots
        .iter()
        .map(|root| root.join("chat_threads.db"))
        .filter(|path| path.exists())
        .collect())
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    ingest::wal_shm_fingerprint(path)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let db = ingest::open_readonly_uri(path)?;
    let threads = table_columns(&db, "chat_threads")?;
    if !threads.contains("id") {
        return Ok(Vec::new());
    }
    let workspaces = table_columns(&db, "workspaces")?;
    let usage = table_columns(&db, "usage_records")?;
    let aux = table_columns(&db, "aux_usage_records")?;
    let source_file = path.to_string_lossy().into_owned();
    let mut records = Vec::new();
    for cols in [&usage, &aux] {
        if !cols.contains("thread_id") {
            continue;
        }
        records.extend(query_usage(&db, cols, &threads, &workspaces, &source_file)?);
    }
    Ok(records)
}

fn query_usage(
    db: &rusqlite::Connection,
    usage_cols: &BTreeSet<String>,
    thread_cols: &BTreeSet<String>,
    workspace_cols: &BTreeSet<String>,
    source_file: &str,
) -> Result<Vec<UsageRecord>, String> {
    let table = if usage_cols.contains("purpose") {
        "aux_usage_records"
    } else {
        "usage_records"
    };
    let cwd = if thread_cols.contains("workspace_id")
        && workspace_cols.contains("id")
        && workspace_cols.contains("path")
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
    let sql = format!(
        "
        SELECT
            {thread_id},
            {model},
            {provider},
            {timestamp},
            {input_tokens},
            {cached_input_tokens},
            {cache_write_input_tokens},
            {output_tokens},
            {cwd}
        FROM {table} AS u
        JOIN chat_threads AS t ON t.id = u.thread_id
        {join}
        WHERE {skip}
        ",
        thread_id = sql_coalesce(usage_cols, "u", "thread_id", "''"),
        model = sql_coalesce(usage_cols, "u", "model", "''"),
        provider = sql_coalesce(usage_cols, "u", "provider_id", "''"),
        timestamp = sql_coalesce(usage_cols, "u", "timestamp", "''"),
        input_tokens = sql_coalesce(usage_cols, "u", "input_tokens", "0"),
        cached_input_tokens = sql_coalesce(usage_cols, "u", "cached_input_tokens", "0"),
        cache_write_input_tokens = sql_coalesce(usage_cols, "u", "cache_write_input_tokens", "0"),
        output_tokens = sql_coalesce(usage_cols, "u", "output_tokens", "0"),
        skip = skip_thread_sql(thread_cols),
    );
    let mut stmt = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ParsedUsage {
                thread_id: row.get(0)?,
                model: row.get(1)?,
                provider: row.get(2)?,
                timestamp: row.get(3)?,
                input_tokens: row.get(4)?,
                cached_input_tokens: row.get(5)?,
                cache_write_input_tokens: row.get(6)?,
                output_tokens: row.get(7)?,
                project: row.get(8)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let mut records = Vec::new();
    for row in rows {
        let parsed = row.map_err(|error| error.to_string())?;
        if let Some(record) = parsed.into_record(source_file) {
            records.push(record);
        }
    }
    Ok(records)
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

struct ParsedUsage {
    thread_id: String,
    model: String,
    provider: String,
    timestamp: String,
    input_tokens: i64,
    cached_input_tokens: i64,
    cache_write_input_tokens: i64,
    output_tokens: i64,
    project: String,
}

impl ParsedUsage {
    fn into_record(self, source_file: &str) -> Option<UsageRecord> {
        let record = finish(UsageRecord {
            occurred_at: self.timestamp,
            source: Source::Alma,
            model: alma_model(&self.model, &self.provider),
            provider: self.provider,
            project: self.project,
            session_id: self.thread_id,
            source_file: source_file.to_string(),
            input_tokens: (self.input_tokens
                - self.cached_input_tokens
                - self.cache_write_input_tokens)
                .max(0),
            output_tokens: self.output_tokens,
            cache_read_tokens: self.cached_input_tokens,
            cache_creation_tokens: self.cache_write_input_tokens,
            reasoning_tokens: 0,
            total_tokens: 0,
            native_cost: None,
        });
        has_billable_tokens(&record).then_some(record)
    }
}

/// Alma 把模型写成 `<provider>:<model>`，插件是 `plugin:<plugin>:<provider>:<model>`。
pub fn alma_model(model: &str, provider_id: &str) -> String {
    let model = model.trim();
    let provider = if provider_id.is_empty() {
        model.split_once(':').map(|(head, _)| head).unwrap_or("")
    } else {
        provider_id
    };
    let Some(rest) = model.strip_prefix(&format!("{provider}:")) else {
        return model.to_string();
    };
    if provider == "plugin" {
        let parts: Vec<&str> = rest.splitn(3, ':').collect();
        if parts.len() == 3 && !parts[2].is_empty() {
            return parts[2].to_string();
        }
    }
    if rest.is_empty() {
        return model.to_string();
    }
    rest.to_string()
}

fn table_columns(conn: &rusqlite::Connection, table: &str) -> Result<BTreeSet<String>, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?;
    let mut names = BTreeSet::new();
    for name in rows {
        names.insert(name.map_err(|error| error.to_string())?);
    }
    Ok(names)
}

fn sql_coalesce(columns: &BTreeSet<String>, alias: &str, name: &str, fallback: &str) -> String {
    if columns.contains(name) {
        format!("COALESCE({alias}.{name}, {fallback})")
    } else {
        fallback.to_string()
    }
}
