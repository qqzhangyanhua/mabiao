use std::path::Path;

use rusqlite::Connection;
use serde_json::Value;

use super::attachments::read_source_line;
use super::event_store::{read_event_by_id, ReadOptions};
use super::parse_conversation_files;
use super::session_store::{ensure_matching_session, load_trusted_session_files};
use crate::domain::{
    ConversationEvent, ConversationEventContentDto,
    ConversationEventContentStatus as ContentStatus, Source,
};

pub(super) fn source_maps_line_to_events(source: Source) -> bool {
    matches!(
        source,
        Source::Codex
            | Source::Trae
            | Source::Claude
            | Source::Qoder
            | Source::QoderCn
            | Source::Pi
            | Source::Omp
    )
}

pub fn rebuild_events_from_line(
    source: Source,
    path: &Path,
    session_id: &str,
    source_sequence: u32,
    include_deferred_content: bool,
) -> Result<Vec<ConversationEvent>, String> {
    if !source_maps_line_to_events(source) {
        return Err("该来源的事件映射不是按行无上下文的".to_string());
    }
    let raw = read_jsonl_record(path, source_sequence)?;
    rebuild_events_from_raw(
        source,
        path,
        session_id,
        source_sequence,
        &raw,
        include_deferred_content,
    )
}

/// 调用方已经按偏移读出了那一行，省掉从文件头数行的那一遍。
pub(super) fn rebuild_events_from_raw(
    source: Source,
    path: &Path,
    session_id: &str,
    source_sequence: u32,
    raw: &str,
    include_deferred_content: bool,
) -> Result<Vec<ConversationEvent>, String> {
    match source {
        Source::Codex | Source::Trae => rebuild_codex(
            source,
            path,
            session_id,
            source_sequence,
            raw,
            include_deferred_content,
        ),
        Source::Claude | Source::Qoder | Source::QoderCn | Source::Pi | Source::Omp => {
            rebuild_jsonl_values(
                source,
                path,
                session_id,
                source_sequence,
                raw,
                include_deferred_content,
            )
        }
        _ => Err("该来源的事件映射不是按行无上下文的".to_string()),
    }
}

pub fn parse_session_events(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<Vec<ConversationEvent>, String> {
    let (source, session, paths) = load_trusted_session_files(conn, home, source, session_id)?;
    let parsed = parse_conversation_files(
        source,
        &paths,
        &session.session_id,
        include_deferred_content,
    )?;
    ensure_matching_session(&parsed, &session)?;
    Ok(parsed.events)
}

pub fn load_event_content(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    event_id: &str,
) -> Result<ConversationEventContentDto, String> {
    let event = read_event_by_id(
        conn,
        home,
        source,
        session_id,
        event_id,
        ReadOptions::full_with_fallback(),
    )?
    .ok_or_else(|| "原始文件中未找到该事件".to_string())?;
    Ok(ConversationEventContentDto {
        event_id: event.event_id,
        text: event.text,
        details: event.details,
    })
}

fn rebuild_codex(
    source: Source,
    path: &Path,
    session_id: &str,
    source_sequence: u32,
    raw: &str,
    include_deferred_content: bool,
) -> Result<Vec<ConversationEvent>, String> {
    let parsed = super::codex::parse_content(super::codex::ParseContentOpts {
        source,
        path,
        content: &format!("{raw}\n"),
        start_byte: 0,
        start_line: source_sequence,
        tolerate_incomplete_tail: false,
        include_deferred_content,
        session_hint: Some(session_id.to_string()),
    })
    .map_err(|issue| issue.message)?;
    Ok(parsed.events)
}

fn rebuild_jsonl_values(
    source: Source,
    path: &Path,
    session_id: &str,
    source_sequence: u32,
    raw: &str,
    include_deferred_content: bool,
) -> Result<Vec<ConversationEvent>, String> {
    let value: Value = serde_json::from_str(raw.trim())
        .map_err(|error| format!("第 {} 行 JSON 无效：{error}", source_sequence + 1))?;
    let values = vec![(source_sequence as usize, value)];
    let parsed = match source {
        Source::Claude | Source::Qoder | Source::QoderCn => super::claude::parse_from_values(
            path,
            values,
            include_deferred_content,
            Some(session_id),
            true,
            false,
            source,
        )?,
        Source::Pi => {
            super::pi::parse_from_values(path, values, include_deferred_content, Some(session_id))?
        }
        Source::Omp => {
            super::omp::parse_from_values(path, values, include_deferred_content, Some(session_id))?
        }
        _ => return Err("该来源的事件映射不是按行无上下文的".to_string()),
    };
    Ok(parsed
        .events
        .into_iter()
        .filter(|event| event.source_sequence == source_sequence)
        .collect())
}

fn read_jsonl_record(path: &Path, source_sequence: u32) -> Result<String, String> {
    let raw = read_source_line(path, source_sequence)?;
    if raw.trim().is_empty() {
        return Err(format!("原始文件中未找到第 {} 行", source_sequence + 1));
    }
    Ok(raw)
}

pub(super) fn line_event_matches_index(
    rebuilt: &ConversationEvent,
    indexed: &ConversationEvent,
) -> bool {
    rebuilt.event_id == indexed.event_id
        && rebuilt.kind == indexed.kind
        && rebuilt.actor == indexed.actor
        && rebuilt.name == indexed.name
        && rebuilt.occurred_at == indexed.occurred_at
        && rebuilt.source_file == indexed.source_file
        && rebuilt.source_sequence == indexed.source_sequence
        && (indexed.content_status == ContentStatus::Deferred || rebuilt.text == indexed.text)
}
