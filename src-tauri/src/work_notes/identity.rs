//! 码表生成身份：哪些会话是纪要引擎自己写出来的，以及这些会话上挂了哪些摘要。
//!
//! 规则住在工作纪要这一侧（agent-layers「工作纪要」第 7 条）：能钉 session id 就钉死，
//! 否则认专用工作目录。识别结果只有两个用处——从后续纪要输入里剔除、在对话记录上打标，
//! 都从这里出去，对话记录只看得到模块根的 `decorate_sessions`。

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;

use rusqlite::{params, params_from_iter, Connection};

use crate::domain::{ConversationSessionRow, WorkNotesSessionSummary, WORK_NOTES_ENGINE_DIR};

/// 一次引擎调用留下的落痕。`session_id` 为空表示这个引擎钉不住 id，只能认工作目录。
struct GeneratedSession {
    engine: String,
    session_id: String,
    work_dir: String,
}

/// 全部落痕的快照。一次查表，按行反复问。
pub(super) struct GeneratedIdentity(Vec<GeneratedSession>);

impl GeneratedIdentity {
    pub(super) fn load(conn: &Connection) -> Result<Self, String> {
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
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(Self(rows))
    }

    pub(super) fn contains(&self, source: &str, session_id: &str, project: &str) -> bool {
        self.0.iter().any(|row| {
            if !row.session_id.is_empty()
                && row.session_id == session_id
                && engine_source(&row.engine) == source
            {
                return true;
            }
            project_matches_work_dir(&row.work_dir, project)
        })
    }
}

pub(super) fn record(
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

/// 对话记录拿到会话行之后的唯一一条 seam：打「码表生成」标记、挂上已有摘要。
pub(super) fn decorate(
    conn: &Connection,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let identity = GeneratedIdentity::load(conn)?;
    let mut summaries = load_summaries(conn, rows)?;
    for row in rows {
        row.generated_by_work_notes = identity.contains(&row.source, &row.session_id, &row.project);
        row.work_notes_summaries = summaries
            .remove(&(row.source.clone(), row.session_id.clone()))
            .unwrap_or_default();
    }
    Ok(())
}

/// 只认指纹仍然对得上的缓存：会话变过之后，旧摘要不再代表这条会话。
fn load_summaries(
    conn: &Connection,
    rows: &[ConversationSessionRow],
) -> Result<BTreeMap<(String, String), Vec<WorkNotesSessionSummary>>, String> {
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
    Ok(grouped)
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
