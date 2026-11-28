//! 事件索引回填与回填进度。
//!
//! 按 `ended_at` 倒序补建未就绪（含待补建）会话的事件索引，调用方可以指定先补哪些会话。
//! 打开对话详情不经过这里。

use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{ConversationIndexProgressDto, Source};

use super::conversation_adapter;
use super::event_store::{
    commit_session, event_index_ready_sql, with_savepoint, SessionCommit, SessionEventStager,
};
use super::session_store::load_session;
use super::trusted_path::trusted_paths_for_session;
use super::CONVERSATION_SOURCES;

/// 补建只管有源文件、有对话内容的会话；只有消耗记录的 Cursor 会话没有事件可建。
const BACKFILL_CANDIDATE_SQL: &str =
    "sessions.file_available = 1 AND sessions.source_revision != 'usage-only'";

pub fn event_index_progress(conn: &Connection) -> Result<ConversationIndexProgressDto, String> {
    let ready_sql = event_index_ready_sql("sessions");
    let (total, indexed) = conn
        .query_row(
            &format!(
                r#"
                SELECT COUNT(*) AS total, COALESCE(SUM({ready_sql}), 0) AS indexed
                FROM conversation_sessions AS sessions
                WHERE {BACKFILL_CANDIDATE_SQL}
                "#
            ),
            [],
            |row| Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u32)),
        )
        .map_err(|error| error.to_string())?;
    Ok(ConversationIndexProgressDto {
        indexed,
        total,
        index_bytes: crate::store::stored_conversation_index_bytes(conn),
    })
}

/// 会话在补建范围内且事件未就绪（含待补建）时为 `true`；会话不存在、已就绪、
/// 源文件已不在或只有消耗记录时为 `false`。
pub fn session_needs_event_backfill(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<bool, String> {
    let ready_sql = event_index_ready_sql("sessions");
    let ready = conn
        .query_row(
            &format!(
                r#"
                SELECT {ready_sql}
                FROM conversation_sessions AS sessions
                WHERE sessions.source = ?1 AND sessions.session_id = ?2
                  AND {BACKFILL_CANDIDATE_SQL}
                "#
            ),
            params![source, session_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(ready == Some(0))
}

pub fn backfill_event_index_step(conn: &Connection, home: &Path) -> Result<bool, String> {
    match backfill_event_index_step_skipping(conn, home, &BTreeSet::new()) {
        Ok(progressed) => Ok(progressed),
        Err((_, error)) => Err(error),
    }
}

fn backfill_event_index_step_skipping(
    conn: &Connection,
    home: &Path,
    skipped: &BTreeSet<(String, String)>,
) -> Result<bool, ((String, String), String)> {
    backfill_event_index_step_prioritized(conn, home, skipped, &[])
}

/// 补建一个会话：先取 `priority` 里第一个仍需补建且不在 `skipped` 的会话，
/// 都没有再按 `ended_at` 倒序取。没有可补的会话时返回 `Ok(false)`。
pub fn backfill_event_index_step_prioritized(
    conn: &Connection,
    home: &Path,
    skipped: &BTreeSet<(String, String)>,
    priority: &[(String, String)],
) -> Result<bool, ((String, String), String)> {
    let next = next_backfill_session(conn, skipped, priority)
        .map_err(|error| ((String::new(), String::new()), error))?;
    let Some((source, session_id)) = next else {
        return Ok(false);
    };
    match reindex_session_events(conn, home, &source, &session_id) {
        Ok(()) => Ok(true),
        Err(error) => Err(((source, session_id), error)),
    }
}

pub fn backfill_event_index(conn: &Connection, home: &Path) -> Result<u32, String> {
    let mut completed = 0;
    let mut skipped = BTreeSet::new();
    while let Some((source, session_id)) = next_backfill_session(conn, &skipped, &[])? {
        match reindex_session_events(conn, home, &source, &session_id) {
            Ok(()) => completed += 1,
            Err(_) => {
                skipped.insert((source, session_id));
            }
        }
    }
    Ok(completed)
}

fn next_backfill_session(
    conn: &Connection,
    skipped: &BTreeSet<(String, String)>,
    priority: &[(String, String)],
) -> Result<Option<(String, String)>, String> {
    for key in priority {
        if !skipped.contains(key) && session_needs_event_backfill(conn, &key.0, &key.1)? {
            return Ok(Some(key.clone()));
        }
    }
    next_unready_session(conn, skipped)
}

fn next_unready_session(
    conn: &Connection,
    skipped: &BTreeSet<(String, String)>,
) -> Result<Option<(String, String)>, String> {
    let ready_sql = event_index_ready_sql("sessions");
    let mut statement = conn
        .prepare(&format!(
            r#"
            SELECT sessions.source, sessions.session_id
            FROM conversation_sessions AS sessions
            WHERE {BACKFILL_CANDIDATE_SQL} AND {ready_sql} = 0
            ORDER BY sessions.ended_at DESC, sessions.source ASC, sessions.session_id ASC
            "#
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows.into_iter().find(|key| !skipped.contains(key)))
}

fn reindex_session_events(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
) -> Result<(), String> {
    let source = Source::parse(source).filter(|source| CONVERSATION_SOURCES.contains(source));
    let Some(source) = source else {
        return Err("该来源尚未支持对话详情".to_string());
    };
    // 包进单个原子区间：新代写入、目录提交与旧代删除一起提交，不再逐语句产生 FTS 小段（ADR 0024）。
    with_savepoint(conn, "event_store_backfill", |tx| {
        let session = load_session(tx, source.as_str(), session_id)?
            .ok_or_else(|| "未找到该对话记录".to_string())?;
        let paths = trusted_paths_for_session(home, source, &session)?;
        let adapter = conversation_adapter(source)?;
        let mut stager = SessionEventStager::default();
        for path in &paths {
            let batch = (adapter.index)(path).map_err(|issue| issue.message)?;
            for parsed in batch.conversations {
                if parsed.session.session_id != session_id {
                    continue;
                }
                stager.stage_full_parse(tx, source, parsed)?;
            }
        }
        let Some(indexed_files) = stager.grouped.remove(session_id) else {
            return Err(format!("会话 {session_id} 的源文件没有可索引的对话"));
        };
        commit_session(
            tx,
            SessionCommit {
                source,
                session_id,
                indexed_files,
                replace_files: true,
                publish_generation: stager.generation_for(session_id),
                file_cursors: stager.file_cursors(),
                mark_dirty: false,
            },
        )
    })
}
