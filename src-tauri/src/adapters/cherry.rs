use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::adapters::goose::unix_seconds_to_rfc3339;
use crate::adapters::sqlite::{sql_text, table_columns};
use crate::adapters::{finish, has_billable_tokens};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// Cherry Studio v2 把用量写进 Electron userData 下的 `cherrystudio.sqlite`。
///
/// `$CHERRY_STUDIO_DATA_DIR` 覆盖 userData 根。未覆盖时扫 Linux `.config`、
/// macOS Application Support、Windows AppData，目录名同时覆盖
/// `CherryStudio`（package.json `name`）与 `Cherry Studio`（productName）。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides
        .get("CHERRY_STUDIO_DATA_DIR")
        .cloned()
        .unwrap_or_else(|| {
            let names = ["CherryStudio", "Cherry Studio"];
            let mut dirs = Vec::new();
            for name in names {
                dirs.push(home.join(".config").join(name));
                dirs.push(home.join("Library/Application Support").join(name));
                dirs.push(home.join("AppData/Roaming").join(name));
            }
            dirs
        })
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(roots
        .iter()
        .map(|root| root.join("cherrystudio.sqlite"))
        .filter(|path| path.exists())
        .collect())
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    ingest::wal_shm_fingerprint(path)
}

pub fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let db = ingest::open_readonly_uri(path)?;
    let columns = table_columns(&db, "ai_usage_record")?;
    if !columns.contains("id") {
        return Ok(Vec::new());
    }
    let sql = format!(
        "
        SELECT
            {request_id},
            {message_id},
            {provider_id},
            {provider_name},
            {model_id},
            {model_name},
            {source_name},
            {input_tokens},
            {output_tokens},
            {reasoning_tokens},
            {no_cache_tokens},
            {cache_read_tokens},
            {cache_write_tokens},
            {cost},
            {cost_currency},
            {created_at}
        FROM ai_usage_record
        ORDER BY created_at ASC, id ASC
        ",
        request_id = sql_text(&columns, "request_id"),
        message_id = sql_text(&columns, "message_id"),
        provider_id = sql_text(&columns, "provider_id"),
        provider_name = sql_text(&columns, "provider_name"),
        model_id = sql_text(&columns, "model_id"),
        model_name = sql_text(&columns, "model_name"),
        source_name = sql_text(&columns, "source_name"),
        input_tokens = sql_opt_int(&columns, "input_tokens"),
        output_tokens = sql_opt_int(&columns, "output_tokens"),
        reasoning_tokens = sql_opt_int(&columns, "reasoning_tokens"),
        no_cache_tokens = sql_opt_int(&columns, "no_cache_tokens"),
        cache_read_tokens = sql_opt_int(&columns, "cache_read_tokens"),
        cache_write_tokens = sql_opt_int(&columns, "cache_write_tokens"),
        cost = sql_opt_real(&columns, "cost"),
        cost_currency = sql_text(&columns, "cost_currency"),
        created_at = if columns.contains("created_at") {
            "COALESCE(created_at, 0)"
        } else {
            "0"
        },
    );
    let mut stmt = db.prepare(&sql).map_err(|error| error.to_string())?;
    let source_file = path.to_string_lossy().into_owned();
    let rows = stmt
        .query_map([], |row| {
            Ok(ParsedUsage {
                request_id: row.get(0)?,
                message_id: row.get(1)?,
                provider_id: row.get(2)?,
                provider_name: row.get(3)?,
                model_id: row.get(4)?,
                model_name: row.get(5)?,
                source_name: row.get(6)?,
                input_tokens: row.get(7)?,
                output_tokens: row.get(8)?,
                reasoning_tokens: row.get(9)?,
                no_cache_tokens: row.get(10)?,
                cache_read_tokens: row.get(11)?,
                cache_write_tokens: row.get(12)?,
                cost: row.get(13)?,
                cost_currency: row.get(14)?,
                created_at: row.get(15)?,
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

struct ParsedUsage {
    request_id: String,
    message_id: String,
    provider_id: String,
    provider_name: String,
    model_id: String,
    model_name: String,
    source_name: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    no_cache_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    cost: Option<f64>,
    cost_currency: String,
    created_at: i64,
}

impl ParsedUsage {
    fn into_record(self, source_file: &str) -> Option<UsageRecord> {
        let model = first_nonempty(&[&self.model_id, &self.model_name]);
        if model.is_empty() {
            return None;
        }
        let cache_read = self.cache_read_tokens.unwrap_or(0).max(0);
        let cache_creation = self.cache_write_tokens.unwrap_or(0).max(0);
        let input = match self.no_cache_tokens {
            Some(value) => value.max(0),
            None => {
                let raw = self.input_tokens.unwrap_or(0).max(0);
                (raw - cache_read - cache_creation).max(0)
            }
        };
        let output = self.output_tokens.unwrap_or(0).max(0);
        let reasoning = self.reasoning_tokens.unwrap_or(0).max(0);
        let occurred_at = millis_to_rfc3339(self.created_at);
        if occurred_at.is_empty() {
            return None;
        }
        let session_id = first_nonempty(&[&self.message_id, &self.request_id]);
        let native_cost = usd_cost(self.cost, &self.cost_currency);
        let record = finish(UsageRecord {
            occurred_at,
            source: Source::Cherry,
            model,
            provider: first_nonempty(&[&self.provider_id, &self.provider_name]),
            project: self.source_name.trim().to_string(),
            session_id,
            source_file: source_file.to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_creation,
            cache_creation_1h_tokens: 0,
            reasoning_tokens: reasoning,
            total_tokens: 0,
            native_cost,
        });
        if !has_billable_tokens(&record) {
            None
        } else {
            Some(record)
        }
    }
}

pub(crate) fn millis_to_rfc3339(millis: i64) -> String {
    if millis <= 0 {
        return String::new();
    }
    unix_seconds_to_rfc3339(millis.div_euclid(1000))
}

fn usd_cost(cost: Option<f64>, currency: &str) -> Option<f64> {
    let cost = cost.filter(|value| value.is_finite() && *value > 0.0)?;
    if currency.trim().eq_ignore_ascii_case("usd") {
        Some(cost)
    } else {
        None
    }
}

fn first_nonempty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("")
        .to_string()
}

fn sql_opt_int(columns: &BTreeSet<String>, name: &str) -> String {
    if columns.contains(name) {
        name.to_string()
    } else {
        "NULL".to_string()
    }
}

fn sql_opt_real(columns: &BTreeSet<String>, name: &str) -> String {
    if columns.contains(name) {
        name.to_string()
    } else {
        "NULL".to_string()
    }
}
