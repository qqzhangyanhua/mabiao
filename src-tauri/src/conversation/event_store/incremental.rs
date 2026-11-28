//! 后缀增量追加与文件索引游标落盘。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{ConversationEvent, Source};

use super::super::incremental::new_events_precede_existing;
use super::super::normalize::{FileIndexCursor, ParsedConversation};
use super::super::trusted_path::modified_nanos;
use super::super::{
    conversation_adapter, CachedConversationFingerprint, ConversationIndexSuffixFn,
    SessionFileCursorWrite, CONVERSATION_ADAPTER_VERSION,
};
use super::readiness::{has_live_generation, session_generation};
use super::tables::refresh_session_tools;
use super::write::{insert_events, with_savepoint, EventBatch};

pub(in crate::conversation) enum IncrementalPrepare {
    Ready(Box<ParsedConversation>),
    NeedFull,
}

pub(in crate::conversation) struct PendingIncremental {
    pub(in crate::conversation) path: PathBuf,
    pub(in crate::conversation) session_id: String,
    pub(in crate::conversation) parsed: ParsedConversation,
    /// 后缀解析的起点。
    pub(in crate::conversation) origin: FileIndexCursor,
    pub(in crate::conversation) mtime_ns: i64,
    pub(in crate::conversation) size: i64,
    pub(in crate::conversation) source_revision: String,
}

pub(in crate::conversation) fn prepare_incremental(
    conn: &Connection,
    source: Source,
    index_suffix: ConversationIndexSuffixFn,
    path: &Path,
    cached: &CachedConversationFingerprint,
) -> Result<IncrementalPrepare, String> {
    let parsed = match index_suffix(
        path,
        cached.indexed_byte_offset as u64,
        cached.indexed_line as u32,
        &cached.session_id,
    ) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(IncrementalPrepare::NeedFull),
    };
    if parsed.session.session_id != cached.session_id {
        return Ok(IncrementalPrepare::NeedFull);
    }
    if !has_live_generation(conn, source, &cached.session_id)? {
        return Ok(IncrementalPrepare::NeedFull);
    }
    if live_index_would_rewind(conn, source, &cached.session_id, &parsed.events)? {
        return Ok(IncrementalPrepare::NeedFull);
    }
    Ok(IncrementalPrepare::Ready(Box::new(parsed)))
}

/// 在 SAVEPOINT 内原子完成后缀增量追加：插事件 + 外置正文 + FTS + 工具汇总 + 游标 + 会话行（含 `event_count`）。
/// 兼容独立连接与外层已有事务（如 `ingest::refresh_conversation_catalogs`）。
pub(in crate::conversation) fn commit_incremental_append(
    conn: &Connection,
    source: Source,
    pending: PendingIncremental,
) -> Result<(), String> {
    with_savepoint(conn, "event_store_incremental_append", |tx| {
        let PendingIncremental {
            path,
            session_id,
            parsed,
            origin,
            mtime_ns,
            size,
            source_revision,
        } = pending;
        let (max_sequence, total_event_count) =
            append_live_events(tx, source, &session_id, &parsed.events, origin)?;
        let cursor = parsed.index_cursor.unwrap_or(FileIndexCursor {
            byte_offset: size,
            line: 0,
        });
        persist_file_cursor(
            tx,
            source,
            &session_id,
            &SessionFileCursorWrite {
                path: &path,
                cursor,
                max_sequence: Some(max_sequence),
                mtime_ns,
                size,
                source_revision: &source_revision,
            },
        )?;
        touch_session_after_append(
            tx,
            source,
            &session_id,
            &parsed,
            mtime_ns,
            size,
            &source_revision,
            total_event_count,
        )
    })
}

pub(super) fn persist_session_file_cursors(
    conn: &Connection,
    source: Source,
    session_id: &str,
    paths: &[PathBuf],
    file_cursors: &BTreeMap<(String, String), FileIndexCursor>,
) -> Result<(), String> {
    let max_sequence = conn
        .query_row(
            r#"
            SELECT MAX(sequence) FROM conversation_events
            WHERE source = ?1 AND session_id = ?2
              AND index_generation = (
                  SELECT event_index_generation FROM conversation_sessions
                  WHERE source = ?1 AND session_id = ?2
              )
            "#,
            params![source.as_str(), session_id],
            |row| row.get::<_, Option<u32>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    for path in paths {
        let metadata =
            fs::metadata(path).map_err(|error| format!("读取文件元数据失败：{error}"))?;
        let revision = (conversation_adapter(source)?.revision)(path)?;
        let cursor = file_cursors
            .get(&(session_id.to_string(), path.to_string_lossy().to_string()))
            .copied()
            .unwrap_or(FileIndexCursor {
                byte_offset: metadata.len() as i64,
                line: 0,
            });
        persist_file_cursor(
            conn,
            source,
            session_id,
            &SessionFileCursorWrite {
                path,
                cursor,
                max_sequence,
                mtime_ns: modified_nanos(&metadata),
                size: metadata.len() as i64,
                source_revision: &revision,
            },
        )?;
    }
    Ok(())
}

/// `origin` 是这批后缀事件在源文件里被解析的起点，外置正文要据此算行偏移。
/// 返回 `(最大序号, 该代次总事件数)`。
fn append_live_events(
    conn: &Connection,
    source: Source,
    session_id: &str,
    events: &[ConversationEvent],
    origin: FileIndexCursor,
) -> Result<(u32, u32), String> {
    if live_index_would_rewind(conn, source, session_id, events)? {
        return Err("新事件时间早于已有索引，需要整份重索引".to_string());
    }
    let Some(generation) = session_generation(conn, source.as_str(), session_id)? else {
        return Err("会话还没有已发布的事件索引".to_string());
    };
    let first_sequence = max_sequence(conn, source.as_str(), session_id, generation)?
        .map(|sequence| sequence + 1)
        .unwrap_or(0);
    let mut next_occurrences = identity_occurrences(conn, source.as_str(), session_id, generation)?
        .into_iter()
        .map(|(identity, occurrence)| (identity, occurrence + 1))
        .collect();
    insert_events(
        conn,
        EventBatch {
            source,
            session_id,
            generation,
            events,
            origin,
            first_sequence: Some(first_sequence),
        },
        &mut next_occurrences,
    )?;
    refresh_session_tools(conn, source.as_str(), session_id, generation)?;
    let total_count = conn
        .query_row(
            r#"
            SELECT COUNT(*)
            FROM conversation_events
            WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
            "#,
            params![source.as_str(), session_id, generation],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count.max(0) as u32)
        .map_err(|error| error.to_string())?;
    let max_seq = (first_sequence + events.len() as u32).saturating_sub(1);
    Ok((max_seq, total_count))
}

#[allow(clippy::too_many_arguments)]
fn touch_session_after_append(
    conn: &Connection,
    source: Source,
    session_id: &str,
    parsed: &ParsedConversation,
    mtime_ns: i64,
    size: i64,
    source_revision: &str,
    event_count: u32,
) -> Result<(), String> {
    let ended_at = parsed.session.ended_at.as_str();
    conn.execute(
        r#"
        UPDATE conversation_sessions
        SET ended_at = CASE
                WHEN ?3 != '' AND (ended_at = '' OR ended_at < ?3) THEN ?3
                ELSE ended_at
            END,
            model = CASE WHEN ?4 != '' THEN ?4 ELSE model END,
            source_file_mtime_ns = ?5,
            source_file_size = ?6,
            source_revision = ?7,
            file_available = 1,
            event_count = ?8
        WHERE source = ?1 AND session_id = ?2
        "#,
        params![
            source.as_str(),
            session_id,
            ended_at,
            parsed.session.model,
            mtime_ns,
            size,
            source_revision,
            i64::from(event_count),
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn persist_file_cursor(
    conn: &Connection,
    source: Source,
    session_id: &str,
    write: &SessionFileCursorWrite<'_>,
) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO conversation_session_files(
            source, session_id, source_file, source_file_mtime_ns, source_file_size,
            adapter_version, source_revision, indexed_byte_offset, indexed_line, max_sequence
        ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ON CONFLICT(source, session_id, source_file) DO UPDATE SET
            source_file_mtime_ns = excluded.source_file_mtime_ns,
            source_file_size = excluded.source_file_size,
            adapter_version = excluded.adapter_version,
            source_revision = excluded.source_revision,
            indexed_byte_offset = excluded.indexed_byte_offset,
            indexed_line = excluded.indexed_line,
            max_sequence = excluded.max_sequence
        "#,
        params![
            source.as_str(),
            session_id,
            write.path.to_string_lossy().to_string(),
            write.mtime_ns,
            write.size,
            CONVERSATION_ADAPTER_VERSION,
            write.source_revision,
            write.cursor.byte_offset,
            write.cursor.line,
            write.max_sequence,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn live_index_would_rewind(
    conn: &Connection,
    source: Source,
    session_id: &str,
    events: &[ConversationEvent],
) -> Result<bool, String> {
    let Some(generation) = session_generation(conn, source.as_str(), session_id)? else {
        return Ok(false);
    };
    let max_occurred_at = max_occurred_at(conn, source.as_str(), session_id, generation)?;
    Ok(new_events_precede_existing(
        max_occurred_at.as_deref(),
        events.iter().map(|event| event.occurred_at.clone()),
    ))
}

fn max_sequence(
    conn: &Connection,
    source: &str,
    session_id: &str,
    generation: i64,
) -> Result<Option<u32>, String> {
    conn.query_row(
        r#"
        SELECT MAX(sequence) FROM conversation_events
        WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
        "#,
        params![source, session_id, generation],
        |row| row.get::<_, Option<u32>>(0),
    )
    .map_err(|error| error.to_string())
}

fn max_occurred_at(
    conn: &Connection,
    source: &str,
    session_id: &str,
    generation: i64,
) -> Result<Option<String>, String> {
    conn.query_row(
        r#"
        SELECT occurred_at FROM conversation_events
        WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
          AND occurred_at IS NOT NULL
        ORDER BY occurred_at_sort DESC
        LIMIT 1
        "#,
        params![source, session_id, generation],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map(Option::flatten)
    .map_err(|error| error.to_string())
}

fn identity_occurrences(
    conn: &Connection,
    source: &str,
    session_id: &str,
    generation: i64,
) -> Result<BTreeMap<String, i64>, String> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT identity_hash, MAX(identity_occurrence)
            FROM conversation_events
            WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
            GROUP BY identity_hash
            "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![source, session_id, generation], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}
