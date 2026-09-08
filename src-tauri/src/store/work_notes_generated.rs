use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;

use rusqlite::{params, params_from_iter, Connection};

use crate::domain::{ConversationSessionRow, WorkNotesSessionSummary, WORK_NOTES_ENGINE_DIR};

#[derive(Debug, Clone)]
pub struct GeneratedSession {
    pub engine: String,
    pub session_id: String,
    pub work_dir: String,
}

pub fn record_generated_session(
    conn: &Connection,
    engine: &str,
    session_id: Option<&str>,
    work_dir: &Path,
    started_at: &str,
    ended_at: &str,
) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO work_notes_generated_sessions(engine, session_id, work_dir, started_at, ended_at)
        VALUES(?1, ?2, ?3, ?4, ?5)
        "#,
        params![
            engine,
            session_id.unwrap_or(""),
            work_dir.to_string_lossy().as_ref(),
            started_at,
            ended_at,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn load_generated_sessions(conn: &Connection) -> Result<Vec<GeneratedSession>, String> {
    let mut stmt = conn
        .prepare("SELECT engine, session_id, work_dir FROM work_notes_generated_sessions")
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(GeneratedSession {
                engine: row.get(0)?,
                session_id: row.get(1)?,
                work_dir: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub fn session_is_generated(
    rows: &[GeneratedSession],
    source: &str,
    session_id: &str,
    project: &str,
) -> bool {
    rows.iter().any(|row| {
        if !row.session_id.is_empty()
            && row.session_id == session_id
            && engine_source(&row.engine) == source
        {
            return true;
        }
        project_matches_work_dir(&row.work_dir, project)
    })
}

fn mark_generated_sessions(
    conn: &Connection,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    let generated = load_generated_sessions(conn)?;
    for row in rows {
        row.generated_by_work_notes =
            session_is_generated(&generated, &row.source, &row.session_id, &row.project);
    }
    Ok(())
}

pub fn decorate_conversation_sessions(
    conn: &Connection,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    mark_generated_sessions(conn, rows)?;
    attach_work_notes_summaries(conn, rows)
}

fn attach_work_notes_summaries(
    conn: &Connection,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut sql = String::from(
        "SELECT c.source, c.session_id, c.engine, c.model, c.summary, c.created_at
         FROM summary_session_cache AS c
         INNER JOIN conversation_sessions AS s
           ON s.source = c.source
          AND s.session_id = c.session_id
          AND s.source_revision = c.fingerprint
         WHERE ",
    );
    let mut binds = Vec::with_capacity(rows.len() * 2);
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            sql.push_str(" OR ");
        }
        sql.push_str("(c.source = ? AND c.session_id = ?)");
        binds.push(row.source.clone());
        binds.push(row.session_id.clone());
    }
    sql.push_str(" ORDER BY c.created_at DESC, c.engine ASC, c.model ASC");
    let mut stmt = conn.prepare(&sql).map_err(|error| error.to_string())?;
    let fetched = stmt
        .query_map(params_from_iter(binds.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                WorkNotesSessionSummary {
                    engine: row.get(2)?,
                    model: row.get(3)?,
                    summary: row.get(4)?,
                    created_at: row.get(5)?,
                },
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut grouped: BTreeMap<(String, String), Vec<WorkNotesSessionSummary>> = BTreeMap::new();
    for (source, session_id, summary) in fetched {
        if summary.summary.trim().is_empty() {
            continue;
        }
        grouped
            .entry((source, session_id))
            .or_default()
            .push(summary);
    }
    for row in rows {
        row.work_notes_summaries = grouped
            .remove(&(row.source.clone(), row.session_id.clone()))
            .unwrap_or_default();
    }
    Ok(())
}

fn engine_source(engine: &str) -> &str {
    match engine {
        "cursor-agent" => "cursor_agent",
        other => other,
    }
}

fn project_matches_work_dir(work_dir: &str, project: &str) -> bool {
    if work_dir.is_empty() || project.is_empty() {
        return false;
    }
    if project == work_dir {
        return true;
    }
    let work = Path::new(work_dir);
    let proj = Path::new(project);
    matches!(
        (work.file_name(), proj.file_name()),
        (Some(left), Some(right))
            if left == right && left == OsStr::new(WORK_NOTES_ENGINE_DIR)
    )
}
