use rusqlite::Connection;

use crate::domain::{FilterOptions, InstructionSourceUsage, InstructionUsageSummary};

use super::sql::*;

pub fn filter_options(conn: &Connection) -> Result<FilterOptions, String> {
    Ok(FilterOptions {
        // source 不过滤空串：它是枚举落库，不该有空值，真出现了也要能在筛选里看见。
        sources: distinct_values(conn, "source", false)?,
        models: distinct_values(conn, "model", true)?,
        projects: distinct_values(conn, "project", true)?,
        providers: distinct_values(conn, "provider", true)?,
    })
}

pub fn recent_projects(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT project FROM usage_records
             WHERE project != ''
             GROUP BY project
             ORDER BY MAX(occurred_at) DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn source_token_totals(conn: &Connection) -> Result<InstructionUsageSummary, String> {
    let mut stmt = conn
        .prepare(
            "SELECT source, SUM(total_tokens) FROM usage_records
             GROUP BY source
             ORDER BY SUM(total_tokens) DESC, source ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(InstructionSourceUsage {
                source: row.get(0)?,
                total_tokens: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(InstructionUsageSummary {
        sources: rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?,
    })
}
