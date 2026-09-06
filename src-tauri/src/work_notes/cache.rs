use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{WorkNotesDto, WorkNotesEntry, WorkNotesFailure, WorkNotesRangeKind};

use super::period::ResolvedRange;

const NOTES_COLUMNS: &str =
    "range_kind, start_date, end_date, extra_instructions, skipped_sparse, \
     session_count, project_count, active_days, total_tokens, headline, entries_json, closing, \
     failed_count, failures_json, actual_input_tokens, actual_output_tokens, actual_cost, actual_unpriced";

pub fn session_fingerprint(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<String, String> {
    conn.query_row(
        "SELECT source_revision FROM conversation_sessions WHERE source = ?1 AND session_id = ?2",
        params![source, session_id],
        |row| row.get(0),
    )
    .map_err(|error| error.to_string())
}

pub fn session_set_hash(parts: &[(String, String, String)]) -> String {
    let mut rows: Vec<String> = parts
        .iter()
        .map(|(source, session_id, fingerprint)| format!("{source}\0{session_id}\0{fingerprint}"))
        .collect();
    rows.sort();
    rows.join("\n")
}

pub fn range_key(range: &ResolvedRange) -> String {
    range_key_parts(range.kind, &range.start_date, &range.end_date)
}

fn range_key_parts(kind: WorkNotesRangeKind, start_date: &str, end_date: &str) -> String {
    match kind {
        WorkNotesRangeKind::Custom => format!("custom:{start_date}:{end_date}"),
        WorkNotesRangeKind::ThisWeek => format!("this_week:{start_date}"),
        WorkNotesRangeKind::ThisMonth => format!("this_month:{start_date}"),
    }
}

pub struct SessionCacheKey<'a> {
    pub source: &'a str,
    pub session_id: &'a str,
    pub fingerprint: &'a str,
    pub engine: &'a str,
    pub model: &'a str,
}

pub fn load_session_summary(
    conn: &Connection,
    key: &SessionCacheKey<'_>,
) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT summary FROM summary_session_cache
         WHERE source = ?1 AND session_id = ?2 AND fingerprint = ?3 AND engine = ?4 AND model = ?5",
        params![
            key.source,
            key.session_id,
            key.fingerprint,
            key.engine,
            key.model
        ],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| error.to_string())
}

pub fn store_session_summary(
    conn: &Connection,
    key: &SessionCacheKey<'_>,
    summary: &str,
    created_at: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO summary_session_cache(
            source, session_id, fingerprint, engine, model, summary, created_at
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(source, session_id, fingerprint, engine, model)
         DO UPDATE SET summary = excluded.summary, created_at = excluded.created_at",
        params![
            key.source,
            key.session_id,
            key.fingerprint,
            key.engine,
            key.model,
            summary,
            created_at
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn load_exact_notes(
    conn: &Connection,
    range: &ResolvedRange,
    engine: &str,
    model: &str,
    extra: &str,
    session_set_hash: &str,
) -> Result<Option<WorkNotesDto>, String> {
    conn.query_row(
        &format!(
            "SELECT {NOTES_COLUMNS} FROM summary_reports
             WHERE range_key = ?1 AND engine = ?2 AND model = ?3
               AND extra_instructions = ?4 AND session_set_hash = ?5"
        ),
        params![range_key(range), engine, model, extra, session_set_hash],
        notes_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

pub fn load_latest_notes(
    conn: &Connection,
    range: &ResolvedRange,
    engine: &str,
    model: &str,
) -> Result<Option<WorkNotesDto>, String> {
    conn.query_row(
        &format!(
            "SELECT {NOTES_COLUMNS} FROM summary_reports
             WHERE range_key = ?1 AND engine = ?2 AND model = ?3
             ORDER BY created_at DESC, id DESC LIMIT 1"
        ),
        params![range_key(range), engine, model],
        notes_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

pub fn store_notes(
    conn: &Connection,
    dto: &WorkNotesDto,
    engine: &str,
    model: &str,
    extra: &str,
    session_set_hash: &str,
    created_at: &str,
) -> Result<(), String> {
    let entries_json = serde_json::to_string(&dto.entries).map_err(|error| error.to_string())?;
    let failures_json = serde_json::to_string(&dto.failures).map_err(|error| error.to_string())?;
    let key = range_key_parts(dto.range_kind, &dto.start_date, &dto.end_date);
    conn.execute(
        "INSERT INTO summary_reports(
            created_at, range_key, range_kind, start_date, end_date, engine, model,
            extra_instructions, session_set_hash, skipped_sparse, session_count,
            project_count, active_days, total_tokens, headline, entries_json, closing,
            failed_count, failures_json, actual_input_tokens, actual_output_tokens,
            actual_cost, actual_unpriced
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)
         ON CONFLICT(range_key, engine, model, extra_instructions, session_set_hash)
         DO UPDATE SET
            created_at = excluded.created_at,
            end_date = excluded.end_date,
            skipped_sparse = excluded.skipped_sparse,
            session_count = excluded.session_count,
            project_count = excluded.project_count,
            active_days = excluded.active_days,
            total_tokens = excluded.total_tokens,
            headline = excluded.headline,
            entries_json = excluded.entries_json,
            closing = excluded.closing,
            failed_count = excluded.failed_count,
            failures_json = excluded.failures_json,
            actual_input_tokens = excluded.actual_input_tokens,
            actual_output_tokens = excluded.actual_output_tokens,
            actual_cost = excluded.actual_cost,
            actual_unpriced = excluded.actual_unpriced",
        params![
            created_at,
            key,
            kind_slug(dto.range_kind),
            dto.start_date,
            dto.end_date,
            engine,
            model,
            extra,
            session_set_hash,
            dto.skipped_sparse,
            dto.session_count,
            dto.project_count,
            dto.active_days,
            dto.total_tokens,
            dto.headline,
            entries_json,
            dto.closing,
            dto.failed_count,
            failures_json,
            dto.actual_input_tokens,
            dto.actual_output_tokens,
            dto.actual_cost,
            dto.actual_unpriced as i64
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn notes_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkNotesDto> {
    let kind_raw: String = row.get(0)?;
    let kind = parse_kind(&kind_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown work notes range kind: {kind_raw}"),
            )),
        )
    })?;
    let entries_json: String = row.get(10)?;
    let entries: Vec<WorkNotesEntry> = serde_json::from_str(&entries_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let failures_json: String = row.get(13)?;
    let failures: Vec<WorkNotesFailure> =
        serde_json::from_str(&failures_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                13,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    let actual_unpriced: i64 = row.get(17)?;
    Ok(WorkNotesDto {
        range_kind: kind,
        start_date: row.get(1)?,
        end_date: row.get(2)?,
        has_data: true,
        extra_instructions: row.get(3)?,
        skipped_sparse: row.get(4)?,
        session_count: row.get(5)?,
        project_count: row.get(6)?,
        active_days: row.get(7)?,
        total_tokens: row.get(8)?,
        headline: row.get(9)?,
        entries,
        closing: row.get(11)?,
        failed_count: row.get(12)?,
        failures,
        actual_input_tokens: row.get(14)?,
        actual_output_tokens: row.get(15)?,
        actual_cost: row.get(16)?,
        actual_unpriced: actual_unpriced != 0,
    })
}

fn kind_slug(kind: WorkNotesRangeKind) -> &'static str {
    match kind {
        WorkNotesRangeKind::ThisWeek => "this_week",
        WorkNotesRangeKind::ThisMonth => "this_month",
        WorkNotesRangeKind::Custom => "custom",
    }
}

fn parse_kind(value: &str) -> Option<WorkNotesRangeKind> {
    match value {
        "this_week" => Some(WorkNotesRangeKind::ThisWeek),
        "this_month" => Some(WorkNotesRangeKind::ThisMonth),
        "custom" => Some(WorkNotesRangeKind::Custom),
        _ => None,
    }
}
