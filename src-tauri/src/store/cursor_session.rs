use std::collections::BTreeSet;

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::CursorSessionRecord;

pub fn cursor_session_has_source_file(conn: &Connection, path: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT 1 FROM cursor_sessions WHERE source_file = ?1",
        params![path],
        |_| Ok(()),
    )
    .optional()
    .map(|row| row.is_some())
    .map_err(|e| e.to_string())
}

pub fn cached_cursor_session_file_stats(
    conn: &Connection,
) -> Result<Vec<(String, i64, i64)>, String> {
    let mut stmt = conn
        .prepare("SELECT path, mtime_ms, size FROM cursor_session_files")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn cursor_session_file_fingerprint(
    conn: &Connection,
    path: &str,
) -> Result<Option<(i64, i64)>, String> {
    conn.query_row(
        "SELECT mtime_ms, size FROM cursor_session_files WHERE path = ?1",
        params![path],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn cursor_session_source_file_for_id(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT source_file FROM cursor_sessions WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn cursor_session_has_id(conn: &Connection, session_id: &str) -> Result<bool, String> {
    Ok(cursor_session_source_file_for_id(conn, session_id)?.is_some())
}

pub fn delete_cursor_sessions_by_id(conn: &Connection, session_id: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM cursor_sessions WHERE session_id = ?1",
        params![session_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn upsert_cursor_session(
    conn: &Connection,
    record: &CursorSessionRecord,
) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO cursor_sessions (
            source_file, session_id, project, turn_count, success_count, error_count, aborted_count,
            user_prompt_count, subagent_count, tool_calls_json, models_json, sources_json,
            extensions_json, first_seen_at, last_seen_at, files_touched
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
        ON CONFLICT(source_file) DO UPDATE SET
            session_id = excluded.session_id,
            project = excluded.project,
            turn_count = excluded.turn_count,
            success_count = excluded.success_count,
            error_count = excluded.error_count,
            aborted_count = excluded.aborted_count,
            user_prompt_count = excluded.user_prompt_count,
            subagent_count = excluded.subagent_count,
            tool_calls_json = excluded.tool_calls_json,
            models_json = excluded.models_json,
            sources_json = excluded.sources_json,
            extensions_json = excluded.extensions_json,
            first_seen_at = COALESCE(cursor_sessions.first_seen_at, excluded.first_seen_at),
            last_seen_at = excluded.last_seen_at,
            files_touched = excluded.files_touched
        "#,
        params![
            record.source_file,
            record.session_id,
            record.project,
            record.turn_count,
            record.success_count,
            record.error_count,
            record.aborted_count,
            record.user_prompt_count,
            record.subagent_count,
            record.tool_calls_json,
            record.models_json,
            record.sources_json,
            record.extensions_json,
            record.first_seen_at,
            record.last_seen_at,
            record.files_touched,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn upsert_cursor_session_file(
    conn: &Connection,
    path: &str,
    mtime_ms: i64,
    size: i64,
) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO cursor_session_files(path, mtime_ms, size) VALUES(?1,?2,?3)
        ON CONFLICT(path) DO UPDATE SET mtime_ms = excluded.mtime_ms, size = excluded.size
        "#,
        params![path, mtime_ms, size],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_cursor_sessions(conn: &Connection) -> Result<Vec<CursorSessionRecord>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT source_file, session_id, project, turn_count, success_count, error_count, aborted_count,
                   user_prompt_count, subagent_count, tool_calls_json, models_json, sources_json,
                   extensions_json, first_seen_at, last_seen_at, files_touched
            FROM cursor_sessions
            ORDER BY last_seen_at ASC, source_file ASC
            "#,
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CursorSessionRecord {
                source_file: row.get(0)?,
                session_id: row.get(1)?,
                project: row.get(2)?,
                turn_count: row.get(3)?,
                success_count: row.get(4)?,
                error_count: row.get(5)?,
                aborted_count: row.get(6)?,
                user_prompt_count: row.get(7)?,
                subagent_count: row.get(8)?,
                tool_calls_json: row.get(9)?,
                models_json: row.get(10)?,
                sources_json: row.get(11)?,
                extensions_json: row.get(12)?,
                first_seen_at: row.get(13)?,
                last_seen_at: row.get(14)?,
                files_touched: row.get(15)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn load_cursor_session(
    conn: &Connection,
    source_file: &str,
) -> Result<Option<CursorSessionRecord>, String> {
    conn.query_row(
        r#"
        SELECT source_file, session_id, project, turn_count, success_count, error_count, aborted_count,
               user_prompt_count, subagent_count, tool_calls_json, models_json, sources_json,
               extensions_json, first_seen_at, last_seen_at, files_touched
        FROM cursor_sessions
        WHERE source_file = ?1
        "#,
        params![source_file],
        |row| {
            Ok(CursorSessionRecord {
                source_file: row.get(0)?,
                session_id: row.get(1)?,
                project: row.get(2)?,
                turn_count: row.get(3)?,
                success_count: row.get(4)?,
                error_count: row.get(5)?,
                aborted_count: row.get(6)?,
                user_prompt_count: row.get(7)?,
                subagent_count: row.get(8)?,
                tool_calls_json: row.get(9)?,
                models_json: row.get(10)?,
                sources_json: row.get(11)?,
                extensions_json: row.get(12)?,
                first_seen_at: row.get(13)?,
                last_seen_at: row.get(14)?,
                files_touched: row.get(15)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn reconcile_cursor_sessions(
    conn: &Connection,
    seen_paths: &BTreeSet<String>,
) -> Result<u64, String> {
    let cached: Vec<String> = conn
        .prepare("SELECT source_file FROM cursor_sessions")
        .map_err(|e| e.to_string())?
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut removed = 0u64;
    for path in cached {
        if seen_paths.contains(&path) {
            continue;
        }
        conn.execute(
            "DELETE FROM cursor_sessions WHERE source_file = ?1",
            params![path],
        )
        .map_err(|e| e.to_string())?;
        removed += 1;
    }
    Ok(removed)
}

pub fn reconcile_cursor_session_files(
    conn: &Connection,
    seen_paths: &BTreeSet<String>,
) -> Result<u64, String> {
    let cached: Vec<String> = conn
        .prepare("SELECT path FROM cursor_session_files")
        .map_err(|e| e.to_string())?
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut removed = 0u64;
    for path in cached {
        if seen_paths.contains(&path) {
            continue;
        }
        conn.execute(
            "DELETE FROM cursor_session_files WHERE path = ?1",
            params![path],
        )
        .map_err(|e| e.to_string())?;
        removed += 1;
    }
    Ok(removed)
}

pub const CURSOR_SESSION_SCHEMA_VERSION: &str = "2";

pub fn cursor_session_schema_version(conn: &Connection) -> Result<String, String> {
    conn.query_row(
        "SELECT value FROM cursor_session_meta WHERE key = 'schema_version'",
        [],
        |row| row.get(0),
    )
    .optional()
    .map(|value| value.unwrap_or_default())
    .map_err(|e| e.to_string())
}

pub fn set_cursor_session_schema_version(conn: &Connection, version: &str) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO cursor_session_meta(key, value) VALUES('schema_version', ?1)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
        params![version],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_cursor_session_as_of(conn: &Connection, as_of: &str) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO cursor_session_meta(key, value) VALUES('as_of', ?1)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
        params![as_of],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn cursor_session_as_of(conn: &Connection) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM cursor_session_meta WHERE key = 'as_of'",
        [],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn set_cursor_tracking_fingerprint(conn: &Connection, fingerprint: &str) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO cursor_session_meta(key, value) VALUES('tracking_fingerprint', ?1)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
        params![fingerprint],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn cursor_tracking_fingerprint(conn: &Connection) -> Result<String, String> {
    conn.query_row(
        "SELECT value FROM cursor_session_meta WHERE key = 'tracking_fingerprint'",
        [],
        |row| row.get(0),
    )
    .optional()
    .map(|value| value.unwrap_or_default())
    .map_err(|e| e.to_string())
}
