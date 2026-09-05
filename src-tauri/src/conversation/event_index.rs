use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::domain::{ConversationEvent, ConversationEventAnchor, ConversationEventPage, Source};

use super::event_tables::{clear_session_tools, refresh_session_tools, FileIds};
use super::merge::event_identity;
use super::toolbox::ParsedConversation;

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
    let insert_sql = format!(
        "INSERT INTO conversation_events({columns}) \
         VALUES(?1,?2,?3,NULL,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        columns = crate::store::CONVERSATION_EVENT_COLUMN_LIST,
    );
    let mut statement = conn.prepare(&insert_sql).map_err(|e| e.to_string())?;
    let mut files = FileIds::default();
    let mut occurrences = BTreeMap::<String, i64>::new();
    for event in &parsed.events {
        let identity = identity_hash(event);
        let occurrence = occurrences.entry(identity.clone()).or_default();
        let attachments = serde_json::to_string(&event.attachments).map_err(|e| e.to_string())?;
        statement
            .execute(params![
                source.as_str(),
                session_id,
                event.event_id,
                files.resolve(conn, &event.source_file)?,
                event.source_sequence,
                enum_token(event.kind)?,
                event.actor.map(enum_token).transpose()?,
                event.name,
                event.occurred_at,
                occurred_at_sort_key(&event.occurred_at),
                event.text,
                attachments,
                enum_token(event.capability_status)?,
                enum_token(event.content_status)?,
                identity,
                *occurrence,
                generation,
            ])
            .map_err(|error| error.to_string())?;
        *occurrence += 1;
    }
    Ok(())
}

pub fn append_live_events(
    conn: &Connection,
    source: Source,
    session_id: &str,
    events: &[ConversationEvent],
) -> Result<u32, String> {
    if live_index_would_rewind(conn, source, session_id, events)? {
        return Err("新事件时间早于已有索引，需要整份重索引".to_string());
    }
    let Some(generation) = live_generation(conn, source.as_str(), session_id)? else {
        return Err("会话还没有已发布的事件索引".to_string());
    };
    let mut next_sequence = max_sequence(conn, source.as_str(), session_id, generation)?
        .map(|sequence| sequence + 1)
        .unwrap_or(0);
    let mut occurrences = identity_occurrences(conn, source.as_str(), session_id, generation)?;
    let insert_sql = format!(
        "INSERT INTO conversation_events({columns}) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
        columns = crate::store::CONVERSATION_EVENT_COLUMN_LIST,
    );
    let mut statement = conn.prepare(&insert_sql).map_err(|e| e.to_string())?;
    let mut files = FileIds::default();
    for event in events {
        let identity = identity_hash(event);
        let occurrence = occurrences.entry(identity.clone()).or_insert(-1);
        *occurrence += 1;
        let attachments = serde_json::to_string(&event.attachments).map_err(|e| e.to_string())?;
        statement
            .execute(params![
                source.as_str(),
                session_id,
                event.event_id,
                next_sequence,
                files.resolve(conn, &event.source_file)?,
                event.source_sequence,
                enum_token(event.kind)?,
                event.actor.map(enum_token).transpose()?,
                event.name,
                event.occurred_at,
                occurred_at_sort_key(&event.occurred_at),
                event.text,
                attachments,
                enum_token(event.capability_status)?,
                enum_token(event.content_status)?,
                identity,
                *occurrence,
                generation,
            ])
            .map_err(|error| error.to_string())?;
        next_sequence += 1;
    }
    refresh_session_tools(conn, source.as_str(), session_id, generation)?;
    Ok(next_sequence.saturating_sub(1))
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
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, String>(4)?,
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
           e.attachments_json, e.capability_status, e.content_status, e.sequence
    FROM conversation_events e
    JOIN conversation_files f ON f.file_id = e.file_id
    WHERE e.source = ?1 AND e.session_id = ?2 AND e.index_generation = ?3
"#;

pub fn indexed_events(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Vec<ConversationEvent>, String> {
    let Some(generation) = live_generation(conn, source, session_id)? else {
        return Ok(Vec::new());
    };
    query_events(
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
    )
}

pub fn indexed_event(
    conn: &Connection,
    source: &str,
    session_id: &str,
    event_id: &str,
) -> Result<Option<ConversationEvent>, String> {
    let Some(generation) = live_generation(conn, source, session_id)? else {
        return Ok(None);
    };
    conn.query_row(
        &format!("{EVENT_SELECT} AND event_id = ?4"),
        params![source, session_id, generation, event_id],
        map_event_tuple,
    )
    .optional()
    .map_err(|error| error.to_string())?
    .map(event_from_tuple)
    .transpose()
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

pub fn indexed_events_page(
    conn: &Connection,
    source: &str,
    session_id: &str,
    anchor: &ConversationEventAnchor,
    limit: u32,
) -> Result<ConversationEventPage, String> {
    let limit = limit.clamp(1, 200);
    let Some(generation) = live_generation(conn, source, session_id)? else {
        return Ok(empty_event_page());
    };
    let events = match anchor {
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
    if events.is_empty() {
        return empty_page_flags(conn, source, session_id, generation, anchor);
    }
    let min_sequence = events[0].sequence;
    let max_sequence = events.last().expect("page is not empty").sequence;
    Ok(ConversationEventPage {
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
    })
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
) -> Result<Vec<ConversationEvent>, String> {
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
                map_event_tuple,
            )
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>(),
        (None, Some(limit)) => statement
            .query_map(
                params![source, session_id, generation, i64::from(limit)],
                map_event_tuple,
            )
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>(),
        (Some(bound), None) => statement
            .query_map(
                params![source, session_id, generation, bound],
                map_event_tuple,
            )
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>(),
        (None, None) => statement
            .query_map(params![source, session_id, generation], map_event_tuple)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>(),
    }
    .map_err(|error| error.to_string())?;
    rows.into_iter().map(event_from_tuple).collect()
}

type IndexedEventTuple = (
    String,
    String,
    u32,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
    u32,
);

fn map_event_tuple(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedEventTuple> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
    ))
}

fn event_from_tuple(
    (
        event_id,
        source_file,
        source_sequence,
        kind,
        actor,
        name,
        occurred_at,
        text,
        attachments_json,
        capability_status,
        content_status,
        sequence,
    ): IndexedEventTuple,
) -> Result<ConversationEvent, String> {
    let attachments = serde_json::from_str(&attachments_json).map_err(|e| e.to_string())?;
    Ok(ConversationEvent {
        event_id,
        sequence,
        source_file,
        source_sequence,
        kind: parse_token(&kind)?,
        occurred_at,
        actor: actor.map(|value| parse_token(&value)).transpose()?,
        name,
        text,
        details: Value::Null,
        attachments,
        capability_status: parse_token(&capability_status)?,
        content_status: parse_token(&content_status)?,
    })
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
