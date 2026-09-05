use std::path::PathBuf;

use rusqlite::{params_from_iter, Connection};

use crate::domain::{
    ConversationEvent, ConversationPage, ConversationQuery, ConversationSessionRow,
    ConversationUsagePage, PriceTable, Source,
};
use crate::query;

use super::catalog_search;
use super::conversation_adapter;
use super::event_index;
use super::hydrate_cursor_hash_models;
use super::session_store::{load_session_files, load_usage_records, row_from_sql};
use super::CONVERSATION_SOURCES;
use super::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE};

pub(crate) fn conversation_source_paths(
    source: Source,
    roots: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    (conversation_adapter(source)?.discover)(roots)
}

pub(crate) fn sql_placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn catalog_session_start_sql() -> &'static str {
    "COALESCE(NULLIF(sessions.started_at, ''), (SELECT cs.first_seen_at FROM cursor_sessions cs WHERE sessions.source = 'cursor_agent' AND cs.session_id = sessions.session_id LIMIT 1), '')"
}

pub(crate) fn catalog_session_end_sql() -> &'static str {
    "COALESCE(NULLIF(sessions.ended_at, ''), (SELECT COALESCE(NULLIF(cs.last_seen_at, ''), cs.first_seen_at) FROM cursor_sessions cs WHERE sessions.source = 'cursor_agent' AND cs.session_id = sessions.session_id LIMIT 1), '')"
}

pub(crate) fn push_in_filter(
    clauses: &mut Vec<String>,
    params: &mut Vec<rusqlite::types::Value>,
    column_sql: &str,
    values: &[String],
) {
    if values.is_empty() {
        return;
    }
    clauses.push(format!(
        "{column_sql} IN ({})",
        sql_placeholders(values.len())
    ));
    for value in values {
        params.push(rusqlite::types::Value::Text(value.clone()));
    }
}

pub(crate) fn catalog_filter_sql(
    query: &ConversationQuery,
) -> (String, Vec<rusqlite::types::Value>) {
    let mut clauses = vec!["sessions.is_top_level = 1".to_string()];
    let mut params = Vec::new();
    push_in_filter(&mut clauses, &mut params, "sessions.source", &query.sources);
    push_in_filter(
        &mut clauses,
        &mut params,
        "sessions.project",
        &query.projects,
    );
    if !query.models.is_empty() {
        let placeholders = sql_placeholders(query.models.len());
        clauses.push(format!(
            "(sessions.model IN ({placeholders}) OR EXISTS (\
                SELECT 1 FROM cursor_sessions cs, json_each(cs.models_json) AS je \
                WHERE sessions.source = 'cursor_agent' \
                  AND cs.session_id = sessions.session_id \
                  AND je.value IN ({placeholders})\
            ))"
        ));
        for _ in 0..2 {
            for model in &query.models {
                params.push(rusqlite::types::Value::Text(model.clone()));
            }
        }
    }
    if !query.providers.is_empty() {
        clauses.push(format!(
            "EXISTS (\
                SELECT 1 FROM usage_records r \
                WHERE r.source = sessions.source \
                  AND r.session_id = sessions.session_id \
                  AND r.provider IN ({})\
            )",
            sql_placeholders(query.providers.len())
        ));
        for provider in &query.providers {
            params.push(rusqlite::types::Value::Text(provider.clone()));
        }
    }
    if let Some(from) = query.from.as_deref().filter(|value| !value.is_empty()) {
        let end_at = catalog_session_end_sql();
        clauses.push(format!("({end_at} != '' AND {end_at} >= ?)"));
        params.push(rusqlite::types::Value::Text(from.to_string()));
    }
    if let Some(to) = query.to.as_deref().filter(|value| !value.is_empty()) {
        let start_at = catalog_session_start_sql();
        clauses.push(format!("({start_at} != '' AND {start_at} <= ?)"));
        params.push(rusqlite::types::Value::Text(to.to_string()));
    }
    if let Some(event_predicate) = catalog_tool_event_predicate(query) {
        clauses.push(catalog_event_exists_sql(&event_predicate));
        for name in query.tool_names.iter().filter(|name| !name.is_empty()) {
            params.push(rusqlite::types::Value::Text(name.clone()));
        }
    }
    (clauses.join(" AND "), params)
}

pub(crate) fn catalog_event_exists_sql(extra: &str) -> String {
    format!(
        "EXISTS (\
            SELECT 1 FROM conversation_events e \
            WHERE e.source = sessions.source \
              AND e.session_id = sessions.session_id \
              AND e.index_generation = sessions.event_index_generation \
              AND ({extra})\
        )"
    )
}

pub(crate) fn catalog_tool_event_predicate(query: &ConversationQuery) -> Option<String> {
    let names = query
        .tool_names
        .iter()
        .filter(|name| !name.is_empty())
        .count();
    if names == 0 && !query.tool_failed {
        return None;
    }
    let mut parts = Vec::new();
    if query.tool_failed {
        // 失败的 tool_result 在 ingest 时记成 kind=error / actor=tool；
        // 事件表没有 is_error 列，目录不能靠 kind=tool_result 判断失败。
        parts.push(catalog_tool_failure_sql().to_string());
    } else {
        parts.push(format!(
            "(e.kind IN ('tool_call', 'tool_result') OR {})",
            catalog_tool_failure_sql()
        ));
    }
    if names > 0 {
        parts.push(format!("e.name IN ({})", sql_placeholders(names)));
    }
    Some(parts.join(" AND "))
}

pub(crate) fn catalog_tool_failure_sql() -> &'static str {
    "e.kind = 'error' AND e.actor = 'tool'"
}

pub fn catalog_tool_names(
    conn: &Connection,
    query: &ConversationQuery,
) -> Result<Vec<String>, String> {
    let options_query = ConversationQuery {
        search: None,
        page: None,
        page_size: None,
        tool_names: Vec::new(),
        tool_failed: false,
        ..query.clone()
    };
    let (predicate, params) = catalog_filter_sql(&options_query);
    let sql = format!(
        r#"
        SELECT DISTINCT e.name
        FROM conversation_events e
        JOIN conversation_sessions AS sessions
          ON sessions.source = e.source
         AND sessions.session_id = e.session_id
         AND sessions.event_index_generation = e.index_generation
        WHERE {predicate}
          AND e.kind = 'tool_call'
          AND e.name IS NOT NULL
          AND e.name != ''
        ORDER BY e.name COLLATE NOCASE
        "#
    );
    let mut statement = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let names = statement
        .query_map(params_from_iter(params.iter()), |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(names)
}

pub fn sessions_page(
    conn: &Connection,
    query: &ConversationQuery,
) -> Result<ConversationPage, String> {
    sessions_page_with_prices(conn, query, &PriceTable::default())
}

pub fn indexed_events(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Vec<ConversationEvent>, String> {
    event_index::indexed_events(conn, source, session_id)
}

pub fn usage_records_page(
    conn: &Connection,
    source: &str,
    session_id: &str,
    page: u32,
    page_size: u32,
) -> Result<ConversationUsagePage, String> {
    let source = Source::parse(source).filter(|source| CONVERSATION_SOURCES.contains(source));
    let Some(source) = source else {
        return Err("该来源尚未支持对话详情".to_string());
    };
    let records = load_usage_records(conn, source, session_id)?;
    let total = records.len() as u32;
    let page = page.max(1);
    let page_size = page_size.clamp(1, MAX_PAGE_SIZE);
    let start = ((page - 1) * page_size) as usize;
    let rows = if start >= records.len() {
        Vec::new()
    } else {
        let end = (start + page_size as usize).min(records.len());
        records[start..end].to_vec()
    };
    Ok(ConversationUsagePage { rows, total })
}

pub fn sessions_page_with_prices(
    conn: &Connection,
    query: &ConversationQuery,
    prices: &PriceTable,
) -> Result<ConversationPage, String> {
    let (predicate, params) = catalog_filter_sql(query);
    if !query.search.as_deref().unwrap_or("").trim().is_empty() {
        return catalog_search::sessions_page_with_search(conn, query, prices, &predicate, params);
    }

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let offset = i64::from((page - 1) * page_size);
    let mut params = params;

    let total = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM conversation_sessions AS sessions WHERE {predicate}"),
            params_from_iter(params.iter()),
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())? as u32;

    params.push(rusqlite::types::Value::Integer(i64::from(page_size)));
    params.push(rusqlite::types::Value::Integer(offset));
    let ready_sql = catalog_search::event_index_ready_sql("sessions");
    let sql = format!(
        r#"
        SELECT sessions.source, sessions.session_id, sessions.title, sessions.project, sessions.model,
               COALESCE(NULLIF(sessions.started_at, ''), cursor_times.first_seen_at, '') AS started_at,
               COALESCE(NULLIF(sessions.ended_at, ''), cursor_times.last_seen_at, cursor_times.first_seen_at, '') AS ended_at,
               sessions.source_file, sessions.capabilities_json, sessions.support_status, sessions.file_available,
               {ready_sql},
               -1
        FROM conversation_sessions AS sessions
        LEFT JOIN cursor_sessions AS cursor_times
          ON sessions.source = 'cursor_agent' AND sessions.session_id = cursor_times.session_id
        WHERE {predicate}
        ORDER BY COALESCE(
            NULLIF(sessions.ended_at, ''),
            NULLIF(sessions.started_at, ''),
            cursor_times.last_seen_at,
            cursor_times.first_seen_at,
            ''
        ) DESC, sessions.source ASC, sessions.session_id ASC
        LIMIT ? OFFSET ?
        "#
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(params_from_iter(params.iter()), row_from_sql)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    finish_catalog_rows(conn, prices, &mut rows)?;

    Ok(ConversationPage { rows, total })
}

pub(crate) fn finish_catalog_rows(
    conn: &Connection,
    prices: &PriceTable,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    for row in rows.iter_mut() {
        let paths = load_session_files(conn, &row.source, &row.session_id)?;
        if !paths.is_empty() {
            row.source_files = paths
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect();
        }
    }
    hydrate_catalog_usage(conn, prices, rows)?;
    hydrate_cursor_hash_models(conn, rows)?;
    Ok(())
}

pub(crate) fn hydrate_catalog_usage(
    conn: &Connection,
    prices: &PriceTable,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let keys = rows
        .iter()
        .map(|row| (row.source.clone(), row.session_id.clone()))
        .collect::<Vec<_>>();
    let totals = query::usage_rollups_for_sessions(conn, prices, &keys)?;
    for row in rows {
        let Some(usage) = totals.get(&(row.source.clone(), row.session_id.clone())) else {
            continue;
        };
        row.total_tokens = usage.total_tokens;
        row.cost = usage.cost;
        row.unpriced = usage.unpriced;
    }
    Ok(())
}
