//! 推送的消耗记录筛选：`occurred_at` 落在区间内（两端都含），可按来源筛。
//!
//! 已归档（源文件已消失）的行与未归档的一视同仁，和所有聚合口径一致（ADR 0004）。

use rusqlite::{params_from_iter, Connection};

use super::PushRange;
use crate::domain::{Source, UsageRecord};

pub fn load_usage_in_range(
    conn: &Connection,
    range: &PushRange,
) -> Result<Vec<UsageRecord>, String> {
    let mut clauses = Vec::new();
    let mut params: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(from) = range.from.as_deref().filter(|value| !value.is_empty()) {
        clauses.push("occurred_at >= ?".to_string());
        params.push(from.to_string().into());
    }
    if let Some(to) = range.to.as_deref().filter(|value| !value.is_empty()) {
        clauses.push("occurred_at <= ?".to_string());
        params.push(to.to_string().into());
    }
    if !range.sources.is_empty() {
        clauses.push(format!(
            "source IN ({})",
            vec!["?"; range.sources.len()].join(", ")
        ));
        params.extend(range.sources.iter().cloned().map(Into::into));
    }
    let filter = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    let sql = format!(
        r#"
        SELECT occurred_at, source, model, provider, project, session_id, source_file,
               input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
               reasoning_tokens, total_tokens, native_cost
        FROM usage_records
        {filter}
        ORDER BY occurred_at ASC, source_file ASC, rowid ASC
        "#
    );
    let mut statement = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(params_from_iter(params.iter()), |row| {
            let source_value: String = row.get(1)?;
            let Some(source) = Source::parse(&source_value) else {
                return Ok(None);
            };
            Ok(Some(UsageRecord {
                occurred_at: row.get(0)?,
                source,
                model: row.get(2)?,
                provider: row.get(3)?,
                project: row.get(4)?,
                session_id: row.get(5)?,
                source_file: row.get(6)?,
                input_tokens: row.get(7)?,
                output_tokens: row.get(8)?,
                cache_read_tokens: row.get(9)?,
                cache_creation_tokens: row.get(10)?,
                reasoning_tokens: row.get(11)?,
                total_tokens: row.get(12)?,
                native_cost: row.get(13)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    Ok(rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .into_iter()
        .flatten()
        .collect())
}
