//! 对话事件的权威读取路径：整场、分页、单条事件、计数与导出流式读取。
//!
//! 所有读取入口都共用 `readiness::is_session_ready`、`query` 索引查询（含 ADR-0025 回水与
//! `event_id` 还原），并按调用方的 `ReadOptions`（是否展开 >4KB 延迟正文、外置正文失配时
//! 回退整份解析还是整场跳过）统一决策。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{
    ConversationEvent, ConversationEventAnchor, ConversationEventContentStatus as ContentStatus,
    ConversationEventPage, ConversationSessionRow, Source,
};

use super::super::cursor;
use super::super::line_direct::{
    line_event_matches_index, parse_session_events, rebuild_events_from_line,
    source_maps_line_to_events,
};
use super::super::normalize::compare_event_order;
use super::super::read::{cursor_missing_transcript_events, load_prepared_parsed, prepare_detail};
use super::super::session_store::{
    ensure_matching_session, load_session, load_trusted_session_files,
};
use super::super::trusted_path::session_source_paths;
use super::super::{
    parse_conversation_files, PreparedConversationDetail, CONVERSATION_SOURCES, MAX_PAGE_SIZE,
};
use super::query::{
    empty_event_page, lookup_indexed_event, query_indexed_events_page, query_live_indexed_events,
    IndexedEventLookup,
};
use super::readiness::{is_session_ready, session_generation};

const TEXT_REF_MISMATCH_ERROR: &str =
    "外置正文读不回，或与索引对不上（源文件在索引之后变了），请先刷新摄取再推送";
const DEFERRED_EVENT_MISSING_ERROR: &str = "大段正文在源文件里找不到了（源文件在索引之后变了）";

/// 当已就绪索引遇到 ADR-0025 外置正文读不回或指纹失配时的处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::conversation) enum FallbackPolicy {
    /// 详情、分页、单条展开、导出、工作纪要：整份回退解析源文件。
    ParseSource,
    /// 推送（ADR 0026）：已就绪会话外置正文失配时整场跳过并给明确原因，不悄悄回退。
    /// 未就绪会话仍走整份源文件解析（ADR 0027）。
    SkipOnReferenceMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::conversation) struct ReadOptions {
    /// `false`：保留 >4KB 大段工具输出为 `Deferred` 预览（分页详情、工作纪要）；
    /// `true`：展开完整正文（单事件展开、导出、推送）。
    pub(in crate::conversation) include_deferred: bool,
    pub(in crate::conversation) fallback: FallbackPolicy,
}

impl ReadOptions {
    pub(in crate::conversation) const fn indexed_with_fallback() -> Self {
        Self {
            include_deferred: false,
            fallback: FallbackPolicy::ParseSource,
        }
    }

    pub(in crate::conversation) const fn full_with_fallback() -> Self {
        Self {
            include_deferred: true,
            fallback: FallbackPolicy::ParseSource,
        }
    }

    pub(in crate::conversation) const fn full_strict_references() -> Self {
        Self {
            include_deferred: true,
            fallback: FallbackPolicy::SkipOnReferenceMismatch,
        }
    }
}

pub(crate) enum PreparedEventsRead {
    Ready(ConversationEventPage),
    NeedsParse(Box<PreparedConversationDetail>),
}

pub(in crate::conversation) enum ExportRecord<'a> {
    Header(&'a ConversationSessionRow),
    Event(&'a ConversationEvent),
}

/// 仅读目录表持久化的事件总数（不读事件表、不碰源文件）。老库未刷新行为 `None`。
pub(crate) fn catalog_event_count(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Option<u32>, String> {
    let count = conn
        .query_row(
            r#"
            SELECT event_count
            FROM conversation_sessions
            WHERE source = ?1 AND session_id = ?2
            "#,
            params![source, session_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    Ok(count.map(|value| value.max(0) as u32))
}

/// 权威会话事件总数：已就绪走索引计数，未就绪回退整份源文件解析。
#[allow(dead_code)]
pub(in crate::conversation) fn read_event_count(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
) -> Result<u32, String> {
    let (parsed_source, session) = resolve_session(conn, source, session_id)?;
    if is_session_ready(conn, home, parsed_source, &session)? {
        return indexed_event_count(conn, parsed_source.as_str(), session_id);
    }
    let events = parse_canonical_session_events(
        conn,
        home,
        parsed_source,
        &session,
        ReadOptions::indexed_with_fallback(),
    )?;
    Ok(events.len() as u32)
}

/// 仅在调用方已经确认会话就绪时使用的快速索引计数。
pub(in crate::conversation) fn indexed_event_count(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<u32, String> {
    let Some(generation) = session_generation(conn, source, session_id)? else {
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

/// 权威整场事件读取：先验完整就绪态，按 `ReadOptions` 决定索引回水、延迟正文展开与回退策略。
pub(in crate::conversation) fn read_session_events(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    opts: ReadOptions,
) -> Result<Vec<ConversationEvent>, String> {
    let (parsed_source, session) = resolve_session(conn, source, session_id)?;
    if is_session_ready(conn, home, parsed_source, &session)? {
        match query_live_indexed_events(conn, parsed_source.as_str(), session_id)? {
            Some(mut events) => {
                if opts.include_deferred
                    && events
                        .iter()
                        .any(|event| event.content_status == ContentStatus::Deferred)
                {
                    let full =
                        parse_session_events(conn, home, parsed_source.as_str(), session_id, true)?;
                    let texts: BTreeMap<String, Option<String>> = full
                        .iter()
                        .map(|event| (event.event_id.clone(), event.text.clone()))
                        .collect();
                    let mut missing_deferred = false;
                    for event in events
                        .iter_mut()
                        .filter(|event| event.content_status == ContentStatus::Deferred)
                    {
                        if let Some(text) = texts.get(&event.event_id) {
                            event.text.clone_from(text);
                            event.content_status = ContentStatus::Complete;
                        } else {
                            missing_deferred = true;
                            break;
                        }
                    }
                    if missing_deferred {
                        return match opts.fallback {
                            FallbackPolicy::SkipOnReferenceMismatch => {
                                Err(DEFERRED_EVENT_MISSING_ERROR.to_string())
                            }
                            FallbackPolicy::ParseSource => Ok(normalize_parsed_events(full)),
                        };
                    }
                }
                return Ok(events);
            }
            None => match opts.fallback {
                FallbackPolicy::SkipOnReferenceMismatch => {
                    return Err(TEXT_REF_MISMATCH_ERROR.to_string());
                }
                FallbackPolicy::ParseSource => {}
            },
        }
    }
    parse_canonical_session_events(conn, home, parsed_source, &session, opts)
}

/// 工作纪要等常规读取方的权威整场事件入口：不展开 >4KB 延迟正文，未就绪或外置失配时回退源文件。
pub(crate) fn load_session_events(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
) -> Result<Vec<ConversationEvent>, String> {
    read_session_events(
        conn,
        home,
        source,
        session_id,
        ReadOptions::indexed_with_fallback(),
    )
}

/// 检查当前已发布代次的库内事件快照（不要求 `home`）。未发布代次返回空数组；
/// 已发布代次若外置正文回不了源文件，按库里登记的会话文件整份解析（ADR 0025）。
pub fn indexed_events(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Vec<ConversationEvent>, String> {
    if let Some(events) = query_live_indexed_events(conn, source, session_id)? {
        return Ok(events);
    }
    let parsed_source = Source::parse(source).ok_or_else(|| "未知来源".to_string())?;
    let session =
        load_session(conn, source, session_id)?.ok_or_else(|| "未找到该对话记录".to_string())?;
    let paths = session_source_paths(&session)?;
    let parsed = parse_conversation_files(parsed_source, &paths, session_id, false)?;
    Ok(normalize_parsed_events(parsed.events))
}

pub fn load_events(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    anchor: ConversationEventAnchor,
    limit: u32,
) -> Result<ConversationEventPage, String> {
    finish_prepared_events(
        home,
        prepare_events_read(conn, home, source, session_id, &anchor, limit)?,
        &anchor,
        limit,
    )
}

pub(crate) fn prepare_events_read(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    anchor: &ConversationEventAnchor,
    limit: u32,
) -> Result<PreparedEventsRead, String> {
    let prepared = prepare_detail(conn, source, session_id)?;
    if is_session_ready(conn, home, prepared.source, &prepared.session)? {
        if let Some(page) = query_indexed_events_page(
            conn,
            prepared.source.as_str(),
            &prepared.session.session_id,
            anchor,
            limit,
        )? {
            return Ok(PreparedEventsRead::Ready(page));
        }
    }
    Ok(PreparedEventsRead::NeedsParse(Box::new(prepared)))
}

pub(crate) fn finish_prepared_events(
    home: &Path,
    read: PreparedEventsRead,
    anchor: &ConversationEventAnchor,
    limit: u32,
) -> Result<ConversationEventPage, String> {
    match read {
        PreparedEventsRead::Ready(page) => Ok(page),
        PreparedEventsRead::NeedsParse(prepared) => {
            let parsed = load_prepared_parsed(home, *prepared)?;
            Ok(paginate_events(&parsed.events, anchor, limit))
        }
    }
}

/// 权威单条事件读取：`include_deferred = true` 时按行直取完整正文与 `details`（或回退整份解析）。
pub(in crate::conversation) fn read_event_by_id(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    event_id: &str,
    opts: ReadOptions,
) -> Result<Option<ConversationEvent>, String> {
    let (parsed_source, session) = resolve_session(conn, source, session_id)?;
    if is_session_ready(conn, home, parsed_source, &session)? {
        if !opts.include_deferred {
            match lookup_indexed_event(conn, parsed_source.as_str(), session_id, event_id)? {
                IndexedEventLookup::Found(event) => return Ok(Some(event)),
                IndexedEventLookup::NotFound => return Ok(None),
                IndexedEventLookup::ReferenceMismatch => match opts.fallback {
                    FallbackPolicy::SkipOnReferenceMismatch => {
                        return Err(TEXT_REF_MISMATCH_ERROR.to_string());
                    }
                    FallbackPolicy::ParseSource => {}
                },
            }
        } else if source_maps_line_to_events(parsed_source) {
            match lookup_indexed_event(conn, parsed_source.as_str(), session_id, event_id)? {
                IndexedEventLookup::Found(indexed) => {
                    if let Ok(rebuilt) = rebuild_events_from_line(
                        parsed_source,
                        Path::new(&indexed.source_file),
                        session_id,
                        indexed.source_sequence,
                        true,
                    ) {
                        if let Some(mut event) = rebuilt
                            .into_iter()
                            .find(|candidate| candidate.event_id == event_id)
                        {
                            if line_event_matches_index(&event, &indexed) {
                                event.sequence = indexed.sequence;
                                return Ok(Some(event));
                            }
                        }
                    }
                }
                IndexedEventLookup::ReferenceMismatch
                    if opts.fallback == FallbackPolicy::SkipOnReferenceMismatch =>
                {
                    return Err(TEXT_REF_MISMATCH_ERROR.to_string());
                }
                IndexedEventLookup::NotFound | IndexedEventLookup::ReferenceMismatch => {}
            }
        }
    }
    let (parsed_source, session, paths) =
        load_trusted_session_files(conn, home, source, session_id)?;
    let parsed =
        parse_conversation_files(parsed_source, &paths, session_id, opts.include_deferred)?;
    ensure_matching_session(&parsed, &session)?;
    let events = normalize_parsed_events(parsed.events);
    Ok(events.into_iter().find(|event| event.event_id == event_id))
}

/// 供附件解析按就绪索引快速定位单条事件。未就绪或外置正文校验不通过时返回 `None`，由调用方回退整份解析。
pub(in crate::conversation) fn read_indexed_event_if_ready(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    event_id: &str,
) -> Result<Option<(PreparedConversationDetail, ConversationEvent)>, String> {
    let prepared = prepare_detail(conn, source, session_id)?;
    if !source_maps_line_to_events(prepared.source)
        || !is_session_ready(conn, home, prepared.source, &prepared.session)?
    {
        return Ok(None);
    }
    match lookup_indexed_event(conn, prepared.source.as_str(), session_id, event_id)? {
        IndexedEventLookup::Found(event) => Ok(Some((prepared, event))),
        IndexedEventLookup::NotFound | IndexedEventLookup::ReferenceMismatch => Ok(None),
    }
}

/// Markdown 导出专用流式读取：已就绪且支持按行重建的来源按页读取并展开正文与 `details`；
/// 首页遇到外置正文校验失配、未就绪或非按行映射来源时回退整份源文件解析。
pub(in crate::conversation) fn stream_events_for_export(
    conn: &Connection,
    home: &Path,
    source: Source,
    session: &ConversationSessionRow,
    paths: &[PathBuf],
    mut emit: impl FnMut(ExportRecord<'_>) -> Result<(), String>,
) -> Result<(), String> {
    if is_session_ready(conn, home, source, session)? && source_maps_line_to_events(source) {
        if let Some(first_page) = query_indexed_events_page(
            conn,
            source.as_str(),
            &session.session_id,
            &ConversationEventAnchor::First,
            MAX_PAGE_SIZE,
        )? {
            let first_hydrated = first_page
                .events
                .iter()
                .map(|event| hydrate_export_event(source, session, event))
                .collect::<Result<Vec<_>, _>>();
            if let Ok(first_events) = first_hydrated {
                emit(ExportRecord::Header(session))?;
                for event in &first_events {
                    emit(ExportRecord::Event(event))?;
                }
                if !first_page.has_more_after {
                    return Ok(());
                }
                let Some(mut last_sequence) = first_page.events.last().map(|event| event.sequence)
                else {
                    return Ok(());
                };
                loop {
                    let anchor = ConversationEventAnchor::After {
                        sequence: last_sequence,
                    };
                    let page = query_indexed_events_page(
                        conn,
                        source.as_str(),
                        &session.session_id,
                        &anchor,
                        MAX_PAGE_SIZE,
                    )?
                    .ok_or_else(|| "原始文件在导出期间发生变化，请重试".to_string())?;
                    if page.events.is_empty() {
                        break;
                    }
                    for event in &page.events {
                        let hydrated = hydrate_export_event(source, session, event)?;
                        emit(ExportRecord::Event(&hydrated))?;
                    }
                    if !page.has_more_after {
                        break;
                    }
                    let Some(last) = page.events.last() else {
                        break;
                    };
                    last_sequence = last.sequence;
                }
                return Ok(());
            }
        }
    }
    let parsed = parse_conversation_files(source, paths, &session.session_id, true)?;
    ensure_matching_session(&parsed, session)?;
    emit(ExportRecord::Header(&parsed.session))?;
    for event in &parsed.events {
        emit(ExportRecord::Event(event))?;
    }
    Ok(())
}

pub(crate) fn paginate_events(
    events: &[ConversationEvent],
    anchor: &ConversationEventAnchor,
    limit: u32,
) -> ConversationEventPage {
    let limit = limit.clamp(1, MAX_PAGE_SIZE) as usize;
    if events.is_empty() {
        return empty_event_page();
    }

    let (start, end) = match anchor {
        ConversationEventAnchor::First => (0, limit.min(events.len())),
        ConversationEventAnchor::Last => (events.len().saturating_sub(limit), events.len()),
        ConversationEventAnchor::Before { sequence } => {
            let prefix_end = events
                .iter()
                .position(|event| event.sequence >= *sequence)
                .unwrap_or(events.len());
            (prefix_end.saturating_sub(limit), prefix_end)
        }
        ConversationEventAnchor::After { sequence } => {
            let start = events
                .iter()
                .position(|event| event.sequence > *sequence)
                .unwrap_or(events.len());
            (start, (start + limit).min(events.len()))
        }
        ConversationEventAnchor::Around { sequence } => {
            let start = events
                .iter()
                .position(|event| event.sequence >= *sequence)
                .unwrap_or(events.len());
            (start, (start + limit).min(events.len()))
        }
    };

    if start >= end {
        let (has_more_before, has_more_after) = match anchor {
            ConversationEventAnchor::First | ConversationEventAnchor::Last => (false, false),
            ConversationEventAnchor::Before { sequence } => (
                false,
                events.iter().any(|event| event.sequence >= *sequence),
            ),
            ConversationEventAnchor::After { sequence } => (
                events.iter().any(|event| event.sequence <= *sequence),
                false,
            ),
            ConversationEventAnchor::Around { sequence } => (
                events.iter().any(|event| event.sequence < *sequence),
                events.iter().any(|event| event.sequence >= *sequence),
            ),
        };
        return ConversationEventPage {
            events: Vec::new(),
            has_more_before,
            has_more_after,
        };
    }

    let page = events[start..end].to_vec();
    let min_sequence = page[0].sequence;
    let max_sequence = page[page.len() - 1].sequence;
    ConversationEventPage {
        events: page,
        has_more_before: events.iter().any(|event| event.sequence < min_sequence),
        has_more_after: events.iter().any(|event| event.sequence > max_sequence),
    }
}

fn hydrate_export_event(
    source: Source,
    session: &ConversationSessionRow,
    indexed: &ConversationEvent,
) -> Result<ConversationEvent, String> {
    let rebuilt = rebuild_events_from_line(
        source,
        Path::new(&indexed.source_file),
        &session.session_id,
        indexed.source_sequence,
        true,
    )?;
    let mut event = rebuilt
        .into_iter()
        .find(|event| event.event_id == indexed.event_id)
        .ok_or_else(|| "原始文件中未找到该事件".to_string())?;
    event.sequence = indexed.sequence;
    Ok(event)
}

fn resolve_session(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<(Source, ConversationSessionRow), String> {
    let parsed_source = Source::parse(source)
        .filter(|source| CONVERSATION_SOURCES.contains(source))
        .ok_or_else(|| "该来源尚未支持对话详情".to_string())?;
    let session = load_session(conn, parsed_source.as_str(), session_id)?
        .ok_or_else(|| "未找到该对话记录".to_string())?;
    Ok((parsed_source, session))
}

fn parse_canonical_session_events(
    conn: &Connection,
    home: &Path,
    source: Source,
    session: &ConversationSessionRow,
    opts: ReadOptions,
) -> Result<Vec<ConversationEvent>, String> {
    let source_path = Path::new(&session.source_file);
    if opts.fallback == FallbackPolicy::ParseSource
        && source == Source::CursorAgent
        && (!cursor::is_native_transcript(source_path) || !source_path.is_file())
    {
        return Ok(cursor_missing_transcript_events(session));
    }
    let (parsed_source, loaded_session, paths) =
        load_trusted_session_files(conn, home, source.as_str(), &session.session_id)?;
    let parsed = parse_conversation_files(
        parsed_source,
        &paths,
        &loaded_session.session_id,
        opts.include_deferred,
    )?;
    ensure_matching_session(&parsed, &loaded_session)?;
    Ok(normalize_parsed_events(parsed.events))
}

fn normalize_parsed_events(mut events: Vec<ConversationEvent>) -> Vec<ConversationEvent> {
    events.sort_by(compare_event_order);
    for (sequence, event) in events.iter_mut().enumerate() {
        event.sequence = sequence as u32;
    }
    events
}
