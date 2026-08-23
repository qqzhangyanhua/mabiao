use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::conversation::event_identity;
use crate::domain::{ConversationEvent, Source};

pub fn write_file_events(
    conn: &Connection,
    source: Source,
    parsed: &crate::conversation::ParsedConversation,
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
    let mut statement = conn
        .prepare(
            r#"
            INSERT INTO conversation_events(
                source, session_id, event_id, sequence, source_file, source_sequence, kind, actor,
                name, occurred_at, occurred_at_sort, text, attachments_json, capability_status,
                content_status, identity_hash, identity_occurrence, index_generation
            ) VALUES(?1,?2,?3,NULL,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
            "#,
        )
        .map_err(|error| error.to_string())?;
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
                event.source_file,
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
              SELECT rowid,
                ROW_NUMBER() OVER (
                  PARTITION BY identity_hash, identity_occurrence
                  ORDER BY source_file
                ) AS file_rank
              FROM conversation_events
              WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
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
            SELECT rowid, occurred_at, source_file, source_sequence, event_id
            FROM conversation_events
            WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
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
        super::compare_optional_timestamps(&left.1, &right.1)
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

pub fn indexed_events(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Vec<ConversationEvent>, String> {
    let generation = conn
        .query_row(
            r#"
            SELECT event_index_generation
            FROM conversation_sessions
            WHERE source = ?1 AND session_id = ?2
            "#,
            params![source, session_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(Some(generation)) = generation else {
        return Ok(Vec::new());
    };
    let mut statement = conn
        .prepare(
            r#"
            SELECT event_id, source_file, source_sequence, kind, actor, name, occurred_at, text,
                   attachments_json, capability_status, content_status, sequence
            FROM conversation_events
            WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
            ORDER BY sequence
            "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![source, session_id, generation], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, u32>(11)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    rows.into_iter()
        .map(
            |(
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
            )| {
                let attachments =
                    serde_json::from_str(&attachments_json).map_err(|e| e.to_string())?;
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
            },
        )
        .collect()
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
        Ok(live + 1)
    } else {
        conn.execute(
            "DELETE FROM conversation_events WHERE source = ?1 AND session_id = ?2",
            params![source.as_str(), session_id],
        )
        .map_err(|error| error.to_string())?;
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
