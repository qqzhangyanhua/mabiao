//! 对话事件索引就绪态与代次判定的唯一实现。
//!
//! 详情、分页、单事件展开、导出、工作纪要、推送、目录标志、补建进度全部共用这里：
//! 1. 库内条件：`adapter_version = CONVERSATION_ADAPTER_VERSION`、`event_index_generation IS NOT NULL`、
//!    `event_index_dirty = 0`；
//! 2. 完整就绪（读事件时）：库内条件 + 可信源文件路径全部在白名单且存在 + 磁盘修订号与库内一致。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{ConversationSessionRow, Source};

use super::super::trusted_path::{files_revision, trusted_paths_for_session};
use super::super::{
    conversation_adapter, PreparedConversationDetail, CONVERSATION_ADAPTER_VERSION,
};

/// 事件索引「就绪」的唯一 SQL 谓词：详情、推送、补建进度、目录行标志都用它。
/// 待补建会话仍留着旧一代事件，正文搜索与工具筛选照常按 `event_index_generation` 连它。
pub(in crate::conversation) fn event_index_ready_sql(alias: &str) -> String {
    format!(
        "CASE WHEN {alias}.adapter_version = {CONVERSATION_ADAPTER_VERSION} \
         AND {alias}.event_index_generation IS NOT NULL \
         AND {alias}.event_index_dirty = 0 THEN 1 ELSE 0 END"
    )
}

/// 仅检查库内就绪条件（不碰磁盘）。会话不存在时为 `false`。
pub(in crate::conversation) fn session_events_ready(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<bool, String> {
    let ready_sql = event_index_ready_sql("sessions");
    let ready = conn
        .query_row(
            &format!(
                "SELECT {ready_sql} FROM conversation_sessions AS sessions \
                 WHERE sessions.source = ?1 AND sessions.session_id = ?2"
            ),
            params![source, session_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(ready == Some(1))
}

/// 完整就绪判定：库内就绪条件 + 可信源文件存在 + 磁盘修订号与库内登记一致。
pub(in crate::conversation) fn is_session_ready(
    conn: &Connection,
    home: &Path,
    source: Source,
    session: &ConversationSessionRow,
) -> Result<bool, String> {
    if !session_events_ready(conn, source.as_str(), &session.session_id)? {
        return Ok(false);
    }
    let Ok(paths) = trusted_paths_for_session(home, source, session) else {
        return Ok(false);
    };
    stored_revisions_match(conn, source, &session.session_id, &paths)
}

pub(crate) fn event_index_ready(
    conn: &Connection,
    home: &Path,
    prepared: &PreparedConversationDetail,
) -> Result<bool, String> {
    is_session_ready(conn, home, prepared.source, &prepared.session)
}

pub(in crate::conversation) fn stored_revisions_match(
    conn: &Connection,
    source: Source,
    session_id: &str,
    paths: &[PathBuf],
) -> Result<bool, String> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT source_file, source_revision
            FROM conversation_session_files
            WHERE source = ?1 AND session_id = ?2
            "#,
        )
        .map_err(|error| error.to_string())?;
    let stored = statement
        .query_map(params![source.as_str(), session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|error| error.to_string())?;
    let adapter = conversation_adapter(source)?;
    if stored.is_empty() {
        let stored_revision = conn
            .query_row(
                r#"
                SELECT source_revision
                FROM conversation_sessions
                WHERE source = ?1 AND session_id = ?2
                "#,
                params![source.as_str(), session_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| error.to_string())?;
        return Ok(files_revision(source, paths)? == stored_revision);
    }
    if stored.len() != paths.len() {
        return Ok(false);
    }
    for path in paths {
        let key = path.to_string_lossy().to_string();
        let Some(stored_revision) = stored.get(&key) else {
            return Ok(false);
        };
        if &(adapter.revision)(path)? != stored_revision {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(in crate::conversation) fn session_generation(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Option<i64>, String> {
    conn.query_row(
        r#"
        SELECT event_index_generation
        FROM conversation_sessions
        WHERE source = ?1 AND session_id = ?2
        "#,
        params![source, session_id],
        |row| row.get::<_, Option<i64>>(0),
    )
    .optional()
    .map(Option::flatten)
    .map_err(|error| error.to_string())
}

pub(in crate::conversation) fn has_live_generation(
    conn: &Connection,
    source: Source,
    session_id: &str,
) -> Result<bool, String> {
    Ok(session_generation(conn, source.as_str(), session_id)?.is_some())
}

pub(in crate::conversation) fn mark_session_events_dirty(
    conn: &Connection,
    source: Source,
    session_id: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE conversation_sessions SET event_index_dirty = 1 WHERE source = ?1 AND session_id = ?2",
        params![source.as_str(), session_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}
