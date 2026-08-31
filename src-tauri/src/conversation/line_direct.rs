use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use super::toolbox::{attachment_candidates, AttachmentCandidate};
use super::{
    ensure_attachment_path_allowed, ensure_matching_session, event_index, event_index_ready,
    load_trusted_session_files, parse_codex_content, parse_conversation_files, prepare_detail,
    read_source_line, read_source_payload, AttachmentKind,
};
use crate::domain::{
    ConversationEvent, ConversationEventContentDto,
    ConversationEventContentStatus as ContentStatus, Source,
};

pub(super) fn source_maps_line_to_events(source: Source) -> bool {
    matches!(source, Source::Codex | Source::Claude | Source::Pi)
}

pub fn rebuild_events_from_line(
    source: Source,
    path: &Path,
    session_id: &str,
    source_sequence: u32,
    include_deferred_content: bool,
) -> Result<Vec<ConversationEvent>, String> {
    match source {
        Source::Codex => rebuild_codex(path, session_id, source_sequence, include_deferred_content),
        Source::Claude | Source::Pi => rebuild_jsonl_values(
            source,
            path,
            session_id,
            source_sequence,
            include_deferred_content,
        ),
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

pub fn try_load_event_content(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    event_id: &str,
) -> Result<Option<ConversationEventContentDto>, String> {
    let prepared = prepare_detail(conn, source, session_id)?;
    if !source_maps_line_to_events(prepared.source) || !event_index_ready(conn, home, &prepared)? {
        return Ok(None);
    }
    let Some(indexed) = event_index::indexed_event(conn, source, session_id, event_id)? else {
        return Ok(None);
    };
    let rebuilt = match rebuild_events_from_line(
        prepared.source,
        Path::new(&indexed.source_file),
        session_id,
        indexed.source_sequence,
        true,
    ) {
        Ok(events) => events,
        Err(_) => return Ok(None),
    };
    let Some(event) = rebuilt.into_iter().find(|event| event.event_id == event_id) else {
        return Ok(None);
    };
    if !line_event_matches_index(&event, &indexed) {
        return Ok(None);
    }
    Ok(Some(ConversationEventContentDto {
        event_id: event.event_id,
        text: event.text,
        details: event.details,
    }))
}

pub fn try_resolve_attachment(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    attachment_id: &str,
) -> Result<Option<AttachmentCandidate>, String> {
    let Some((event_id, attachment_index)) = split_attachment_id(attachment_id) else {
        return Ok(None);
    };
    let prepared = prepare_detail(conn, source, session_id)?;
    if !source_maps_line_to_events(prepared.source) || !event_index_ready(conn, home, &prepared)? {
        return Ok(None);
    }
    let Some(indexed) = event_index::indexed_event(conn, source, session_id, event_id)? else {
        return Ok(None);
    };
    let Some(attachment) = indexed.attachments.get(attachment_index).cloned() else {
        return Ok(None);
    };
    if attachment.kind != AttachmentKind::Image {
        return Err("该附件不是可预览的图片".to_string());
    }
    let source_path = PathBuf::from(&indexed.source_file);
    let Ok(payload) = read_source_payload(prepared.source, &source_path, indexed.source_sequence)
    else {
        return Ok(None);
    };
    let Some(mut candidate) =
        attachment_candidates(indexed.source_sequence, &payload, &prepared.session.project)
            .into_iter()
            .nth(attachment_index)
    else {
        return Ok(None);
    };
    candidate.attachment = attachment;
    ensure_attachment_path_allowed(&candidate, &prepared.session.project)?;
    Ok(Some(candidate))
}

fn rebuild_codex(
    path: &Path,
    session_id: &str,
    source_sequence: u32,
    include_deferred_content: bool,
) -> Result<Vec<ConversationEvent>, String> {
    let raw = read_jsonl_record(path, source_sequence)?;
    let parsed = parse_codex_content(
        path,
        &format!("{raw}\n"),
        0,
        source_sequence,
        false,
        include_deferred_content,
        Some(session_id.to_string()),
    )
    .map_err(|issue| issue.message)?;
    Ok(parsed.events)
}

fn rebuild_jsonl_values(
    source: Source,
    path: &Path,
    session_id: &str,
    source_sequence: u32,
    include_deferred_content: bool,
) -> Result<Vec<ConversationEvent>, String> {
    let raw = read_jsonl_record(path, source_sequence)?;
    let value: Value = serde_json::from_str(raw.trim())
        .map_err(|error| format!("第 {} 行 JSON 无效：{error}", source_sequence + 1))?;
    let values = vec![(source_sequence as usize, value)];
    let parsed = match source {
        Source::Claude => super::claude::parse_from_values(
            path,
            values,
            include_deferred_content,
            Some(session_id),
            true,
        )?,
        Source::Pi => {
            super::pi::parse_from_values(path, values, include_deferred_content, Some(session_id))?
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

fn line_event_matches_index(rebuilt: &ConversationEvent, indexed: &ConversationEvent) -> bool {
    rebuilt.event_id == indexed.event_id
        && rebuilt.kind == indexed.kind
        && rebuilt.actor == indexed.actor
        && rebuilt.name == indexed.name
        && rebuilt.occurred_at == indexed.occurred_at
        && rebuilt.source_file == indexed.source_file
        && rebuilt.source_sequence == indexed.source_sequence
        && (indexed.content_status == ContentStatus::Deferred || rebuilt.text == indexed.text)
}

fn split_attachment_id(attachment_id: &str) -> Option<(&str, usize)> {
    let (event_id, index) = attachment_id.rsplit_once(':')?;
    Some((event_id, index.parse().ok()?))
}
