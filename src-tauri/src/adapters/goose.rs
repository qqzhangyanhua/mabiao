use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::adapters::sqlite::{sql_int, sql_text};
use crate::adapters::{finish, has_billable_tokens};

pub(crate) use crate::adapters::sqlite::table_columns;
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// Goose 把 CLI / 桌面会话写进一份 SQLite。
///
/// `$GOOSE_PATH_ROOT` 覆盖根目录，扫 `data/sessions/sessions.db`。
/// 未覆盖时扫 aaif 默认目录与 Block 旧路径。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides
        .get("GOOSE_PATH_ROOT")
        .cloned()
        .map(|roots| {
            roots
                .into_iter()
                .map(|root| root.join("data/sessions"))
                .collect()
        })
        .unwrap_or_else(|| {
            vec![
                home.join(".local/share/goose/sessions"),
                home.join("Library/Application Support/goose/sessions"),
                home.join("Library/Application Support/Block/goose/sessions"),
                home.join(".local/share/Block/goose/sessions"),
                home.join("AppData/Roaming/Block/goose/data/sessions"),
            ]
        })
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(roots
        .iter()
        .map(|root| root.join("sessions.db"))
        .filter(|path| path.exists())
        .collect())
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    ingest::wal_shm_fingerprint(path)
}

pub fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let db = ingest::open_readonly_uri(path)?;
    let columns = table_columns(&db, "sessions")?;
    if !columns.contains("id") {
        return Ok(Vec::new());
    }
    let sql = format!(
        "
        SELECT
            {id},
            {working_dir},
            {created_at},
            {updated_at},
            {input_tokens},
            {output_tokens},
            {total_tokens},
            {accumulated_input},
            {accumulated_output},
            {accumulated_total},
            {provider},
            {model_config},
            {session_type}
        FROM sessions
        ",
        id = sql_text(&columns, "id"),
        working_dir = sql_text(&columns, "working_dir"),
        created_at = sql_text(&columns, "created_at"),
        updated_at = sql_text(&columns, "updated_at"),
        input_tokens = sql_int(&columns, "input_tokens"),
        output_tokens = sql_int(&columns, "output_tokens"),
        total_tokens = sql_int(&columns, "total_tokens"),
        accumulated_input = sql_int(&columns, "accumulated_input_tokens"),
        accumulated_output = sql_int(&columns, "accumulated_output_tokens"),
        accumulated_total = sql_int(&columns, "accumulated_total_tokens"),
        provider = sql_text(&columns, "provider_name"),
        model_config = sql_text(&columns, "model_config_json"),
        session_type = sql_text(&columns, "session_type"),
    );
    let mut stmt = db.prepare(&sql).map_err(|error| error.to_string())?;
    let source_file = path.to_string_lossy().into_owned();
    let rows = stmt
        .query_map([], |row| {
            Ok(ParsedSession {
                session_id: row.get(0)?,
                project: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                input_tokens: row.get(4)?,
                output_tokens: row.get(5)?,
                total_tokens: row.get(6)?,
                accumulated_input: row.get(7)?,
                accumulated_output: row.get(8)?,
                accumulated_total: row.get(9)?,
                provider: row.get(10)?,
                model_config: row.get(11)?,
                session_type: row.get(12)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let mut records = Vec::new();
    for row in rows {
        let parsed = row.map_err(|error| error.to_string())?;
        if let Some(record) = parsed.into_record(&source_file) {
            records.push(record);
        }
    }
    Ok(records)
}

struct ParsedSession {
    session_id: String,
    project: String,
    created_at: String,
    updated_at: String,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    accumulated_input: i64,
    accumulated_output: i64,
    accumulated_total: i64,
    provider: String,
    model_config: String,
    session_type: String,
}

impl ParsedSession {
    fn into_record(self, source_file: &str) -> Option<UsageRecord> {
        if skip_usage_session(&self.session_type) {
            return None;
        }
        let model = parse_model_name(&self.model_config).unwrap_or_default();
        let input = prefer_positive(self.accumulated_input, self.input_tokens);
        let output = prefer_positive(self.accumulated_output, self.output_tokens);
        let total = prefer_positive(self.accumulated_total, self.total_tokens);
        let occurred_at = parse_sql_timestamp(if self.updated_at.trim().is_empty() {
            &self.created_at
        } else {
            &self.updated_at
        });
        let record = finish(UsageRecord {
            occurred_at,
            source: Source::Goose,
            model,
            provider: self.provider,
            project: self.project,
            session_id: self.session_id,
            source_file: source_file.to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            cache_creation_1h_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: total,
            native_cost: None,
        });
        if !has_billable_tokens(&record) && record.total_tokens <= 0 {
            None
        } else {
            Some(record)
        }
    }
}

#[derive(Deserialize)]
struct GooseModelConfig {
    model_name: Option<String>,
}

pub(crate) fn parse_model_name(json: &str) -> Option<String> {
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return None;
    }
    let config: GooseModelConfig = serde_json::from_str(trimmed).ok()?;
    let name = config.model_name?.trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

pub(crate) fn skip_usage_session(session_type: &str) -> bool {
    matches!(session_type.trim().to_ascii_lowercase().as_str(), "hidden")
}

pub(crate) fn is_catalog_session(session_type: &str) -> bool {
    !matches!(
        session_type.trim().to_ascii_lowercase().as_str(),
        "hidden" | "subagent"
    )
}

pub(crate) fn parse_sql_timestamp(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return dt.with_timezone(&chrono::Utc).to_rfc3339();
    }
    for format in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d"] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(raw, format) {
            return dt.and_utc().to_rfc3339();
        }
        if format == "%Y-%m-%d" {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(raw, format) {
                if let Some(dt) = date.and_hms_opt(0, 0, 0) {
                    return dt.and_utc().to_rfc3339();
                }
            }
        }
    }
    String::new()
}

pub(crate) fn unix_seconds_to_rfc3339(started_at: i64) -> String {
    if started_at <= 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp(started_at, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

fn prefer_positive(primary: i64, fallback: i64) -> i64 {
    if primary > 0 {
        primary
    } else {
        fallback.max(0)
    }
}
