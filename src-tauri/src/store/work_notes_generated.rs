use std::ffi::OsStr;
use std::path::Path;

use rusqlite::{params, Connection};

use crate::domain::{ConversationSessionRow, WORK_NOTES_ENGINE_DIR};

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

pub fn mark_generated_sessions(
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

pub fn mark_generated_session(
    conn: &Connection,
    row: &mut ConversationSessionRow,
) -> Result<(), String> {
    mark_generated_sessions(conn, std::slice::from_mut(row))
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
