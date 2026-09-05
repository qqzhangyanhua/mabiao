//! Cursor hash 模型与 usage-only 会话桥接。
//!
//! Cursor 会话（ADR 0007 行为 KPI）与对话记录（ADR 0011 事件正文）是两个分区。
//! 这一簇坐在接缝上；聚在这里便于核对它没有渗进消耗记录。按会话 id 反查
//! 会话行与用量条目走兄弟模块 `session_store`，不经模块根。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::{params, params_from_iter, Connection};

use crate::domain::{ConversationSessionRow, Source};

use super::catalog::sql_placeholders;
use super::cursor;
use super::merge::IndexedAgentMetadata;
use super::session_store::{load_session, load_usage_records, upsert_session};
use super::toolbox::{CAPABILITY_EVENTS, CAPABILITY_USAGE, EXPERIMENTAL};

pub(crate) fn sync_cursor_usage_only_sessions(conn: &Connection) -> Result<(), String> {
    let mut statement = conn
        .prepare(
            "SELECT DISTINCT session_id FROM usage_records WHERE source = ?1 AND session_id != ''",
        )
        .map_err(|error| error.to_string())?;
    let session_ids = statement
        .query_map(params![Source::CursorAgent.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let current_session_ids = session_ids.iter().cloned().collect::<BTreeSet<_>>();
    for session_id in session_ids {
        let existing = load_session(conn, Source::CursorAgent.as_str(), &session_id)?;
        let records = load_usage_records(conn, Source::CursorAgent, &session_id)?;
        let Some(first) = records.first() else {
            continue;
        };
        let last = records.last().unwrap_or(first);
        let model = records
            .iter()
            .rev()
            .find_map(|record| (!record.model.is_empty()).then(|| record.model.clone()))
            .unwrap_or_default();
        let project = records
            .iter()
            .rev()
            .find_map(|record| (!record.project.is_empty()).then(|| record.project.clone()))
            .unwrap_or_default();
        if existing
            .as_ref()
            .is_some_and(|existing| cursor::is_native_transcript(Path::new(&existing.source_file)))
        {
            conn.execute(
                r#"
                UPDATE conversation_sessions SET
                    model = CASE WHEN model = '' THEN ?3 ELSE model END,
                    project = CASE WHEN project = '' THEN ?4 ELSE project END,
                    started_at = CASE WHEN started_at = '' THEN ?5 ELSE started_at END,
                    ended_at = CASE WHEN ended_at = '' THEN ?6 ELSE ended_at END
                WHERE source = ?1 AND session_id = ?2
                "#,
                params![
                    Source::CursorAgent.as_str(),
                    session_id,
                    model,
                    project,
                    first.occurred_at,
                    last.occurred_at,
                ],
            )
            .map_err(|error| error.to_string())?;
            continue;
        }
        let session = ConversationSessionRow {
            source: Source::CursorAgent.as_str().to_string(),
            session_id: session_id.clone(),
            title: session_id,
            project,
            model,
            started_at: first.occurred_at.clone(),
            ended_at: last.occurred_at.clone(),
            source_file: first.source_file.clone(),
            source_files: vec![first.source_file.clone()],
            capabilities: vec![CAPABILITY_EVENTS.to_string(), CAPABILITY_USAGE.to_string()],
            support_status: EXPERIMENTAL.to_string(),
            file_available: false,
            ..Default::default()
        };
        upsert_session(
            conn,
            &session,
            true,
            &IndexedAgentMetadata::default(),
            0,
            0,
            "usage-only",
        )?;
    }
    let mut synthetic = conn
        .prepare(
            "SELECT session_id FROM conversation_sessions WHERE source = ?1 AND source_revision = 'usage-only'",
        )
        .map_err(|error| error.to_string())?;
    let stale_session_ids = synthetic
        .query_map(params![Source::CursorAgent.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for session_id in stale_session_ids {
        if !current_session_ids.contains(&session_id) {
            conn.execute(
                "DELETE FROM conversation_sessions WHERE source = ?1 AND session_id = ?2",
                params![Source::CursorAgent.as_str(), session_id],
            )
            .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// transcript 不含模型名。有过代码编辑时，`ai_code_hashes` enrich 写在 `cursor_sessions.models_json`。
pub(crate) fn model_label_from_models_json(raw: &str) -> String {
    serde_json::from_str::<Vec<String>>(raw)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn cursor_hash_models_by_session(
    conn: &Connection,
    session_ids: &[String],
) -> Result<BTreeMap<String, String>, String> {
    if session_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let sql = format!(
        "SELECT session_id, models_json FROM cursor_sessions WHERE session_id IN ({})",
        sql_placeholders(session_ids.len())
    );
    let mut statement = conn.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params_from_iter(session_ids.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|(session_id, models_json)| {
            let model = model_label_from_models_json(&models_json);
            (!model.is_empty()).then_some((session_id, model))
        })
        .collect())
}

pub(crate) fn apply_cursor_hash_model(session: &mut ConversationSessionRow, model: Option<&str>) {
    if session.model.is_empty() {
        if let Some(model) = model.filter(|value| !value.is_empty()) {
            session.model = model.to_string();
        }
    }
}

pub(crate) fn sync_cursor_hash_models(conn: &Connection) -> Result<(), String> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT session_id FROM conversation_sessions
            WHERE source = ?1 AND model = ''
            "#,
        )
        .map_err(|error| error.to_string())?;
    let session_ids = statement
        .query_map(params![Source::CursorAgent.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let models = cursor_hash_models_by_session(conn, &session_ids)?;
    for (session_id, model) in models {
        conn.execute(
            r#"
            UPDATE conversation_sessions
            SET model = ?3
            WHERE source = ?1 AND session_id = ?2 AND model = ''
            "#,
            params![Source::CursorAgent.as_str(), session_id, model],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(crate) fn hydrate_cursor_hash_models(
    conn: &Connection,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    let session_ids = rows
        .iter()
        .filter(|row| row.source == Source::CursorAgent.as_str() && row.model.is_empty())
        .map(|row| row.session_id.clone())
        .collect::<Vec<_>>();
    if session_ids.is_empty() {
        return Ok(());
    }
    let models = cursor_hash_models_by_session(conn, &session_ids)?;
    for row in rows {
        apply_cursor_hash_model(row, models.get(&row.session_id).map(String::as_str));
    }
    Ok(())
}

pub(crate) fn fill_empty_cursor_hash_model(
    conn: &Connection,
    session: &mut ConversationSessionRow,
) -> Result<(), String> {
    if session.source != Source::CursorAgent.as_str() || !session.model.is_empty() {
        return Ok(());
    }
    let models = cursor_hash_models_by_session(conn, std::slice::from_ref(&session.session_id))?;
    apply_cursor_hash_model(session, models.get(&session.session_id).map(String::as_str));
    Ok(())
}
