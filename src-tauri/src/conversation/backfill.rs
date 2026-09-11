//! 事件索引回填与回填进度。
//!
//! 按 `ended_at` 倒序补建未就绪会话的事件索引。打开对话详情不经过这里。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::domain::{ConversationIndexProgressDto, Source};

use super::conversation_adapter;
use super::event_index;
use super::merge::{merge_indexed_files, summarize_for_index};
use super::persist::{persist_session_file_cursors, write_session_file_events};
use super::session_store::{load_session, update_session_files, upsert_session};
use super::trusted_path::{modified_nanos, trusted_paths_for_session};
use super::{CONVERSATION_ADAPTER_VERSION, CONVERSATION_SOURCES};

pub fn event_index_progress(conn: &Connection) -> Result<ConversationIndexProgressDto, String> {
    let (total, indexed) = conn
        .query_row(
            r#"
            SELECT
                COUNT(*) AS total,
                COALESCE(SUM(
                    CASE
                        WHEN adapter_version = ?1 AND event_index_generation IS NOT NULL THEN 1
                        ELSE 0
                    END
                ), 0) AS indexed
            FROM conversation_sessions
            WHERE file_available = 1 AND source_revision != 'usage-only'
            "#,
            params![CONVERSATION_ADAPTER_VERSION],
            |row| Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u32)),
        )
        .map_err(|error| error.to_string())?;
    Ok(ConversationIndexProgressDto {
        indexed,
        total,
        index_bytes: conversation_index_bytes(conn, indexed >= total || total == 0),
    })
}

fn conversation_index_bytes(conn: &Connection, complete: bool) -> u64 {
    if let Ok(bytes) = conn.query_row(
        "SELECT COALESCE(SUM(pgsize), 0) FROM dbstat
         WHERE name GLOB 'conversation_events*'
            OR name IN ('conversation_files', 'conversation_session_tools')",
        [],
        |row| row.get::<_, i64>(0),
    ) {
        return bytes.max(0) as u64;
    }
    if !complete {
        return 0;
    }
    conn.query_row(
        "SELECT COALESCE(SUM(LENGTH(COALESCE(text, '')) + LENGTH(COALESCE(name, ''))), 0)
         FROM conversation_events",
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
    .max(0) as u64
}

pub fn backfill_event_index_step(conn: &Connection, home: &Path) -> Result<bool, String> {
    match backfill_event_index_step_skipping(conn, home, &BTreeSet::new()) {
        Ok(progressed) => Ok(progressed),
        Err((_, error)) => Err(error),
    }
}

pub(crate) fn backfill_event_index_step_skipping(
    conn: &Connection,
    home: &Path,
    skipped: &BTreeSet<(String, String)>,
) -> Result<bool, ((String, String), String)> {
    let next = next_unready_session(conn, skipped)
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
    while let Some((source, session_id)) = next_unready_session(conn, &skipped)? {
        match reindex_session_events(conn, home, &source, &session_id) {
            Ok(()) => completed += 1,
            Err(_) => {
                skipped.insert((source, session_id));
            }
        }
    }
    Ok(completed)
}

fn next_unready_session(
    conn: &Connection,
    skipped: &BTreeSet<(String, String)>,
) -> Result<Option<(String, String)>, String> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT source, session_id
            FROM conversation_sessions
            WHERE file_available = 1
              AND source_revision != 'usage-only'
              AND (adapter_version != ?1 OR event_index_generation IS NULL)
            ORDER BY ended_at DESC, source ASC, session_id ASC
            "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![CONVERSATION_ADAPTER_VERSION], |row| {
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
    let session = load_session(conn, source.as_str(), session_id)?
        .ok_or_else(|| "未找到该对话记录".to_string())?;
    let paths = trusted_paths_for_session(home, source, &session)?;
    let adapter = conversation_adapter(source)?;
    let mut event_generations = BTreeMap::new();
    let mut indexed_files = Vec::new();
    let mut file_cursors = BTreeMap::new();
    for path in &paths {
        let batch = (adapter.index)(path).map_err(|issue| issue.message)?;
        for parsed in batch.conversations {
            if parsed.session.session_id != session_id {
                continue;
            }
            if let Some(cursor) = parsed.index_cursor {
                file_cursors.insert(
                    (session_id.to_string(), parsed.session.source_file.clone()),
                    cursor,
                );
            }
            write_session_file_events(conn, source, &parsed, &mut event_generations)?;
            indexed_files.push(summarize_for_index(parsed));
        }
    }
    if indexed_files.is_empty() {
        return Err(format!("会话 {session_id} 的源文件没有可索引的对话"));
    }
    let source_files = indexed_files
        .iter()
        .map(|file| PathBuf::from(&file.session.source_file))
        .collect::<Vec<_>>();
    let (merged_session, is_top_level, agent_metadata) = merge_indexed_files(indexed_files);
    let representative_metadata = fs::metadata(&merged_session.source_file)
        .map_err(|error| format!("读取文件元数据失败：{error}"))?;
    let representative_revision = (adapter.revision)(Path::new(&merged_session.source_file))?;
    if let Some(&generation) = event_generations.get(session_id) {
        event_index::finalize_session_events(conn, source, session_id, generation)?;
    }
    upsert_session(
        conn,
        &merged_session,
        is_top_level,
        &agent_metadata,
        modified_nanos(&representative_metadata),
        representative_metadata.len() as i64,
        &representative_revision,
    )?;
    update_session_files(conn, source, session_id, &source_files, true)?;
    persist_session_file_cursors(conn, source, session_id, &source_files, &file_cursors)?;
    Ok(())
}
