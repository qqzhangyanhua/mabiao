use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{finish, has_billable_tokens, i64_field, text_field};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// Warp 把 Agent 会话用量写进本机 `warp.sqlite` 的 `agent_conversations.conversation_data`。
///
/// 默认扫：
/// - Linux：`~/.local/state/warp-terminal`
/// - macOS Stable / Preview：Group Container `2BBY89MBSN.dev.warp` 下的 Application Support
/// - Windows：`%LOCALAPPDATA%/warp/Warp/data`
///
/// `WARP_HOME` 整体替换，指向含 `warp.sqlite` 的目录（也可以是该文件本身）。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides.get("WARP_HOME").cloned().unwrap_or_else(|| {
        vec![
            home.join(".local/state/warp-terminal"),
            home.join("Library/Group Containers/2BBY89MBSN.dev.warp/Library/Application Support/dev.warp.Warp-Stable"),
            home.join("Library/Group Containers/2BBY89MBSN.dev.warp/Library/Application Support/dev.warp.Warp-Preview"),
            home.join("AppData/Local/warp/Warp/data"),
        ]
    })
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        if root.is_file() {
            if is_warp_sqlite(root) && !paths.contains(root) {
                paths.push(root.clone());
            }
            continue;
        }
        let candidate = root.join("warp.sqlite");
        if candidate.is_file() && !paths.contains(&candidate) {
            paths.push(candidate);
        }
    }
    Ok(paths)
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    ingest::wal_shm_fingerprint(path)
}

pub fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let db = ingest::open_readonly_uri(path)?;
    let columns = table_columns(&db, "agent_conversations")?;
    if !columns.contains("conversation_id") || !columns.contains("conversation_data") {
        return Ok(Vec::new());
    }
    let sql = format!(
        "
        SELECT
            {conversation_id},
            {conversation_data},
            {last_modified_at}
        FROM agent_conversations
        ",
        conversation_id = sql_text(&columns, "conversation_id"),
        conversation_data = sql_text(&columns, "conversation_data"),
        last_modified_at = sql_text(&columns, "last_modified_at"),
    );
    let mut stmt = db.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    let source_file = path.to_string_lossy().into_owned();
    let mut records = Vec::new();
    for row in rows {
        let (session_id, data, last_modified_at) = row.map_err(|error| error.to_string())?;
        records.extend(parse_conversation(
            &session_id,
            &data,
            &last_modified_at,
            &source_file,
        ));
    }
    Ok(records)
}

fn parse_conversation(
    session_id: &str,
    data: &str,
    last_modified_at: &str,
    source_file: &str,
) -> Vec<UsageRecord> {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };
    let Some(metadata) = value.get("conversation_usage_metadata") else {
        return Vec::new();
    };
    let Some(token_usage) = metadata.get("token_usage").and_then(Value::as_array) else {
        return Vec::new();
    };
    let occurred_at = parse_sql_timestamp(last_modified_at);
    token_usage
        .iter()
        .filter_map(|entry| parse_model_usage(entry, session_id, &occurred_at, source_file))
        .collect()
}

fn parse_model_usage(
    entry: &Value,
    session_id: &str,
    occurred_at: &str,
    source_file: &str,
) -> Option<UsageRecord> {
    let model = text_field(entry, &["model_id"]);
    if model.is_empty() {
        return None;
    }
    let warp = warp_tokens(entry);
    let byok = i64_field(entry, &["byok_tokens"]).max(0);
    let custom = i64_field(entry, &["custom_endpoint_tokens"]).max(0);
    let total = warp + byok + custom;
    if total <= 0 {
        return None;
    }
    let record = finish(UsageRecord {
        occurred_at: occurred_at.to_string(),
        source: Source::Warp,
        model,
        provider: provider_of(warp, byok, custom),
        project: String::new(),
        session_id: session_id.to_string(),
        source_file: source_file.to_string(),
        // Warp 只给按模型的总量，没有 input/output/cache/reasoning 拆分。
        // 未拆分的总量必须进可计费字段，否则 has_billable_tokens 会丢掉。
        input_tokens: total,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        cache_creation_1h_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: total,
        native_cost: None,
    });
    has_billable_tokens(&record).then_some(record)
}

fn warp_tokens(entry: &Value) -> i64 {
    if entry.get("warp_tokens").is_some() {
        i64_field(entry, &["warp_tokens"]).max(0)
    } else {
        i64_field(entry, &["total_tokens"]).max(0)
    }
}

fn provider_of(warp: i64, byok: i64, custom: i64) -> String {
    if custom > warp && custom > byok {
        "custom".to_string()
    } else if byok > warp {
        "byok".to_string()
    } else {
        "warp".to_string()
    }
}

fn is_warp_sqlite(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("warp.sqlite")
}

fn parse_sql_timestamp(raw: &str) -> String {
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

fn sql_text(columns: &BTreeSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("COALESCE({name}, '')")
    } else {
        "''".to_string()
    }
}
