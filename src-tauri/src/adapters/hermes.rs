use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::OpenFlags;

use crate::adapters::finish;
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "HERMES_HOME", ".hermes", "")
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(roots
        .iter()
        .map(|root| root.join("state.db"))
        .filter(|path| path.exists())
        .collect())
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    format!(
        "{}|{}",
        ingest::metadata_fingerprint(&sidecar_path(path, "-wal")),
        ingest::metadata_fingerprint(&sidecar_path(path, "-shm"))
    )
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let source_db = open_readonly(path)?;
    let usage_cols = table_columns(&source_db, "session_model_usage")?;
    let session_cols = table_columns(&source_db, "sessions")?;
    let sql = format!(
        "
            SELECT
                {session_id},
                {model},
                {provider},
                {input_tokens},
                {output_tokens},
                {cache_read_tokens},
                {cache_write_tokens},
                {reasoning_tokens},
                {actual_cost_usd},
                {cost_source},
                {started_at},
                {cwd},
                {git_repo_root}
            FROM session_model_usage AS u
            JOIN sessions AS s ON s.id = u.session_id
            ",
        session_id = sql_coalesce(&usage_cols, "u", "session_id", "''"),
        model = sql_coalesce(&usage_cols, "u", "model", "''"),
        provider = sql_coalesce(&usage_cols, "u", "billing_provider", "''"),
        input_tokens = sql_coalesce(&usage_cols, "u", "input_tokens", "0"),
        output_tokens = sql_coalesce(&usage_cols, "u", "output_tokens", "0"),
        cache_read_tokens = sql_coalesce(&usage_cols, "u", "cache_read_tokens", "0"),
        cache_write_tokens = sql_coalesce(&usage_cols, "u", "cache_write_tokens", "0"),
        reasoning_tokens = sql_coalesce(&usage_cols, "u", "reasoning_tokens", "0"),
        actual_cost_usd = sql_coalesce(&usage_cols, "u", "actual_cost_usd", "0"),
        cost_source = sql_optional(&usage_cols, "u", "cost_source"),
        started_at = sql_optional(&session_cols, "s", "started_at"),
        cwd = sql_coalesce(&session_cols, "s", "cwd", "''"),
        git_repo_root = sql_coalesce(&session_cols, "s", "git_repo_root", "''"),
    );
    let mut stmt = source_db.prepare(&sql).map_err(|error| error.to_string())?;
    let source_file = path.to_string_lossy().into_owned();
    let rows = stmt
        .query_map([], |row| {
            Ok(ParsedUsage {
                session_id: row.get(0)?,
                model: row.get(1)?,
                provider: row.get(2)?,
                input_tokens: row.get(3)?,
                output_tokens: row.get(4)?,
                cache_read_tokens: row.get(5)?,
                cache_write_tokens: row.get(6)?,
                reasoning_tokens: row.get(7)?,
                actual_cost_usd: row.get(8)?,
                cost_source: row.get(9)?,
                started_at: row.get(10)?,
                cwd: row.get(11)?,
                git_repo_root: row.get(12)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let mut records = Vec::new();
    for row in rows {
        records.push(
            row.map_err(|error| error.to_string())?
                .into_record(&source_file),
        );
    }
    Ok(records)
}

struct ParsedUsage {
    session_id: String,
    model: String,
    provider: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
    actual_cost_usd: f64,
    cost_source: Option<String>,
    started_at: Option<f64>,
    cwd: String,
    git_repo_root: String,
}

impl ParsedUsage {
    fn into_record(self, source_file: &str) -> UsageRecord {
        finish(UsageRecord {
            occurred_at: unix_seconds_to_rfc3339(self.started_at.unwrap_or(0.0)),
            source: Source::Hermes,
            model: self.model,
            provider: self.provider,
            project: project_path(&self.cwd, &self.git_repo_root),
            session_id: self.session_id,
            source_file: source_file.to_string(),
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_creation_tokens: self.cache_write_tokens,
            reasoning_tokens: self.reasoning_tokens,
            total_tokens: 0,
            native_cost: native_cost(self.cost_source.as_deref(), self.actual_cost_usd),
        })
    }
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

fn sql_optional(columns: &BTreeSet<String>, alias: &str, name: &str) -> String {
    if columns.contains(name) {
        format!("{alias}.{name}")
    } else {
        "NULL".to_string()
    }
}

fn project_path(cwd: &str, git_repo_root: &str) -> String {
    if !cwd.is_empty() {
        cwd.to_string()
    } else {
        git_repo_root.to_string()
    }
}

fn native_cost(cost_source: Option<&str>, actual_cost_usd: f64) -> Option<f64> {
    let source = cost_source.unwrap_or("");
    if source.is_empty() || source == "none" || actual_cost_usd <= 0.0 {
        None
    } else {
        Some(actual_cost_usd)
    }
}

fn unix_seconds_to_rfc3339(started_at: f64) -> String {
    if !started_at.is_finite() || started_at <= 0.0 {
        return String::new();
    }
    let secs = started_at.trunc() as i64;
    let nanos = ((started_at.fract()) * 1_000_000_000.0).round() as i64;
    let (secs, nanos) = if nanos >= 1_000_000_000 {
        (secs.saturating_add(1), 0)
    } else if nanos < 0 {
        (secs, 0)
    } else {
        (secs, nanos)
    };
    chrono::DateTime::from_timestamp(secs, nanos as u32)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.to_string_lossy()))
}

fn open_readonly(path: &Path) -> Result<rusqlite::Connection, String> {
    let uri = sqlite_readonly_uri(path);
    let connection = rusqlite::Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn sqlite_readonly_uri(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file:");
    for ch in raw.chars() {
        match ch {
            ' ' => uri.push_str("%20"),
            '?' => uri.push_str("%3F"),
            '#' => uri.push_str("%23"),
            c => uri.push(c),
        }
    }
    uri.push_str("?mode=ro");
    uri
}
