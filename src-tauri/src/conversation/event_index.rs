use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::domain::{ConversationEvent, ConversationEventAnchor, ConversationEventPage, Source};

use super::event_storage::{
    hydrate_texts, plan_text_refs, restore_event_id, split_derived_event_id, stored_event_id,
    TextRef,
};
use super::event_tables::{clear_session_tools, refresh_session_tools, FileIds};
use super::merge::event_identity;
use super::toolbox::{FileIndexCursor, ParsedConversation};

pub fn write_file_events(
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

/// `origin` 是这批后缀事件在源文件里被解析的起点，外置正文要据此算行偏移。
pub fn append_live_events(
    conn: &Connection,
    source: Source,
    session_id: &str,
    events: &[ConversationEvent],
    origin: FileIndexCursor,
) -> Result<u32, String> {
    if live_index_would_rewind(conn, source, session_id, events)? {
        return Err("新事件时间早于已有索引，需要整份重索引".to_string());
    }
    let Some(generation) = live_generation(conn, source.as_str(), session_id)? else {
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
    Ok((first_sequence + events.len() as u32).saturating_sub(1))
}

struct EventBatch<'a> {
    source: Source,
    session_id: &'a str,
    generation: i64,
    events: &'a [ConversationEvent],
    origin: FileIndexCursor,
    /// 整份写入时为空，由 `finalize_session_events` 统一排序后再编号。
    first_sequence: Option<u32>,
}

fn insert_events(
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
        let identity = identity_hash(event);
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
    .map(|value| value.flatten())
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

pub fn finalize_session_events(
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
        super::toolbox::compare_optional_timestamps(&left.1, &right.1)
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.cmp(&right.3))
            .then_with(|| left.4.cmp(&right.4))
    });
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
        SET event_index_generation = ?3
        WHERE source = ?1 AND session_id = ?2
        "#,
        params![source.as_str(), session_id, generation],
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

pub fn has_live_generation(
    conn: &Connection,
    source: Source,
    session_id: &str,
) -> Result<bool, String> {
    let generation = conn
        .query_row(
            r#"
            SELECT event_index_generation
            FROM conversation_sessions
            WHERE source = ?1 AND session_id = ?2
            "#,
            params![source.as_str(), session_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    Ok(generation.is_some())
}

pub fn clear_session_events(
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
        SET event_index_generation = NULL
        WHERE source = ?1 AND session_id = ?2
        "#,
        params![source.as_str(), session_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

const EVENT_SELECT: &str = r#"
    SELECT e.event_id, f.path, e.source_sequence, e.kind, e.actor, e.name, e.occurred_at, e.text,
           e.attachments_json, e.capability_status, e.content_status, e.sequence,
           e.line_offset, e.text_hash
    FROM conversation_events e
    JOIN conversation_files f ON f.file_id = e.file_id
    WHERE e.source = ?1 AND e.session_id = ?2 AND e.index_generation = ?3
"#;

/// 外置正文回不了源文件时返回 `None`，调用方整份解析源文件。
pub fn indexed_events(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Option<Vec<ConversationEvent>>, String> {
    let Some(generation) = live_generation(conn, source, session_id)? else {
        return Ok(Some(Vec::new()));
    };
    let rows = query_events(
        conn,
        EventQuery {
            source,
            session_id,
            generation,
            extra_predicate: "1 = 1",
            bound: None,
            order_by: "sequence ASC",
            limit: None,
        },
    )?;
    Ok(hydrated(source, session_id, rows))
}

/// 外置正文回不了源文件时与「索引里没有」一样返回 `None`，调用方走整份解析。
pub fn indexed_event(
    conn: &Connection,
    source: &str,
    session_id: &str,
    event_id: &str,
) -> Result<Option<ConversationEvent>, String> {
    let Some(generation) = live_generation(conn, source, session_id)? else {
        return Ok(None);
    };
    let (file_id, source_sequence, stored_suffix) = match split_derived_event_id(event_id) {
        Some((path, source_sequence, suffix)) => (
            file_id_for_path(conn, &path)?.unwrap_or(-1),
            i64::from(source_sequence),
            suffix,
        ),
        None => (-1, -1, String::new()),
    };
    let row = conn
        .query_row(
            &format!(
                "{EVENT_SELECT} AND (e.event_id = ?4 \
                 OR (e.file_id = ?5 AND e.source_sequence = ?6 AND e.event_id = ?7))"
            ),
            params![
                source,
                session_id,
                generation,
                event_id,
                file_id,
                source_sequence,
                stored_suffix
            ],
            IndexedRow::from_sql,
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(hydrated(source, session_id, vec![row.into_event()?])
        .and_then(|events| events.into_iter().next()))
}

fn file_id_for_path(conn: &Connection, path: &str) -> Result<Option<i64>, String> {
    conn.query_row(
        "SELECT file_id FROM conversation_files WHERE path = ?1",
        params![path],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn hydrated(
    source: &str,
    session_id: &str,
    rows: Vec<(ConversationEvent, Option<TextRef>)>,
) -> Option<Vec<ConversationEvent>> {
    let (mut events, refs): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
    if refs.iter().all(Option::is_none) {
        return Some(events);
    }
    let source = Source::parse(source)?;
    hydrate_texts(source, session_id, &mut events, &refs).then_some(events)
}

pub fn indexed_event_count(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<u32, String> {
    let Some(generation) = live_generation(conn, source, session_id)? else {
        return Ok(0);
    };
    conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM conversation_events
        WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
        "#,
        params![source, session_id, generation],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count as u32)
    .map_err(|error| error.to_string())
}

/// 外置正文回不了源文件时返回 `None`，调用方整份解析源文件再分页。
pub fn indexed_events_page(
    conn: &Connection,
    source: &str,
    session_id: &str,
    anchor: &ConversationEventAnchor,
    limit: u32,
) -> Result<Option<ConversationEventPage>, String> {
    let limit = limit.clamp(1, 200);
    let Some(generation) = live_generation(conn, source, session_id)? else {
        return Ok(Some(empty_event_page()));
    };
    let rows = match anchor {
        ConversationEventAnchor::First => query_events(
            conn,
            EventQuery {
                source,
                session_id,
                generation,
                extra_predicate: "1 = 1",
                bound: None,
                order_by: "sequence ASC",
                limit: Some(limit),
            },
        )?,
        ConversationEventAnchor::Last => {
            let mut page = query_events(
                conn,
                EventQuery {
                    source,
                    session_id,
                    generation,
                    extra_predicate: "1 = 1",
                    bound: None,
                    order_by: "sequence DESC",
                    limit: Some(limit),
                },
            )?;
            page.reverse();
            page
        }
        ConversationEventAnchor::Before { sequence } => {
            let mut page = query_events(
                conn,
                EventQuery {
                    source,
                    session_id,
                    generation,
                    extra_predicate: "sequence < ?4",
                    bound: Some(*sequence),
                    order_by: "sequence DESC",
                    limit: Some(limit),
                },
            )?;
            page.reverse();
            page
        }
        ConversationEventAnchor::After { sequence } => query_events(
            conn,
            EventQuery {
                source,
                session_id,
                generation,
                extra_predicate: "sequence > ?4",
                bound: Some(*sequence),
                order_by: "sequence ASC",
                limit: Some(limit),
            },
        )?,
        ConversationEventAnchor::Around { sequence } => query_events(
            conn,
            EventQuery {
                source,
                session_id,
                generation,
                extra_predicate: "sequence >= ?4",
                bound: Some(*sequence),
                order_by: "sequence ASC",
                limit: Some(limit),
            },
        )?,
    };
    if rows.is_empty() {
        return empty_page_flags(conn, source, session_id, generation, anchor).map(Some);
    }
    let Some(events) = hydrated(source, session_id, rows) else {
        return Ok(None);
    };
    let min_sequence = events[0].sequence;
    let max_sequence = events.last().expect("page is not empty").sequence;
    Ok(Some(ConversationEventPage {
        events,
        has_more_before: sequence_exists(
            conn,
            source,
            session_id,
            generation,
            "sequence < ?4",
            min_sequence,
        )?,
        has_more_after: sequence_exists(
            conn,
            source,
            session_id,
            generation,
            "sequence > ?4",
            max_sequence,
        )?,
    }))
}

pub fn live_index_would_rewind(
    conn: &Connection,
    source: Source,
    session_id: &str,
    events: &[ConversationEvent],
) -> Result<bool, String> {
    let Some(generation) = live_generation(conn, source.as_str(), session_id)? else {
        return Ok(false);
    };
    let max_occurred_at = max_occurred_at(conn, source.as_str(), session_id, generation)?;
    Ok(super::incremental::new_events_precede_existing(
        max_occurred_at.as_deref(),
        events.iter().map(|event| event.occurred_at.clone()),
    ))
}

pub(crate) fn session_generation(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Option<i64>, String> {
    live_generation(conn, source, session_id)
}

fn live_generation(
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
    .map(|generation| generation.flatten())
    .map_err(|error| error.to_string())
}

struct EventQuery<'a> {
    source: &'a str,
    session_id: &'a str,
    generation: i64,
    extra_predicate: &'a str,
    bound: Option<u32>,
    order_by: &'a str,
    limit: Option<u32>,
}

fn query_events(
    conn: &Connection,
    query: EventQuery<'_>,
) -> Result<Vec<(ConversationEvent, Option<TextRef>)>, String> {
    let EventQuery {
        source,
        session_id,
        generation,
        extra_predicate,
        bound,
        order_by,
        limit,
    } = query;
    let mut sql = format!("{EVENT_SELECT} AND ({extra_predicate}) ORDER BY {order_by}");
    if limit.is_some() {
        sql.push_str(if bound.is_some() {
            " LIMIT ?5"
        } else {
            " LIMIT ?4"
        });
    }
    let mut statement = conn.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = match (bound, limit) {
        (Some(bound), Some(limit)) => statement
            .query_map(
                params![source, session_id, generation, bound, i64::from(limit)],
                IndexedRow::from_sql,
            )
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>(),
        (None, Some(limit)) => statement
            .query_map(
                params![source, session_id, generation, i64::from(limit)],
                IndexedRow::from_sql,
            )
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>(),
        (Some(bound), None) => statement
            .query_map(
                params![source, session_id, generation, bound],
                IndexedRow::from_sql,
            )
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>(),
        (None, None) => statement
            .query_map(
                params![source, session_id, generation],
                IndexedRow::from_sql,
            )
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>(),
    }
    .map_err(|error| error.to_string())?;
    rows.into_iter().map(IndexedRow::into_event).collect()
}

/// `EVENT_SELECT` 的一行，列顺序与它一一对应。
struct IndexedRow {
    stored_event_id: String,
    source_file: String,
    source_sequence: u32,
    kind: String,
    actor: Option<String>,
    name: Option<String>,
    occurred_at: Option<String>,
    text: Option<String>,
    attachments_json: String,
    capability_status: String,
    content_status: String,
    sequence: u32,
    line_offset: Option<i64>,
    text_hash: Option<i64>,
}

impl IndexedRow {
    fn from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            stored_event_id: row.get(0)?,
            source_file: row.get(1)?,
            source_sequence: row.get(2)?,
            kind: row.get(3)?,
            actor: row.get(4)?,
            name: row.get(5)?,
            occurred_at: row.get(6)?,
            text: row.get(7)?,
            attachments_json: row.get(8)?,
            capability_status: row.get(9)?,
            content_status: row.get(10)?,
            sequence: row.get(11)?,
            line_offset: row.get(12)?,
            text_hash: row.get(13)?,
        })
    }

    fn into_event(self) -> Result<(ConversationEvent, Option<TextRef>), String> {
        let attachments =
            serde_json::from_str(&self.attachments_json).map_err(|e| e.to_string())?;
        let text_ref = match (self.line_offset, self.text_hash) {
            (Some(line_offset), Some(text_hash)) => Some(TextRef {
                line_offset,
                text_hash,
            }),
            _ => None,
        };
        let event = ConversationEvent {
            event_id: restore_event_id(
                self.stored_event_id,
                &self.source_file,
                self.source_sequence,
            ),
            sequence: self.sequence,
            source_file: self.source_file,
            source_sequence: self.source_sequence,
            kind: parse_token(&self.kind)?,
            occurred_at: self.occurred_at,
            actor: self.actor.map(|value| parse_token(&value)).transpose()?,
            name: self.name,
            text: self.text,
            details: Value::Null,
            attachments,
            capability_status: parse_token(&self.capability_status)?,
            content_status: parse_token(&self.content_status)?,
        };
        Ok((event, text_ref))
    }
}

fn sequence_exists(
    conn: &Connection,
    source: &str,
    session_id: &str,
    generation: i64,
    predicate: &str,
    sequence: u32,
) -> Result<bool, String> {
    conn.query_row(
        &format!(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM conversation_events
                WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3 AND {predicate}
            )
            "#
        ),
        params![source, session_id, generation, sequence],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|error| error.to_string())
}

fn empty_page_flags(
    conn: &Connection,
    source: &str,
    session_id: &str,
    generation: i64,
    anchor: &ConversationEventAnchor,
) -> Result<ConversationEventPage, String> {
    let (has_more_before, has_more_after) = match anchor {
        ConversationEventAnchor::First | ConversationEventAnchor::Last => (false, false),
        ConversationEventAnchor::Before { sequence } => (
            false,
            sequence_exists(
                conn,
                source,
                session_id,
                generation,
                "sequence >= ?4",
                *sequence,
            )?,
        ),
        ConversationEventAnchor::After { sequence } => (
            sequence_exists(
                conn,
                source,
                session_id,
                generation,
                "sequence <= ?4",
                *sequence,
            )?,
            false,
        ),
        ConversationEventAnchor::Around { sequence } => (
            sequence_exists(
                conn,
                source,
                session_id,
                generation,
                "sequence < ?4",
                *sequence,
            )?,
            sequence_exists(
                conn,
                source,
                session_id,
                generation,
                "sequence >= ?4",
                *sequence,
            )?,
        ),
    };
    Ok(ConversationEventPage {
        events: Vec::new(),
        has_more_before,
        has_more_after,
    })
}

fn empty_event_page() -> ConversationEventPage {
    ConversationEventPage {
        events: Vec::new(),
        has_more_before: false,
        has_more_after: false,
    }
}

fn next_generation(conn: &Connection, source: Source, session_id: &str) -> Result<i64, String> {
    let live = conn
        .query_row(
            r#"
            SELECT event_index_generation
            FROM conversation_sessions
            WHERE source = ?1 AND session_id = ?2
            "#,
            params![source.as_str(), session_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
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

fn identity_hash(event: &ConversationEvent) -> String {
    let identity = event_identity(event);
    format!(
        "{:016x}{:016x}",
        fnv1a64(identity.as_bytes()),
        fnv1a64_alt(identity.as_bytes())
    )
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn fnv1a64_alt(bytes: &[u8]) -> u64 {
    let mut hash = 0x84222325cbf29ce4;
    for byte in bytes.iter().rev() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn enum_token<T: Serialize>(value: T) -> Result<String, String> {
    match serde_json::to_value(value).map_err(|error| error.to_string())? {
        Value::String(token) => Ok(token),
        _ => Err("枚举序列化失败".to_string()),
    }
}

fn parse_token<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    serde_json::from_value(Value::String(raw.to_string()))
        .map_err(|error| format!("事件索引字段无法解析：{error}"))
}
