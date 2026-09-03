use std::collections::BTreeMap;

use rusqlite::{params, Connection};

use crate::billing_window;
use crate::domain::{Source, WorkSessionSpan, WorkTimelineDto};

use super::sql::*;

pub fn work_timeline(conn: &Connection, day: &str) -> Result<WorkTimelineDto, String> {
    let Some((from, to)) = crate::work_timeline::broad_date_bounds(day) else {
        return Ok(WorkTimelineDto::empty(day));
    };
    let Some((day_start, day_end)) = crate::work_timeline::local_day_sql_bounds(day) else {
        return Ok(WorkTimelineDto::empty(day));
    };
    let to_end = billing_window::iso_day_end(&to);
    let project_key = latest_nonempty_key_sql("project");
    let model_key = latest_nonempty_key_sql("model");
    let sql = format!(
        "SELECT
            r.source,
            r.session_id,
            MIN(r.occurred_at),
            MAX(r.occurred_at),
            {project_key},
            {model_key},
            COALESCE(SUM(CASE WHEN r.occurred_at >= ?3 AND r.occurred_at < ?4 THEN r.total_tokens ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN r.occurred_at >= ?3 AND r.occurred_at < ?4 THEN 1 ELSE 0 END), 0)
        FROM usage_records r
        WHERE r.occurred_at >= ?1 AND r.occurred_at < ?2
        GROUP BY r.source, r.session_id"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![from, to_end, day_start, day_end], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut sessions = BTreeMap::new();
    for row in rows {
        let (source, session_id, start_at, end_at, project_key, model_key, day_tokens, day_turns) =
            row.map_err(|e| e.to_string())?;
        let Some(start) = billing_window::parse_occurred_at(&start_at) else {
            continue;
        };
        let Some(end) = billing_window::parse_occurred_at(&end_at) else {
            continue;
        };
        let (project, project_at) = split_latest_key(project_key);
        let (model, model_at) = split_latest_key(model_key);
        sessions.insert(
            (source.clone(), session_id.clone()),
            crate::work_timeline::SessionAcc {
                source,
                session_id,
                project,
                project_at,
                model,
                model_at,
                start,
                end,
                day_tokens,
                day_turns,
            },
        );
    }
    let extra = work_session_spans(conn, &from, &to)?;
    Ok(crate::work_timeline::assemble(sessions, &extra, day))
}

fn split_latest_key(raw: Option<String>) -> (String, Option<String>) {
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return (String::new(), None);
    };
    match raw.split_once('\u{1f}') {
        Some((at, value)) => (value.to_string(), Some(at.to_string())),
        None => (String::new(), None),
    }
}

/// 宽口径拉取与 `[from, to]` 日期串有交集的 Cursor 本机会话，转成时间线补充区间。
/// `first_seen_at` / `last_seen_at` 缺一则无法画条，直接跳过。
pub(crate) fn work_session_spans(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<WorkSessionSpan>, String> {
    let to_end = billing_window::iso_day_end(to);
    let mut stmt = conn
        .prepare(
            r#"
            SELECT session_id, project, models_json, first_seen_at, last_seen_at
            FROM cursor_sessions
            WHERE first_seen_at IS NOT NULL AND first_seen_at != ''
              AND last_seen_at IS NOT NULL AND last_seen_at != ''
              AND first_seen_at < ?2
              AND last_seen_at >= ?1
            "#,
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![from, to_end], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut by_session: BTreeMap<String, WorkSessionSpan> = BTreeMap::new();
    for row in rows {
        let (session_id, project, models_json, first_seen_at, last_seen_at) =
            row.map_err(|e| e.to_string())?;
        let span = WorkSessionSpan {
            source: Source::CursorAgent.as_str().to_string(),
            session_id: session_id.clone(),
            project,
            model: last_model_from_json(&models_json),
            started_at: first_seen_at,
            ended_at: last_seen_at,
        };
        match by_session.get_mut(&session_id) {
            Some(existing) => {
                if span.started_at < existing.started_at {
                    existing.started_at = span.started_at;
                }
                if span.ended_at > existing.ended_at {
                    existing.ended_at = span.ended_at;
                    if !span.model.is_empty() {
                        existing.model = span.model;
                    }
                }
                if !span.project.is_empty() {
                    existing.project = span.project;
                }
            }
            None => {
                by_session.insert(session_id, span);
            }
        }
    }
    Ok(by_session.into_values().collect())
}

fn last_model_from_json(raw: &str) -> String {
    serde_json::from_str::<Vec<String>>(raw)
        .ok()
        .and_then(|models| models.into_iter().next_back())
        .unwrap_or_default()
}
