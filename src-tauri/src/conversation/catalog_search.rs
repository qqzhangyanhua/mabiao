use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

use crate::domain::{
    ConversationMatchField, ConversationPage, ConversationQuery, ConversationSessionRow, PriceTable,
};

use super::session_store::row_from_sql;
use super::{finish_catalog_rows, CONVERSATION_ADAPTER_VERSION, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE};

const TITLE_LIKE_FIELDS: usize = 7;
const SNIPPET_RADIUS: usize = 48;
const FTS_MIN_CHARS: usize = 3;

pub(super) fn sessions_page_with_search(
    conn: &Connection,
    query: &ConversationQuery,
    prices: &PriceTable,
    predicate: &str,
    filter_params: Vec<rusqlite::types::Value>,
) -> Result<ConversationPage, String> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let offset = i64::from((page - 1) * page_size);
    let search = query.search.as_deref().unwrap_or("").trim();
    let include_body = search.chars().count() >= FTS_MIN_CHARS;
    let with_sql = ranked_with_sql(predicate, include_body);
    let mut params = filter_params;
    push_search_params(&mut params, search, include_body);

    let total = conn
        .query_row(
            &format!("{with_sql} SELECT COUNT(*) FROM ranked"),
            params_from_iter(params.iter()),
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())? as u32;

    params.push(rusqlite::types::Value::Integer(i64::from(page_size)));
    params.push(rusqlite::types::Value::Integer(offset));
    let sql = format!(
        r#"
        {with_sql}
        SELECT filtered.source, filtered.session_id, filtered.title, filtered.project, filtered.model,
               filtered.started_at, filtered.ended_at, filtered.source_file, filtered.capabilities_json,
               filtered.support_status, filtered.file_available, filtered.event_index_ready,
               ranked.match_rank
        FROM ranked
        JOIN filtered
          ON filtered.source = ranked.source AND filtered.session_id = ranked.session_id
        ORDER BY ranked.match_rank ASC, filtered.ended_at DESC, filtered.source ASC, filtered.session_id ASC
        LIMIT ? OFFSET ?
        "#
    );
    let mut statement = conn.prepare(&sql).map_err(|error| error.to_string())?;
    let mut rows = statement
        .query_map(params_from_iter(params.iter()), row_from_sql)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    hydrate_body_matches(conn, search, include_body, &mut rows)?;
    finish_catalog_rows(conn, prices, &mut rows)?;
    Ok(ConversationPage { rows, total })
}

fn ranked_with_sql(predicate: &str, include_body: bool) -> String {
    let ready_sql = event_index_ready_sql("sessions");
    let body_hits = if include_body {
        r#"
        body_hits AS (
            SELECT e.source, e.session_id, 1 AS match_rank
            FROM conversation_events_fts
            JOIN conversation_events AS e ON e.rowid = conversation_events_fts.rowid
            JOIN filtered AS s
              ON s.source = e.source
             AND s.session_id = e.session_id
             AND e.index_generation = s.event_index_generation
            WHERE conversation_events_fts MATCH ?
            GROUP BY e.source, e.session_id
        )
        "#
        .to_string()
    } else {
        "body_hits AS (SELECT NULL AS source, NULL AS session_id, 1 AS match_rank WHERE 0)"
            .to_string()
    };
    format!(
        r#"
        WITH filtered AS (
            SELECT sessions.source, sessions.session_id, sessions.title, sessions.project, sessions.model,
                   COALESCE(NULLIF(sessions.started_at, ''), cursor_times.first_seen_at, '') AS started_at,
                   COALESCE(NULLIF(sessions.ended_at, ''), cursor_times.last_seen_at, cursor_times.first_seen_at, '') AS ended_at,
                   sessions.source_file, sessions.capabilities_json, sessions.support_status, sessions.file_available,
                   sessions.event_index_generation,
                   {ready_sql} AS event_index_ready
            FROM conversation_sessions AS sessions
            LEFT JOIN cursor_sessions AS cursor_times
              ON sessions.source = 'cursor_agent' AND sessions.session_id = cursor_times.session_id
            WHERE {predicate}
        ),
        title_hits AS (
            SELECT source, session_id, 0 AS match_rank
            FROM filtered
            WHERE title LIKE ? ESCAPE '\' OR source LIKE ? ESCAPE '\'
               OR project LIKE ? ESCAPE '\' OR model LIKE ? ESCAPE '\'
               OR session_id LIKE ? ESCAPE '\' OR started_at LIKE ? ESCAPE '\'
               OR ended_at LIKE ? ESCAPE '\'
        ),
        {body_hits},
        ranked AS (
            SELECT source, session_id, MIN(match_rank) AS match_rank
            FROM (
                SELECT source, session_id, match_rank FROM title_hits
                UNION ALL
                SELECT source, session_id, match_rank FROM body_hits
            )
            GROUP BY source, session_id
        )
        "#
    )
}

pub(super) fn event_index_ready_sql(alias: &str) -> String {
    format!(
        "CASE WHEN {alias}.adapter_version = {CONVERSATION_ADAPTER_VERSION} AND {alias}.event_index_generation IS NOT NULL THEN 1 ELSE 0 END"
    )
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn push_search_params(params: &mut Vec<rusqlite::types::Value>, search: &str, include_body: bool) {
    let pattern = format!("%{}%", escape_like(search));
    for _ in 0..TITLE_LIKE_FIELDS {
        params.push(rusqlite::types::Value::Text(pattern.clone()));
    }
    if include_body {
        params.push(rusqlite::types::Value::Text(fts_match_query(search)));
    }
}

fn fts_match_query(search: &str) -> String {
    let clipped: String = search.chars().take(200).collect();
    format!("\"{}\"", clipped.replace('"', "\"\""))
}

fn hydrate_body_matches(
    conn: &Connection,
    search: &str,
    include_body: bool,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    if !include_body {
        return Ok(());
    }
    let match_query = fts_match_query(search);
    for row in rows.iter_mut() {
        if row.match_field != Some(ConversationMatchField::Body) {
            continue;
        }
        let Some((event_id, sequence, snippet)) =
            first_body_hit(conn, &row.source, &row.session_id, &match_query, search)?
        else {
            continue;
        };
        row.match_event_id = Some(event_id);
        row.match_sequence = Some(sequence);
        row.match_snippet = Some(snippet);
    }
    Ok(())
}

fn first_body_hit(
    conn: &Connection,
    source: &str,
    session_id: &str,
    match_query: &str,
    search: &str,
) -> Result<Option<(String, u32, String)>, String> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT e.event_id, e.sequence, e.text, e.name
            FROM conversation_events_fts
            JOIN conversation_events AS e ON e.rowid = conversation_events_fts.rowid
            JOIN conversation_sessions AS s
              ON s.source = e.source
             AND s.session_id = e.session_id
             AND e.index_generation = s.event_index_generation
            WHERE e.source = ?1 AND e.session_id = ?2
              AND conversation_events_fts MATCH ?3
            ORDER BY e.sequence ASC
            LIMIT 1
            "#,
        )
        .map_err(|error| error.to_string())?;
    statement
        .query_row(params![source, session_id, match_query], |row| {
            let event_id: String = row.get(0)?;
            let sequence: u32 = row.get(1)?;
            let text: Option<String> = row.get(2)?;
            let name: Option<String> = row.get(3)?;
            Ok((
                event_id,
                sequence,
                snippet_for(search, text.as_deref(), name.as_deref()),
            ))
        })
        .optional()
        .map_err(|error| error.to_string())
}

fn snippet_for(search: &str, text: Option<&str>, name: Option<&str>) -> String {
    if let Some(text) = text.filter(|value| contains_ignore_ascii_case(value, search)) {
        return excerpt(text, search, SNIPPET_RADIUS);
    }
    if let Some(name) = name.filter(|value| contains_ignore_ascii_case(value, search)) {
        return excerpt(name, search, SNIPPET_RADIUS);
    }
    if let Some(text) = text.filter(|value| !value.is_empty()) {
        return excerpt(text, search, SNIPPET_RADIUS);
    }
    name.unwrap_or("").to_string()
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn excerpt(text: &str, query: &str, radius: usize) -> String {
    let haystack = text.to_ascii_lowercase();
    let needle = query.to_ascii_lowercase();
    let Some(byte_at) = haystack.find(&needle) else {
        return ellipsize(text, radius * 2);
    };
    let match_end = (byte_at + query.len()).min(text.len());
    let start = floor_char_boundary(text, byte_at.saturating_sub(radius));
    let end = ceil_char_boundary(text, (match_end + radius).min(text.len()));
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(&text[start..end]);
    if end < text.len() {
        out.push('…');
    }
    out
}

fn ellipsize(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let taken: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{taken}…")
    } else {
        taken
    }
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}
