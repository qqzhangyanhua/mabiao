//! `conversation_events` SQL 查询与 ADR-0025 外置正文回水辅助。

use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::domain::{ConversationEvent, ConversationEventAnchor, ConversationEventPage, Source};

use super::super::MAX_PAGE_SIZE;
use super::readiness::session_generation;
use super::storage::{hydrate_texts, restore_event_id, split_derived_event_id, TextRef};

const EVENT_SELECT: &str = r#"
    SELECT e.event_id, f.path, e.source_sequence, e.kind, e.actor, e.name, e.occurred_at, e.text,
           e.attachments_json, e.capability_status, e.content_status, e.sequence,
           e.line_offset, e.text_hash
    FROM conversation_events e
    JOIN conversation_files f ON f.file_id = e.file_id
    WHERE e.source = ?1 AND e.session_id = ?2 AND e.index_generation = ?3
"#;

pub(super) enum IndexedEventLookup {
    Found(ConversationEvent),
    NotFound,
    ReferenceMismatch,
}

pub(super) fn lookup_indexed_event(
    conn: &Connection,
    source: &str,
    session_id: &str,
    event_id: &str,
) -> Result<IndexedEventLookup, String> {
    let Some(generation) = session_generation(conn, source, session_id)? else {
        return Ok(IndexedEventLookup::NotFound);
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
        return Ok(IndexedEventLookup::NotFound);
    };
    match hydrated(source, session_id, vec![row.into_event()?]) {
        Some(events) => match events.into_iter().next() {
            Some(event) => Ok(IndexedEventLookup::Found(event)),
            None => Ok(IndexedEventLookup::NotFound),
        },
        None => Ok(IndexedEventLookup::ReferenceMismatch),
    }
}

/// 返回 `Ok(None)` 表示存在已发布代次但 ADR-0025 外置正文回不了源文件或指纹失配。
pub(super) fn query_live_indexed_events(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Option<Vec<ConversationEvent>>, String> {
    let Some(generation) = session_generation(conn, source, session_id)? else {
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

/// 外置正文回不了源文件时返回 `None`，调用方整份解析源文件再分页。
pub(super) fn query_indexed_events_page(
    conn: &Connection,
    source: &str,
    session_id: &str,
    anchor: &ConversationEventAnchor,
    limit: u32,
) -> Result<Option<ConversationEventPage>, String> {
    let limit = limit.clamp(1, MAX_PAGE_SIZE);
    let Some(generation) = session_generation(conn, source, session_id)? else {
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

pub(super) fn empty_event_page() -> ConversationEventPage {
    ConversationEventPage {
        events: Vec::new(),
        has_more_before: false,
        has_more_after: false,
    }
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

fn parse_token<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    serde_json::from_value(Value::String(raw.to_string()))
        .map_err(|error| format!("事件索引字段无法解析：{error}"))
}
