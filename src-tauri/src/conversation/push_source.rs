//! 推送要读的对话记录：选会话、读回完整事件、取上下文清单与注入原文（ADR 0026「推什么」）。
//!
//! 只负责「读」。打码、转协议类型、联网都在 `crate::push`。读不全的会话返回 `Err(原因)`，
//! 调用方整场跳过并把原因写给用户，不推半截正文。
//!
//! 事件 `details` 不在这里取：推送只带语义事件的 `text`，原始载荷不出本机。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{params_from_iter, Connection};

use crate::domain::{
    ConversationContextManifest, ConversationEvent, ConversationQuery, ConversationSessionRow,
    Source,
};

use super::catalog::catalog_filter_sql;
use super::event_store::{event_index_ready_sql, read_session_events, ReadOptions};
use super::session_store::row_from_sql;
use super::{
    context_content, context_manifest, cursor, hydrate, load_detail, prepare_detail,
    CONVERSATION_SOURCES,
};

/// 一场会话推送时现场读到的全部内容。注入原文只活在这个值里，不落库。
pub struct PushSessionSource {
    pub session: ConversationSessionRow,
    /// 按显示顺序，`sequence` 连续；`text` 是完整正文，`details` 为空。
    pub events: Vec<ConversationEvent>,
    pub context: Option<PushContext>,
}

pub struct PushContext {
    pub manifest: ConversationContextManifest,
    /// `injected` 层条目 id → 注入原文。快照已清理、来自缓存度量时为空。
    pub injected_contents: BTreeMap<String, String>,
}

/// 与区间**有重叠**的顶层会话：结束时间 ≥ from 且开始时间 ≤ to，两端都含。
/// 整场推送不切开；源文件已不在的（已归档）会话也列出来，读不回时由 [`read_session`] 说明原因。
pub fn list_sessions(
    conn: &Connection,
    query: &ConversationQuery,
) -> Result<Vec<ConversationSessionRow>, String> {
    let (predicate, params) = catalog_filter_sql(query);
    let ready_sql = event_index_ready_sql("sessions");
    let sql = format!(
        r#"
        SELECT sessions.source, sessions.session_id, sessions.title, sessions.project, sessions.model,
               COALESCE(NULLIF(sessions.started_at, ''), cursor_times.first_seen_at, '') AS started_at,
               COALESCE(NULLIF(sessions.ended_at, ''), cursor_times.last_seen_at, cursor_times.first_seen_at, '') AS ended_at,
               sessions.source_file, sessions.capabilities_json, sessions.support_status, sessions.file_available,
               {ready_sql},
               -1
        FROM conversation_sessions AS sessions
        LEFT JOIN cursor_sessions AS cursor_times
          ON sessions.source = 'cursor_agent' AND sessions.session_id = cursor_times.session_id
        WHERE {predicate}
        ORDER BY sessions.source ASC, sessions.session_id ASC
        "#
    );
    let mut statement = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = statement
        .query_map(params_from_iter(params.iter()), row_from_sql)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    hydrate::sessions(conn, None, &mut rows)?;
    Ok(rows)
}

/// 读回一场会话的完整事件与上下文清单。`Err` 的文字就是给用户看的跳过原因。
pub fn read_session(
    conn: &Connection,
    home: &Path,
    row: &ConversationSessionRow,
) -> Result<PushSessionSource, String> {
    let source = Source::parse(&row.source)
        .filter(|source| CONVERSATION_SOURCES.contains(source))
        .ok_or_else(|| "该来源尚未支持对话详情".to_string())?;
    let prepared = prepare_detail(conn, source.as_str(), &row.session_id)?;
    if !prepared.session.file_available {
        return Err("原始文件已不存在，正文读不回".to_string());
    }
    let representative = PathBuf::from(&prepared.session.source_file);
    if source == Source::CursorAgent
        && (!cursor::is_native_transcript(&representative) || !representative.is_file())
    {
        return Err("Cursor transcript 不可读取，只剩用量元数据".to_string());
    }

    let events = strip_details(read_session_events(
        conn,
        home,
        source.as_str(),
        &row.session_id,
        ReadOptions::full_strict_references(),
    )?);
    let context = read_context(conn, home, source, row)?;
    Ok(PushSessionSource {
        session: row.clone(),
        events,
        context,
    })
}

fn strip_details(mut events: Vec<ConversationEvent>) -> Vec<ConversationEvent> {
    for event in &mut events {
        event.details = serde_json::Value::Null;
    }
    events
}

fn read_context(
    conn: &Connection,
    home: &Path,
    source: Source,
    row: &ConversationSessionRow,
) -> Result<Option<PushContext>, String> {
    if !context_manifest::supported_source(source) {
        return Ok(None);
    }
    let detail = load_detail(conn, home, source.as_str(), &row.session_id)
        .map_err(|error| format!("上下文清单读取失败：{error}"))?;
    let Some(manifest) = detail.context_manifest else {
        return Ok(None);
    };
    // 只有会话当时真实的注入快照还在才有原文：来自缓存度量、或 Cursor 按当前磁盘重建的，
    // 都不是「会话当时的内容」，不能装成注入原文推出去。
    let injected_contents = if manifest.has_injected_snapshot && !manifest.metrics_from_cache {
        context_content::injected_contents(home, source, &detail.session)
    } else {
        BTreeMap::new()
    };
    Ok(Some(PushContext {
        manifest,
        injected_contents,
    }))
}
