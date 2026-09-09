//! 上下文清单度量缓存：只存条目名与数字，不存注入正文。
//!
//! 摄取时写入；源快照被清理后详情仍能读出当初的度量。可删后从源文件重建，
//! 不装不可再生的用户正文。备份时整表剔除，待遇与 FTS 派生缓存相同。

use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

use crate::domain::{
    ConversationContextItem, ConversationContextLayer, ConversationSessionRow, Source,
};

use super::session_store::load_session;
use super::{cursor_inject, grok_inject};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CachedContextMetrics {
    pub items: Vec<ConversationContextItem>,
    pub mcp_init_summary: Option<String>,
    pub incomplete_keys: Option<Vec<String>>,
}

pub(crate) fn load(
    conn: &Connection,
    source: Source,
    session_id: &str,
) -> Result<Option<CachedContextMetrics>, String> {
    let row = conn
        .query_row(
            r#"
            SELECT items_json, mcp_init_summary, incomplete_keys_json
            FROM conversation_context_metrics
            WHERE source = ?1 AND session_id = ?2
            "#,
            params![source.as_str(), session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((items_json, mcp_init_summary, incomplete_keys_json)) = row else {
        return Ok(None);
    };
    let Ok(items) = serde_json::from_str::<Vec<ConversationContextItem>>(&items_json) else {
        return Ok(None);
    };
    let items: Vec<ConversationContextItem> = items
        .into_iter()
        .filter(|item| item.layer == ConversationContextLayer::Injected)
        .map(sanitize_item)
        .collect();
    let incomplete_keys = incomplete_keys_json
        .as_deref()
        .and_then(|text| serde_json::from_str::<Vec<String>>(text).ok())
        .filter(|keys| !keys.is_empty());
    Ok(Some(CachedContextMetrics {
        items,
        mcp_init_summary: nonempty(mcp_init_summary),
        incomplete_keys,
    }))
}

pub(crate) fn persist_seen_sessions(
    conn: &Connection,
    source: Source,
    session_ids: &BTreeSet<String>,
    skip: &BTreeSet<String>,
) -> Result<(), String> {
    if !matches!(source, Source::CursorAgent | Source::Grok) {
        return Ok(());
    }
    for session_id in session_ids {
        if skip.contains(session_id) {
            continue;
        }
        let Some(session) = load_session(conn, source.as_str(), session_id)? else {
            continue;
        };
        persist_live(conn, source, &session)?;
    }
    Ok(())
}
fn persist_live(
    conn: &Connection,
    source: Source,
    session: &ConversationSessionRow,
) -> Result<(), String> {
    match source {
        Source::CursorAgent => {
            let Some(home) = home_from_cursor_source_file(&session.source_file) else {
                return Ok(());
            };
            let snapshot = cursor_inject::from_session(&home, session);
            if !snapshot.found {
                return Ok(());
            }
            upsert(
                conn,
                source,
                &session.session_id,
                &snapshot.items,
                None,
                snapshot.incomplete_keys.as_deref(),
            )
        }
        Source::Grok => {
            let snapshot = grok_inject::from_session(session);
            if !snapshot.found {
                return Ok(());
            }
            upsert(
                conn,
                source,
                &session.session_id,
                &snapshot.items,
                snapshot.mcp_init_summary.as_deref(),
                None,
            )
        }
        _ => Ok(()),
    }
}

fn upsert(
    conn: &Connection,
    source: Source,
    session_id: &str,
    items: &[ConversationContextItem],
    mcp_init_summary: Option<&str>,
    incomplete_keys: Option<&[String]>,
) -> Result<(), String> {
    let sanitized: Vec<ConversationContextItem> = items
        .iter()
        .filter(|item| item.layer == ConversationContextLayer::Injected)
        .cloned()
        .map(sanitize_item)
        .collect();
    let items_json = serde_json::to_string(&sanitized).map_err(|error| error.to_string())?;
    let incomplete_keys_json = incomplete_keys
        .filter(|keys| !keys.is_empty())
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| error.to_string())?;
    conn.execute(
        r#"
        INSERT INTO conversation_context_metrics(
            source, session_id, items_json, mcp_init_summary, incomplete_keys_json
        ) VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(source, session_id) DO UPDATE SET
            items_json = excluded.items_json,
            mcp_init_summary = excluded.mcp_init_summary,
            incomplete_keys_json = excluded.incomplete_keys_json
        "#,
        params![
            source.as_str(),
            session_id,
            items_json,
            nonempty(mcp_init_summary.map(str::to_string)),
            incomplete_keys_json,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn sanitize_item(mut item: ConversationContextItem) -> ConversationContextItem {
    item.is_noise = false;
    item.is_unused_install = false;
    item.meta = sanitize_meta(item.meta);
    item
}

fn sanitize_meta(meta: Option<Value>) -> Option<Value> {
    let Value::Object(map) = meta? else {
        return None;
    };
    let mut kept = Map::new();
    if let Some(count) = map.get("tool_count").and_then(Value::as_u64) {
        kept.insert("tool_count".into(), json!(count));
    }
    if let Some(error_type) = map.get("error_type").and_then(Value::as_str) {
        if !error_type.is_empty() {
            kept.insert("error_type".into(), json!(error_type));
        }
    }
    if kept.is_empty() {
        None
    } else {
        Some(Value::Object(kept))
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}

fn home_from_cursor_source_file(source_file: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(source_file);
    for ancestor in path.ancestors() {
        if ancestor.file_name().is_some_and(|name| name == ".cursor") {
            return ancestor.parent().map(Path::to_path_buf);
        }
    }
    None
}
