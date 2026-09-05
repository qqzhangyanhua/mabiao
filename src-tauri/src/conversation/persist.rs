use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::Source;

use super::conversation_adapter;
use super::event_index;
use super::merge::{summarize_for_index, IndexedFile};
use super::modified_nanos;
use super::toolbox::{FileIndexCursor, ParsedConversation};
use super::CachedConversationFingerprint;
use super::ConversationIndexSuffixFn;
use super::SessionFileCursorWrite;
use super::CONVERSATION_ADAPTER_VERSION;

pub(crate) enum IncrementalPrepare {
    Ready(Box<ParsedConversation>),
    NeedFull,
}

pub(crate) struct PendingIncremental {
    pub(crate) path: PathBuf,
    pub(crate) session_id: String,
    pub(crate) parsed: ParsedConversation,
    pub(crate) mtime_ns: i64,
    pub(crate) size: i64,
    pub(crate) source_revision: String,
}

pub(crate) fn record_full_parse(
    conn: &Connection,
    source: Source,
    parsed: ParsedConversation,
    event_generations: &mut BTreeMap<String, i64>,
    grouped: &mut BTreeMap<String, Vec<IndexedFile>>,
    file_cursors: &mut BTreeMap<(String, String), FileIndexCursor>,
) -> Result<(), String> {
    if let Some(cursor) = parsed.index_cursor {
        file_cursors.insert(
            (
                parsed.session.session_id.clone(),
                parsed.session.source_file.clone(),
            ),
            cursor,
        );
    }
    write_session_file_events(conn, source, &parsed, event_generations)?;
    grouped
        .entry(parsed.session.session_id.clone())
        .or_default()
        .push(summarize_for_index(parsed));
    Ok(())
}

pub(crate) fn prepare_incremental(
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
    if !event_index::has_live_generation(conn, source, &cached.session_id)? {
        return Ok(IncrementalPrepare::NeedFull);
    }
    if event_index::live_index_would_rewind(conn, source, &cached.session_id, &parsed.events)? {
        return Ok(IncrementalPrepare::NeedFull);
    }
    Ok(IncrementalPrepare::Ready(Box::new(parsed)))
}

pub(crate) fn apply_incremental(
    conn: &Connection,
    source: Source,
    pending: PendingIncremental,
) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let result = apply_incremental_in_tx(&tx, source, pending);
    match result {
        Ok(()) => tx.commit().map_err(|error| error.to_string()),
        Err(error) => {
            let _ = tx.rollback();
            Err(error)
        }
    }
}

pub(crate) fn apply_incremental_in_tx(
    conn: &Connection,
    source: Source,
    pending: PendingIncremental,
) -> Result<(), String> {
    let PendingIncremental {
        path,
        session_id,
        parsed,
        mtime_ns,
        size,
        source_revision,
    } = pending;
    let max_sequence = event_index::append_live_events(conn, source, &session_id, &parsed.events)?;
    let cursor = parsed.index_cursor.unwrap_or(FileIndexCursor {
        byte_offset: size,
        line: 0,
    });
    persist_file_cursor(
        conn,
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
        conn,
        source,
        &session_id,
        &parsed,
        mtime_ns,
        size,
        &source_revision,
    )
}

pub(crate) fn touch_session_after_append(
    conn: &Connection,
    source: Source,
    session_id: &str,
    parsed: &ParsedConversation,
    mtime_ns: i64,
    size: i64,
    source_revision: &str,
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
            file_available = 1
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
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn persist_session_file_cursors(
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

pub(crate) fn persist_file_cursor(
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

pub(crate) fn write_session_file_events(
    conn: &Connection,
    source: Source,
    parsed: &ParsedConversation,
    generations: &mut BTreeMap<String, i64>,
) -> Result<(), String> {
    event_index::write_file_events(conn, source, parsed, generations)
}
