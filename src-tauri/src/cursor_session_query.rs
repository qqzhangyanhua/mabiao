use std::collections::BTreeMap;

use rusqlite::{params_from_iter, types::Value, Connection};

use crate::adapters::cursor_session::tool_group;
use crate::domain::{
    CursorSessionDailyPoint, CursorSessionExtensionRow, CursorSessionListRow,
    CursorSessionModelRow, CursorSessionPage, CursorSessionProjectRow, CursorSessionQuery,
    CursorSessionRecord, CursorSessionSourceRow, CursorSessionSummaryDto, CursorSessionToolRow,
};
use crate::store;

#[derive(Default)]
struct ProjectAgg {
    session_count: i64,
    turn_count: i64,
    error_count: i64,
    files_touched: i64,
    last_seen_at: Option<String>,
}

pub fn load_summary(conn: &Connection) -> Result<CursorSessionSummaryDto, String> {
    let sessions = store::load_cursor_sessions(conn)?;
    let mut summary = summarize_cursor_sessions(&sessions);
    summary.as_of = store::cursor_session_as_of(conn)?;
    Ok(summary)
}

pub fn summarize_cursor_sessions(sessions: &[CursorSessionRecord]) -> CursorSessionSummaryDto {
    if sessions.is_empty() {
        return CursorSessionSummaryDto::empty();
    }

    let session_count = sessions.len() as i64;
    let turn_count: i64 = sessions.iter().map(|session| session.turn_count).sum();
    let error_count: i64 = sessions.iter().map(|session| session.error_count).sum();
    let aborted_count: i64 = sessions.iter().map(|session| session.aborted_count).sum();
    let user_prompt_count: i64 = sessions
        .iter()
        .map(|session| session.user_prompt_count)
        .sum();
    let subagent_count: i64 = sessions.iter().map(|session| session.subagent_count).sum();
    let error_rate = if turn_count > 0 {
        Some(error_count as f64 / turn_count as f64)
    } else {
        None
    };
    let average_turns = if session_count > 0 {
        Some(turn_count as f64 / session_count as f64)
    } else {
        None
    };
    let single_prompt_count = sessions
        .iter()
        .filter(|session| session.user_prompt_count == 1)
        .count();
    let single_prompt_ratio = if session_count > 0 {
        Some(single_prompt_count as f64 / session_count as f64)
    } else {
        None
    };

    let mut projects: BTreeMap<String, ProjectAgg> = BTreeMap::new();
    let mut daily: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut models: BTreeMap<String, i64> = BTreeMap::new();
    let mut sources: BTreeMap<String, i64> = BTreeMap::new();
    let mut extensions: BTreeMap<String, i64> = BTreeMap::new();
    let mut tools: BTreeMap<String, i64> = BTreeMap::new();
    let mut groups: BTreeMap<String, i64> = BTreeMap::new();

    for session in sessions {
        let project = display_project(&session.project);
        let entry = projects.entry(project).or_default();
        entry.session_count += 1;
        entry.turn_count += session.turn_count;
        entry.error_count += session.error_count;
        entry.files_touched += session.files_touched;
        entry.last_seen_at = later_ts(&entry.last_seen_at, &session.last_seen_at);

        if let Some(day) = session
            .last_seen_at
            .as_deref()
            .map(local_day)
            .filter(|day| !day.is_empty())
        {
            let bucket = daily.entry(day).or_insert((0, 0));
            bucket.0 += 1;
            bucket.1 += session.turn_count;
        }

        for name in parse_models(&session.models_json) {
            if name.is_empty() {
                continue;
            }
            *models.entry(name).or_insert(0) += 1;
        }
        for name in parse_models(&session.sources_json) {
            if name.is_empty() {
                continue;
            }
            *sources.entry(name).or_insert(0) += 1;
        }
        for (name, count) in parse_extensions(&session.extensions_json) {
            if name.is_empty() {
                continue;
            }
            *extensions.entry(name).or_insert(0) += count;
        }
        for (name, count) in parse_tools(&session.tool_calls_json) {
            *tools.entry(name.clone()).or_insert(0) += count;
            *groups.entry(tool_group(&name).to_string()).or_insert(0) += count;
        }
    }

    let tool_call_count: i64 = tools.values().sum();
    let average_tools_per_turn = if turn_count > 0 {
        Some(tool_call_count as f64 / turn_count as f64)
    } else {
        None
    };
    let read_count = *groups.get("read").unwrap_or(&0);
    let write_count = *groups.get("write").unwrap_or(&0);
    let write_read_ratio = if read_count > 0 {
        Some(write_count as f64 / read_count as f64)
    } else {
        None
    };

    let active_project_count = projects.len() as i64;
    let mut by_project: Vec<CursorSessionProjectRow> = projects
        .into_iter()
        .map(|(name, agg)| CursorSessionProjectRow {
            name,
            session_count: agg.session_count,
            turn_count: agg.turn_count,
            error_count: agg.error_count,
            files_touched: agg.files_touched,
            last_seen_at: agg.last_seen_at,
        })
        .collect();
    by_project.sort_by(|a, b| {
        b.session_count
            .cmp(&a.session_count)
            .then_with(|| b.turn_count.cmp(&a.turn_count))
            .then_with(|| a.name.cmp(&b.name))
    });

    let daily = daily
        .into_iter()
        .map(
            |(bucket, (session_count, turn_count))| CursorSessionDailyPoint {
                bucket,
                session_count,
                turn_count,
            },
        )
        .collect();

    let mut by_model: Vec<CursorSessionModelRow> = models
        .into_iter()
        .map(|(name, session_count)| CursorSessionModelRow {
            name,
            session_count,
        })
        .collect();
    by_model.sort_by(|a, b| {
        b.session_count
            .cmp(&a.session_count)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut by_source: Vec<CursorSessionSourceRow> = sources
        .into_iter()
        .map(|(name, session_count)| CursorSessionSourceRow {
            name,
            session_count,
        })
        .collect();
    by_source.sort_by(|a, b| {
        b.session_count
            .cmp(&a.session_count)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut by_extension: Vec<CursorSessionExtensionRow> = extensions
        .into_iter()
        .map(|(name, file_count)| CursorSessionExtensionRow { name, file_count })
        .collect();
    by_extension.sort_by(|a, b| {
        b.file_count
            .cmp(&a.file_count)
            .then_with(|| a.name.cmp(&b.name))
    });
    by_extension.truncate(8);

    let mut top_tools: Vec<CursorSessionToolRow> = tools
        .into_iter()
        .map(|(name, call_count)| CursorSessionToolRow { name, call_count })
        .collect();
    top_tools.sort_by(|a, b| {
        b.call_count
            .cmp(&a.call_count)
            .then_with(|| a.name.cmp(&b.name))
    });
    top_tools.truncate(12);

    let group_order = ["read", "write", "shell", "web", "agent", "other"];
    let tool_groups: Vec<CursorSessionToolRow> = group_order
        .iter()
        .filter_map(|name| {
            let call_count = *groups.get(*name)?;
            if call_count == 0 {
                return None;
            }
            Some(CursorSessionToolRow {
                name: (*name).to_string(),
                call_count,
            })
        })
        .collect();

    CursorSessionSummaryDto {
        as_of: None,
        session_count,
        turn_count,
        aborted_count,
        user_prompt_count,
        subagent_count,
        error_rate,
        average_turns,
        single_prompt_ratio,
        average_tools_per_turn,
        write_read_ratio,
        active_project_count,
        by_project,
        by_model,
        by_source,
        by_extension,
        top_tools,
        tool_groups,
        daily,
    }
}

pub fn sessions_page(
    conn: &Connection,
    query: &CursorSessionQuery,
) -> Result<CursorSessionPage, String> {
    let mut clauses = Vec::new();
    let mut params: Vec<Value> = Vec::new();

    if let Some(project) = query
        .project
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        clauses.push("display_project = ?".to_string());
        params.push(Value::Text(project.to_string()));
    }

    if let Some(search) = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let pattern = format!("%{}%", escape_like(search));
        clauses.push(
            "(session_id LIKE ? ESCAPE '\\' OR display_project LIKE ? ESCAPE '\\'
                OR models_json LIKE ? ESCAPE '\\' OR sources_json LIKE ? ESCAPE '\\'
                OR source_file LIKE ? ESCAPE '\\')"
                .to_string(),
        );
        for _ in 0..5 {
            params.push(Value::Text(pattern.clone()));
        }
    }

    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };

    let sort_column = match query.sort_by.as_deref() {
        Some("session") => "session_id",
        Some("project") => "display_project",
        Some("turns") => "turn_count",
        Some("errors") => "error_count",
        Some("tools") => "tool_call_count",
        Some("files") => "files_touched",
        Some("model") => "models_json",
        _ => "last_seen_at",
    };
    let sort_dir = if query.sort_dir.as_deref() == Some("asc") {
        "ASC"
    } else {
        "DESC"
    };

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 20_000);
    let offset = (page - 1) * page_size;
    params.push(Value::Integer(page_size as i64));
    params.push(Value::Integer(offset as i64));

    let sql = format!(
        "WITH listed AS MATERIALIZED (
            SELECT
                source_file,
                session_id,
                CASE WHEN project = '' THEN '未知项目' ELSE project END AS display_project,
                turn_count,
                success_count,
                error_count,
                aborted_count,
                user_prompt_count,
                subagent_count,
                models_json,
                sources_json,
                first_seen_at,
                last_seen_at,
                files_touched,
                COALESCE((
                    SELECT SUM(CAST(json_each.value AS INTEGER))
                    FROM json_each(tool_calls_json)
                ), 0) AS tool_call_count
            FROM cursor_sessions
        ),
        filtered AS MATERIALIZED (
            SELECT * FROM listed {where_sql}
        ),
        summary AS (
            SELECT COUNT(*) AS match_count FROM filtered
        ),
        page AS (
            SELECT source_file, session_id, display_project, turn_count, success_count,
                error_count, aborted_count, user_prompt_count, subagent_count, models_json,
                sources_json, tool_call_count, first_seen_at, last_seen_at, files_touched
            FROM filtered
            ORDER BY {sort_column} {sort_dir}, session_id ASC
            LIMIT ? OFFSET ?
        )
        SELECT summary.match_count,
            page.source_file, page.session_id, page.display_project, page.turn_count,
            page.success_count, page.error_count, page.aborted_count, page.user_prompt_count,
            page.subagent_count, page.models_json, page.sources_json, page.tool_call_count,
            page.first_seen_at, page.last_seen_at, page.files_touched
        FROM summary
        LEFT JOIN page ON 1"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let raw = stmt
        .query_map(params_from_iter(params.iter()), |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<i64>>(12)?,
                row.get::<_, Option<String>>(13)?,
                row.get::<_, Option<String>>(14)?,
                row.get::<_, Option<i64>>(15)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut total = 0;
    let mut rows = Vec::new();
    for (
        match_count,
        source_file,
        session_id,
        project,
        turn_count,
        success_count,
        error_count,
        aborted_count,
        user_prompt_count,
        subagent_count,
        models_json,
        sources_json,
        tool_call_count,
        first_seen_at,
        last_seen_at,
        files_touched,
    ) in raw
    {
        total = match_count;
        let Some(session_id) = session_id else {
            continue;
        };
        rows.push(CursorSessionListRow {
            session_id,
            project: project.unwrap_or_else(|| "未知项目".to_string()),
            turn_count: turn_count.unwrap_or(0),
            success_count: success_count.unwrap_or(0),
            error_count: error_count.unwrap_or(0),
            aborted_count: aborted_count.unwrap_or(0),
            user_prompt_count: user_prompt_count.unwrap_or(0),
            subagent_count: subagent_count.unwrap_or(0),
            models: parse_models(&models_json.unwrap_or_else(|| "[]".to_string())),
            sources: parse_models(&sources_json.unwrap_or_else(|| "[]".to_string())),
            tool_call_count: tool_call_count.unwrap_or(0),
            first_seen_at,
            last_seen_at,
            files_touched: files_touched.unwrap_or(0),
            source_file: source_file.unwrap_or_default(),
        });
    }

    Ok(CursorSessionPage { rows, total })
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn display_project(project: &str) -> String {
    if project.is_empty() {
        "未知项目".to_string()
    } else {
        project.to_string()
    }
}

fn parse_models(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

fn parse_tools(raw: &str) -> BTreeMap<String, i64> {
    serde_json::from_str::<BTreeMap<String, i64>>(raw).unwrap_or_default()
}

fn parse_extensions(raw: &str) -> BTreeMap<String, i64> {
    serde_json::from_str::<BTreeMap<String, i64>>(raw).unwrap_or_default()
}

fn later_ts(current: &Option<String>, candidate: &Option<String>) -> Option<String> {
    match (current.as_deref(), candidate.as_deref()) {
        (None, None) => None,
        (Some(value), None) => Some(value.to_string()),
        (None, Some(value)) => Some(value.to_string()),
        (Some(left), Some(right)) => {
            let pick_right = match (
                chrono::DateTime::parse_from_rfc3339(left).ok(),
                chrono::DateTime::parse_from_rfc3339(right).ok(),
            ) {
                (Some(left_dt), Some(right_dt)) => right_dt > left_dt,
                _ => right > left,
            };
            Some(if pick_right { right } else { left }.to_string())
        }
    }
}

fn local_day(occurred_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(occurred_at)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|_| occurred_at.get(..10).unwrap_or(occurred_at).to_string())
}
