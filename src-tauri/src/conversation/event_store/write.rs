//! 会话事件与目录行的原子写入路径。
//!
//! 统一全量刷新、未变子集重解析、目录待补建刷新与后台补建的暂存与提交顺序：
//! 事件、ADR-0025 外置正文位置、推导 `event_id`、无正文倒排、工具汇总、`conversation_sessions`
//! （含 `event_count`）与 `conversation_session_files` 游标始终在 SAVEPOINT 内按同一顺序提交。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;

use crate::domain::{ConversationEvent, Source};

use super::super::conversation_adapter;
use super::super::merge::{
    event_identity_hash, format_identity_hash, merge_indexed_files, summarize_for_index,
    IndexedFile,
};
use super::super::normalize::{compare_optional_timestamps, FileIndexCursor, ParsedConversation};
use super::super::session_store::{update_session_files, upsert_session, SessionFileMetadata};
use super::super::trusted_path::modified_nanos;
use super::incremental::persist_session_file_cursors;
use super::readiness::{mark_session_events_dirty, session_generation};
use super::storage::{plan_text_refs, restore_event_id, stored_event_id};
use super::tables::{clear_session_tools, refresh_session_tools, FileIds};

/// 扫描或补建期间的暂存器：负责写入新代事件并收集会话文件摘要与游标，
/// 统一全量刷新、未变子集重解析与后台补建三条暂存路径。
#[derive(Default)]
pub(in crate::conversation) struct SessionEventStager {
    pub(in crate::conversation) grouped: BTreeMap<String, Vec<IndexedFile>>,
    event_generations: BTreeMap<String, i64>,
    file_cursors: BTreeMap<(String, String), FileIndexCursor>,
}

impl SessionEventStager {
    pub(in crate::conversation) fn stage_full_parse(
        &mut self,
        conn: &Connection,
        source: Source,
        parsed: ParsedConversation,
    ) -> Result<(), String> {
        if let Some(cursor) = parsed.index_cursor {
            self.file_cursors.insert(
                (
                    parsed.session.session_id.clone(),
                    parsed.session.source_file.clone(),
                ),
                cursor,
            );
        }
        write_file_events(conn, source, &parsed, &mut self.event_generations)?;
        self.grouped
            .entry(parsed.session.session_id.clone())
            .or_default()
            .push(summarize_for_index(parsed));
        Ok(())
    }

    /// 只留会话摘要（含去重所需事件身份指纹），不写事件，也不记事件游标。
    pub(in crate::conversation) fn stage_catalog_parse(&mut self, parsed: ParsedConversation) {
        self.grouped
            .entry(parsed.session.session_id.clone())
            .or_default()
            .push(summarize_for_index(parsed));
    }

    pub(in crate::conversation) fn generation_for(&self, session_id: &str) -> Option<i64> {
        self.event_generations.get(session_id).copied()
    }

    pub(in crate::conversation) fn file_cursors(
        &self,
    ) -> &BTreeMap<(String, String), FileIndexCursor> {
        &self.file_cursors
    }
}

pub(in crate::conversation) struct SessionCommit<'a> {
    pub(in crate::conversation) source: Source,
    pub(in crate::conversation) session_id: &'a str,
    pub(in crate::conversation) indexed_files: Vec<IndexedFile>,
    pub(in crate::conversation) replace_files: bool,
    pub(in crate::conversation) publish_generation: Option<i64>,
    pub(in crate::conversation) file_cursors: &'a BTreeMap<(String, String), FileIndexCursor>,
    pub(in crate::conversation) mark_dirty: bool,
}

/// 原子提交一条会话的目录行、文件清单，以及（可选）已暂存的事件代次与游标。
///
/// 统一全量刷新、目录待补建刷新与后台补建的写入顺序：
/// 1. 合并跨文件会话摘要并计算精确 `event_count`
/// 2. `upsert_session`（含 `event_count`）
/// 3. `update_session_files`
/// 4. 待补建模式标记 `event_index_dirty = 1`；全量/补建模式发布新代事件（去重、排序、更新
///    `event_index_generation` / `event_index_dirty = 0` / `event_count`、清理旧代、刷新工具汇总）
///    并落盘文件游标
pub(in crate::conversation) fn commit_session(
    conn: &Connection,
    commit: SessionCommit<'_>,
) -> Result<(), String> {
    let SessionCommit {
        source,
        session_id,
        indexed_files,
        replace_files,
        publish_generation,
        file_cursors,
        mark_dirty,
    } = commit;
    let source_files = indexed_files
        .iter()
        .map(|file| PathBuf::from(&file.session.source_file))
        .collect::<Vec<_>>();
    let merged = merge_indexed_files(indexed_files);
    let representative_metadata = fs::metadata(&merged.session.source_file)
        .map_err(|error| format!("读取文件元数据失败：{error}"))?;
    let representative_revision =
        (conversation_adapter(source)?.revision)(Path::new(&merged.session.source_file))?;
    with_savepoint(conn, "event_store_session_commit", |tx| {
        upsert_session(
            tx,
            &merged.session,
            merged.is_top_level,
            &merged.agent_metadata,
            SessionFileMetadata {
                mtime_ns: modified_nanos(&representative_metadata),
                size: representative_metadata.len() as i64,
                source_revision: &representative_revision,
                event_count: Some(merged.event_count),
            },
        )?;
        update_session_files(tx, source, session_id, &source_files, replace_files)?;
        if mark_dirty {
            mark_session_events_dirty(tx, source, session_id)?;
        }
        if let Some(generation) = publish_generation {
            finalize_session_events(tx, source, session_id, generation)?;
            persist_session_file_cursors(tx, source, session_id, &source_files, file_cursors)?;
        }
        Ok(())
    })
}

pub(in crate::conversation) fn clear_session_events(
    conn: &Connection,
    source: Source,
    session_id: &str,
) -> Result<(), String> {
    conn.execute(
        "DELETE FROM conversation_events WHERE source = ?1 AND session_id = ?2",
        params![source.as_str(), session_id],
    )
    .map_err(|error| error.to_string())?;
    clear_session_tools(conn, source.as_str(), session_id, None)?;
    conn.execute(
        r#"
        UPDATE conversation_sessions
        SET event_index_generation = NULL, event_count = 0
        WHERE source = ?1 AND session_id = ?2
        "#,
        params![source.as_str(), session_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// 独立或嵌套事务均可安全使用的 SQLite `SAVEPOINT` 原子执行器。
pub(in crate::conversation) fn with_savepoint<T>(
    conn: &Connection,
    name: &str,
    action: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    conn.execute_batch(&format!("SAVEPOINT {name};"))
        .map_err(|error| error.to_string())?;
    struct Guard<'a> {
        conn: &'a Connection,
        name: &'a str,
        committed: bool,
    }
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            if !self.committed {
                let _ = self.conn.execute_batch(&format!(
                    "ROLLBACK TO SAVEPOINT {0}; RELEASE SAVEPOINT {0};",
                    self.name
                ));
            }
        }
    }
    let mut guard = Guard {
        conn,
        name,
        committed: false,
    };
    let value = action(guard.conn)?;
    guard
        .conn
        .execute_batch(&format!("RELEASE SAVEPOINT {name};"))
        .map_err(|error| error.to_string())?;
    guard.committed = true;
    Ok(value)
}

pub(super) struct EventBatch<'a> {
    pub(super) source: Source,
    pub(super) session_id: &'a str,
    pub(super) generation: i64,
    pub(super) events: &'a [ConversationEvent],
    pub(super) origin: FileIndexCursor,
    /// 整份写入时为空，由 `finalize_session_events` 统一排序后再编号。
    pub(super) first_sequence: Option<u32>,
}

pub(super) fn insert_events(
    conn: &Connection,
    batch: EventBatch<'_>,
    next_occurrences: &mut BTreeMap<String, i64>,
) -> Result<(), String> {
    let EventBatch {
        source,
        session_id,
        generation,
        events,
        origin,
        first_sequence,
    } = batch;
    let contentless_fts = crate::store::conversation_fts_is_contentless(conn)?;
    let text_refs = if contentless_fts {
        plan_text_refs(source, session_id, events, origin)
    } else {
        vec![None; events.len()]
    };
    let insert_sql = format!(
        "INSERT INTO conversation_events({columns}) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        columns = crate::store::CONVERSATION_EVENT_COLUMN_LIST,
    );
    let mut statement = conn.prepare(&insert_sql).map_err(|e| e.to_string())?;
    let mut files = FileIds::default();
    for (index, (event, text_ref)) in events.iter().zip(text_refs).enumerate() {
        let identity = format_identity_hash(event_identity_hash(event));
        let occurrence = next_occurrences.entry(identity.clone()).or_default();
        let attachments = serde_json::to_string(&event.attachments).map_err(|e| e.to_string())?;
        statement
            .execute(params![
                source.as_str(),
                session_id,
                stored_event_id(event),
                first_sequence.map(|sequence| sequence + index as u32),
                files.resolve(conn, &event.source_file)?,
                event.source_sequence,
                enum_token(event.kind)?,
                event.actor.map(enum_token).transpose()?,
                event.name,
                event.occurred_at,
                occurred_at_sort_key(&event.occurred_at),
                event.text.as_deref().filter(|_| text_ref.is_none()),
                attachments,
                enum_token(event.capability_status)?,
                enum_token(event.content_status)?,
                identity,
                *occurrence,
                generation,
                text_ref.map(|text_ref| text_ref.line_offset),
                text_ref.map(|text_ref| text_ref.text_hash),
            ])
            .map_err(|error| error.to_string())?;
        *occurrence += 1;
        if contentless_fts {
            crate::store::insert_conversation_fts(
                conn,
                conn.last_insert_rowid(),
                event.text.as_deref().unwrap_or(""),
                event.name.as_deref().unwrap_or(""),
            )?;
        }
    }
    Ok(())
}

fn write_file_events(
    conn: &Connection,
    source: Source,
    parsed: &ParsedConversation,
    generations: &mut BTreeMap<String, i64>,
) -> Result<(), String> {
    let session_id = parsed.session.session_id.as_str();
    let generation = match generations.get(session_id) {
        Some(generation) => *generation,
        None => {
            let generation = next_generation(conn, source, session_id)?;
            generations.insert(session_id.to_string(), generation);
            generation
        }
    };
    insert_events(
        conn,
        EventBatch {
            source,
            session_id,
            generation,
            events: &parsed.events,
            origin: FileIndexCursor {
                byte_offset: 0,
                line: 0,
            },
            first_sequence: None,
        },
        &mut BTreeMap::new(),
    )
}

/// 发布一代整份写完的事件。新一代覆盖了源文件全部内容，会话的待补建标记随之清掉，
/// 并同步写入精确的 `event_count`。
fn finalize_session_events(
    conn: &Connection,
    source: Source,
    session_id: &str,
    generation: i64,
) -> Result<(), String> {
    conn.execute(
        r#"
        DELETE FROM conversation_events
        WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
          AND rowid NOT IN (
            SELECT rowid FROM (
              SELECT e.rowid AS rowid,
                ROW_NUMBER() OVER (
                  PARTITION BY e.identity_hash, e.identity_occurrence
                  ORDER BY f.path
                ) AS file_rank
              FROM conversation_events e
              JOIN conversation_files f ON f.file_id = e.file_id
              WHERE e.source = ?1 AND e.session_id = ?2 AND e.index_generation = ?3
            )
            WHERE file_rank = 1
          )
        "#,
        params![source.as_str(), session_id, generation],
    )
    .map_err(|error| error.to_string())?;
    let mut order_statement = conn
        .prepare(
            r#"
            SELECT e.rowid, e.occurred_at, f.path, e.source_sequence, e.event_id
            FROM conversation_events e
            JOIN conversation_files f ON f.file_id = e.file_id
            WHERE e.source = ?1 AND e.session_id = ?2 AND e.index_generation = ?3
            "#,
        )
        .map_err(|error| error.to_string())?;
    let mut ordered = order_statement
        .query_map(params![source.as_str(), session_id, generation], |row| {
            let path = row.get::<_, String>(2)?;
            let source_sequence = row.get::<_, u32>(3)?;
            let event_id = restore_event_id(row.get(4)?, &path, source_sequence);
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                path,
                source_sequence,
                event_id,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    ordered.sort_by(|left, right| {
        compare_optional_timestamps(&left.1, &right.1)
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.cmp(&right.3))
            .then_with(|| left.4.cmp(&right.4))
    });
    let event_count = ordered.len() as i64;
    let mut update_sequence = conn
        .prepare("UPDATE conversation_events SET sequence = ?1 WHERE rowid = ?2")
        .map_err(|error| error.to_string())?;
    for (sequence, (rowid, _, _, _, _)) in ordered.into_iter().enumerate() {
        update_sequence
            .execute(params![sequence as u32, rowid])
            .map_err(|error| error.to_string())?;
    }
    conn.execute(
        r#"
        UPDATE conversation_sessions
        SET event_index_generation = ?3, event_index_dirty = 0, event_count = ?4
        WHERE source = ?1 AND session_id = ?2
        "#,
        params![source.as_str(), session_id, generation, event_count],
    )
    .map_err(|error| error.to_string())?;
    conn.execute(
        r#"
        DELETE FROM conversation_events
        WHERE source = ?1 AND session_id = ?2 AND index_generation != ?3
        "#,
        params![source.as_str(), session_id, generation],
    )
    .map_err(|error| error.to_string())?;
    clear_session_tools(conn, source.as_str(), session_id, Some(generation))?;
    refresh_session_tools(conn, source.as_str(), session_id, generation)?;
    Ok(())
}

fn next_generation(conn: &Connection, source: Source, session_id: &str) -> Result<i64, String> {
    let live = session_generation(conn, source.as_str(), session_id)?;
    if let Some(live) = live {
        conn.execute(
            r#"
            DELETE FROM conversation_events
            WHERE source = ?1 AND session_id = ?2 AND index_generation != ?3
            "#,
            params![source.as_str(), session_id, live],
        )
        .map_err(|error| error.to_string())?;
        clear_session_tools(conn, source.as_str(), session_id, Some(live))?;
        Ok(live + 1)
    } else {
        conn.execute(
            "DELETE FROM conversation_events WHERE source = ?1 AND session_id = ?2",
            params![source.as_str(), session_id],
        )
        .map_err(|error| error.to_string())?;
        clear_session_tools(conn, source.as_str(), session_id, None)?;
        Ok(1)
    }
}

fn occurred_at_sort_key(occurred_at: &Option<String>) -> Option<String> {
    let raw = occurred_at.as_ref()?;
    match chrono::DateTime::parse_from_rfc3339(raw) {
        Ok(parsed) => Some(
            parsed
                .with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
        ),
        Err(_) => Some(raw.clone()),
    }
}

fn enum_token<T: Serialize>(value: T) -> Result<String, String> {
    match serde_json::to_value(value).map_err(|error| error.to_string())? {
        Value::String(token) => Ok(token),
        _ => Err("枚举序列化失败".to_string()),
    }
}
