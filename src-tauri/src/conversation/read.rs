use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{
    ConversationAttachmentContentDto, ConversationAttachmentKind as AttachmentKind,
    ConversationDetailDto, ConversationDetailStateDto, ConversationEvent,
    ConversationEventContentDto, ConversationEventKind as EventKind, ConversationIndexProgressDto,
    ConversationParsedDetail, ConversationSessionRow, CursorSessionDetailDto, CursorSessionRecord,
    Source, UsageRecord,
};
use crate::ingest;

use super::agent_graph::load_agent_relations;
use super::attachments::{
    attachment_data_url, attachment_thumbnail_data_url, ensure_attachment_path_allowed,
    read_source_payload,
};
use super::merge::{merge_indexed_files, summarize_for_index};
use super::session_store::{
    ensure_matching_session, load_session, load_trusted_session_files, load_usage_records,
    update_session_files, upsert_session, usage_record_identity,
};
use super::toolbox::{
    attachment_candidates, compare_event_order, semantic_event, AttachmentCandidate,
    ParsedConversation,
};
use super::trusted_path::{
    detail_file_revision, detail_files_revision, files_revision, modified_nanos,
    session_source_paths, trusted_paths_for_session,
};
use super::{
    conversation_adapter, cursor, event_index, line_direct, parse_conversation_file,
    parse_conversation_files, persist_session_file_cursors, write_session_file_events,
    PreparedConversationDetail, PreparedDetailRead, CONVERSATION_ADAPTER_VERSION,
    CONVERSATION_SOURCES, DETAIL_READ_ATTEMPTS,
};

pub fn load_detail(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
) -> Result<ConversationDetailDto, String> {
    finish_prepared_detail(home, prepare_detail_read(conn, home, source, session_id)?)
}

pub(crate) fn prepare_detail_read(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
) -> Result<PreparedDetailRead, String> {
    let prepared = prepare_detail(conn, source, session_id)?;
    if event_index_ready(conn, home, &prepared)? {
        let event_count = event_index::indexed_event_count(conn, source, session_id)?;
        return Ok(PreparedDetailRead::Indexed {
            prepared,
            event_count,
        });
    }
    Ok(PreparedDetailRead::Parsed { prepared })
}

pub(crate) fn finish_prepared_detail(
    home: &Path,
    read: PreparedDetailRead,
) -> Result<ConversationDetailDto, String> {
    match read {
        PreparedDetailRead::Indexed {
            prepared,
            event_count,
        } => assemble_indexed_detail(home, prepared, event_count),
        PreparedDetailRead::Parsed { prepared } => load_prepared_detail(home, prepared),
    }
}

/// 始终整份解析源文件，供差分基准与回退路径使用。
pub fn load_parsed_detail(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
) -> Result<ConversationParsedDetail, String> {
    let prepared = prepare_detail(conn, source, session_id)?;
    load_prepared_parsed(home, prepared)
}

pub(crate) fn prepare_detail(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<PreparedConversationDetail, String> {
    let source = Source::parse(source).filter(|source| CONVERSATION_SOURCES.contains(source));
    let Some(source) = source else {
        return Err("该来源尚未支持对话详情".to_string());
    };
    let session = load_session(conn, source.as_str(), session_id)?
        .ok_or_else(|| "未找到该对话记录".to_string())?;
    let usage_records = load_usage_records(conn, source, session_id)?;
    let agent_relations = load_agent_relations(conn, source, session_id, &[])?;
    let cursor_session_stats = if source == Source::CursorAgent {
        load_exact_cursor_session(conn, session_id)?
    } else {
        None
    };
    Ok(PreparedConversationDetail {
        source,
        session,
        usage_records,
        agent_relations,
        cursor_session_stats,
    })
}

pub(crate) fn load_prepared_detail(
    home: &Path,
    prepared: PreparedConversationDetail,
) -> Result<ConversationDetailDto, String> {
    let usage_record_count = prepared.usage_records.len() as u32;
    Ok(parsed_detail_to_dto(
        load_prepared_parsed(home, prepared)?,
        usage_record_count,
    ))
}

pub(crate) fn load_prepared_parsed(
    home: &Path,
    prepared: PreparedConversationDetail,
) -> Result<ConversationParsedDetail, String> {
    let PreparedConversationDetail {
        source,
        mut session,
        usage_records,
        agent_relations,
        cursor_session_stats,
    } = prepared;
    let source_path = Path::new(&session.source_file);
    let cursor_behavior = cursor_behavior_dto(home, cursor_session_stats.as_ref());
    if source == Source::CursorAgent
        && (!cursor::is_native_transcript(source_path) || !source_path.is_file())
    {
        session.file_available = false;
        let events = cursor_missing_transcript_events(&session);
        return Ok(ConversationParsedDetail {
            revision: cursor_metadata_revision(&usage_records, cursor_session_stats.as_ref()),
            session,
            events,
            agent_relations,
            cursor_behavior,
        });
    }
    let paths = trusted_paths_for_session(home, source, &session)?;
    let (parsed, revision) =
        parse_conversation_files_with_revision(source, &paths, &session.session_id)?;
    ensure_matching_session(&parsed, &session)?;
    session.file_available = true;
    session.source_files = parsed.session.source_files.clone();
    let mut events = parsed.events;
    events.sort_by(compare_event_order);
    for (sequence, event) in events.iter_mut().enumerate() {
        event.sequence = sequence as u32;
    }
    Ok(ConversationParsedDetail {
        revision,
        session,
        events,
        agent_relations,
        cursor_behavior,
    })
}

pub(crate) fn parsed_detail_to_dto(
    parsed: ConversationParsedDetail,
    usage_record_count: u32,
) -> ConversationDetailDto {
    ConversationDetailDto {
        revision: parsed.revision,
        session: parsed.session,
        event_count: parsed.events.len() as u32,
        usage_record_count,
        agent_relations: parsed.agent_relations,
        cursor_behavior: parsed.cursor_behavior,
    }
}

pub(crate) fn event_index_ready(
    conn: &Connection,
    home: &Path,
    prepared: &PreparedConversationDetail,
) -> Result<bool, String> {
    let row = conn
        .query_row(
            r#"
            SELECT adapter_version, event_index_generation
            FROM conversation_sessions
            WHERE source = ?1 AND session_id = ?2
            "#,
            params![prepared.source.as_str(), prepared.session.session_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((adapter_version, Some(_))) = row else {
        return Ok(false);
    };
    if adapter_version != CONVERSATION_ADAPTER_VERSION {
        return Ok(false);
    }
    let Ok(paths) = trusted_paths_for_session(home, prepared.source, &prepared.session) else {
        return Ok(false);
    };
    stored_revisions_match(conn, prepared.source, &prepared.session.session_id, &paths)
}

pub(crate) fn assemble_indexed_detail(
    home: &Path,
    prepared: PreparedConversationDetail,
    event_count: u32,
) -> Result<ConversationDetailDto, String> {
    let PreparedConversationDetail {
        source,
        mut session,
        usage_records,
        agent_relations,
        cursor_session_stats,
    } = prepared;
    let cursor_behavior = cursor_behavior_dto(home, cursor_session_stats.as_ref());
    let paths = trusted_paths_for_session(home, source, &session)?;
    let revision = files_revision(source, &paths)?;
    session.file_available = true;
    session.source_files = paths
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();
    Ok(ConversationDetailDto {
        revision,
        session,
        event_count,
        usage_record_count: usage_records.len() as u32,
        agent_relations,
        cursor_behavior,
    })
}

pub fn event_index_progress(conn: &Connection) -> Result<ConversationIndexProgressDto, String> {
    let (total, indexed) = conn
        .query_row(
            r#"
            SELECT
                COUNT(*) AS total,
                COALESCE(SUM(
                    CASE
                        WHEN adapter_version = ?1 AND event_index_generation IS NOT NULL THEN 1
                        ELSE 0
                    END
                ), 0) AS indexed
            FROM conversation_sessions
            WHERE file_available = 1 AND source_revision != 'usage-only'
            "#,
            params![CONVERSATION_ADAPTER_VERSION],
            |row| Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u32)),
        )
        .map_err(|error| error.to_string())?;
    Ok(ConversationIndexProgressDto {
        indexed,
        total,
        index_bytes: conversation_index_bytes(conn, indexed >= total || total == 0),
    })
}

pub(crate) fn conversation_index_bytes(conn: &Connection, complete: bool) -> u64 {
    if let Ok(bytes) = conn.query_row(
        "SELECT COALESCE(SUM(pgsize), 0) FROM dbstat
         WHERE name GLOB 'conversation_events*'
            OR name IN ('conversation_files', 'conversation_session_tools')",
        [],
        |row| row.get::<_, i64>(0),
    ) {
        return bytes.max(0) as u64;
    }
    if !complete {
        return 0;
    }
    conn.query_row(
        "SELECT COALESCE(SUM(LENGTH(COALESCE(text, '')) + LENGTH(COALESCE(name, ''))), 0)
         FROM conversation_events",
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
    .max(0) as u64
}

pub fn backfill_event_index_step(conn: &Connection, home: &Path) -> Result<bool, String> {
    match backfill_event_index_step_skipping(conn, home, &BTreeSet::new()) {
        Ok(progressed) => Ok(progressed),
        Err((_, error)) => Err(error),
    }
}

pub(crate) fn backfill_event_index_step_skipping(
    conn: &Connection,
    home: &Path,
    skipped: &BTreeSet<(String, String)>,
) -> Result<bool, ((String, String), String)> {
    let next = next_unready_session(conn, skipped)
        .map_err(|error| ((String::new(), String::new()), error))?;
    let Some((source, session_id)) = next else {
        return Ok(false);
    };
    match reindex_session_events(conn, home, &source, &session_id) {
        Ok(()) => Ok(true),
        Err(error) => Err(((source, session_id), error)),
    }
}

pub fn backfill_event_index(conn: &Connection, home: &Path) -> Result<u32, String> {
    let mut completed = 0;
    let mut skipped = BTreeSet::new();
    while let Some((source, session_id)) = next_unready_session(conn, &skipped)? {
        match reindex_session_events(conn, home, &source, &session_id) {
            Ok(()) => completed += 1,
            Err(_) => {
                skipped.insert((source, session_id));
            }
        }
    }
    Ok(completed)
}

pub(crate) fn next_unready_session(
    conn: &Connection,
    skipped: &BTreeSet<(String, String)>,
) -> Result<Option<(String, String)>, String> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT source, session_id
            FROM conversation_sessions
            WHERE file_available = 1
              AND source_revision != 'usage-only'
              AND (adapter_version != ?1 OR event_index_generation IS NULL)
            ORDER BY ended_at DESC, source ASC, session_id ASC
            "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![CONVERSATION_ADAPTER_VERSION], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows.into_iter().find(|key| !skipped.contains(key)))
}

pub(crate) fn reindex_session_events(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
) -> Result<(), String> {
    let source = Source::parse(source).filter(|source| CONVERSATION_SOURCES.contains(source));
    let Some(source) = source else {
        return Err("该来源尚未支持对话详情".to_string());
    };
    let session = load_session(conn, source.as_str(), session_id)?
        .ok_or_else(|| "未找到该对话记录".to_string())?;
    let paths = trusted_paths_for_session(home, source, &session)?;
    let adapter = conversation_adapter(source)?;
    let mut event_generations = BTreeMap::new();
    let mut indexed_files = Vec::new();
    let mut file_cursors = BTreeMap::new();
    for path in &paths {
        let batch = (adapter.index)(path).map_err(|issue| issue.message)?;
        for parsed in batch.conversations {
            if parsed.session.session_id != session_id {
                continue;
            }
            if let Some(cursor) = parsed.index_cursor {
                file_cursors.insert(
                    (session_id.to_string(), parsed.session.source_file.clone()),
                    cursor,
                );
            }
            write_session_file_events(conn, source, &parsed, &mut event_generations)?;
            indexed_files.push(summarize_for_index(parsed));
        }
    }
    if indexed_files.is_empty() {
        return Err(format!("会话 {session_id} 的源文件没有可索引的对话"));
    }
    let source_files = indexed_files
        .iter()
        .map(|file| PathBuf::from(&file.session.source_file))
        .collect::<Vec<_>>();
    let (merged_session, is_top_level, agent_metadata) = merge_indexed_files(indexed_files);
    let representative_metadata = fs::metadata(&merged_session.source_file)
        .map_err(|error| format!("读取文件元数据失败：{error}"))?;
    let representative_revision = (adapter.revision)(Path::new(&merged_session.source_file))?;
    if let Some(&generation) = event_generations.get(session_id) {
        event_index::finalize_session_events(conn, source, session_id, generation)?;
    }
    upsert_session(
        conn,
        &merged_session,
        is_top_level,
        &agent_metadata,
        modified_nanos(&representative_metadata),
        representative_metadata.len() as i64,
        &representative_revision,
    )?;
    update_session_files(conn, source, session_id, &source_files, true)?;
    persist_session_file_cursors(conn, source, session_id, &source_files, &file_cursors)?;
    Ok(())
}

pub(crate) fn stored_revisions_match(
    conn: &Connection,
    source: Source,
    session_id: &str,
    paths: &[PathBuf],
) -> Result<bool, String> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT source_file, source_revision
            FROM conversation_session_files
            WHERE source = ?1 AND session_id = ?2
            "#,
        )
        .map_err(|error| error.to_string())?;
    let stored = statement
        .query_map(params![source.as_str(), session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|error| error.to_string())?;
    let adapter = conversation_adapter(source)?;
    if stored.is_empty() {
        let stored_revision = conn
            .query_row(
                r#"
                SELECT source_revision
                FROM conversation_sessions
                WHERE source = ?1 AND session_id = ?2
                "#,
                params![source.as_str(), session_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| error.to_string())?;
        return Ok(files_revision(source, paths)? == stored_revision);
    }
    if stored.len() != paths.len() {
        return Ok(false);
    }
    for path in paths {
        let key = path.to_string_lossy().to_string();
        let Some(stored_revision) = stored.get(&key) else {
            return Ok(false);
        };
        if &(adapter.revision)(path)? != stored_revision {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn load_exact_cursor_session(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<CursorSessionRecord>, String> {
    let matches = crate::store::load_cursor_sessions(conn)?
        .into_iter()
        .filter(|record| record.session_id == session_id)
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.into_iter().next()),
        _ => Err(format!(
            "Cursor 会话 ID {session_id} 对应多个行为记录，无法确定性关联"
        )),
    }
}

pub(crate) fn cursor_behavior_dto(
    home: &Path,
    stats: Option<&CursorSessionRecord>,
) -> Option<CursorSessionDetailDto> {
    stats.map(|record| crate::cursor_session_detail::detail_from_record(home, record))
}

pub(crate) fn cursor_missing_transcript_events(
    session: &ConversationSessionRow,
) -> Vec<ConversationEvent> {
    let mut event = semantic_event(
        0,
        EventKind::SystemStatus,
        &session.ended_at,
        None,
        Some("transcript_missing".to_string()),
        Some("Cursor transcript 不可读取；仅展示确定性关联的用量与状态".to_string()),
        serde_json::json!({"session_id": session.session_id}),
    );
    event.event_id = format!("cursor-transcript-missing:{}", session.session_id);
    event.source_file = session.source_file.clone();
    vec![event]
}

pub(crate) fn cursor_metadata_revision(
    usage_records: &[UsageRecord],
    stats: Option<&CursorSessionRecord>,
) -> String {
    serde_json::to_string(&(
        usage_records
            .iter()
            .map(usage_record_identity)
            .collect::<Vec<_>>(),
        stats,
    ))
    .unwrap_or_default()
}

/// Cursor Agent 对话在 `~/.cursor/projects`，与 token 包装目录不是同一条路径。
pub(crate) fn catalog_roots(
    overrides: &crate::ingest::PathOverrides,
    home: &Path,
    source: Source,
) -> Vec<PathBuf> {
    if source == Source::CursorAgent {
        vec![home.join(".cursor/projects")]
    } else {
        ingest::source_scan_dirs_with(overrides, home, source)
    }
}

pub(crate) fn conversation_source_roots(home: &Path, source: Source) -> Vec<PathBuf> {
    catalog_roots(&ingest::path_overrides(), home, source)
}

pub fn detail_state(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    known_revision: &str,
) -> Result<ConversationDetailStateDto, String> {
    let source = Source::parse(source).filter(|source| CONVERSATION_SOURCES.contains(source));
    let Some(source) = source else {
        return Err("该来源尚未支持对话详情".to_string());
    };
    let session = load_session(conn, source.as_str(), session_id)?
        .ok_or_else(|| "未找到该对话记录".to_string())?;
    let representative = PathBuf::from(&session.source_file);
    if source == Source::CursorAgent
        && (!cursor::is_native_transcript(&representative) || !representative.is_file())
    {
        let usage_records = load_usage_records(conn, source, session_id)?;
        let stats = load_exact_cursor_session(conn, session_id)?;
        let revision = cursor_metadata_revision(&usage_records, stats.as_ref());
        return Ok(ConversationDetailStateDto {
            changed: revision != known_revision,
            revision,
            file_available: false,
        });
    }
    let roots = conversation_source_roots(home, source);
    let Some(_) = detail_file_revision(source, &representative, &roots)? else {
        return Ok(ConversationDetailStateDto {
            revision: known_revision.to_string(),
            changed: false,
            file_available: false,
        });
    };
    let paths = session_source_paths(&session)?;
    let Some(revision) = detail_files_revision(source, &paths, &roots)? else {
        return Ok(ConversationDetailStateDto {
            revision: known_revision.to_string(),
            changed: false,
            file_available: false,
        });
    };
    Ok(ConversationDetailStateDto {
        changed: revision != known_revision,
        revision,
        file_available: true,
    })
}

pub(crate) fn parse_conversation_files_with_revision(
    source: Source,
    paths: &[PathBuf],
    session_id: &str,
) -> Result<(ParsedConversation, String), String> {
    read_consistent_snapshot(
        || files_revision(source, paths),
        || parse_conversation_files(source, paths, session_id, false),
    )
}

pub(crate) fn read_consistent_snapshot<T>(
    mut revision: impl FnMut() -> Result<String, String>,
    mut read: impl FnMut() -> Result<T, String>,
) -> Result<(T, String), String> {
    for _ in 0..DETAIL_READ_ATTEMPTS {
        let before_revision = revision()?;
        let snapshot = read();
        let after_revision = revision()?;
        if after_revision != before_revision {
            continue;
        }
        return snapshot.map(|snapshot| (snapshot, after_revision));
    }
    Err("原始文件在读取期间持续变化，请重试".to_string())
}

pub fn rebuild_events_from_line(
    source: Source,
    path: &Path,
    session_id: &str,
    source_sequence: u32,
    include_deferred_content: bool,
) -> Result<Vec<ConversationEvent>, String> {
    line_direct::rebuild_events_from_line(
        source,
        path,
        session_id,
        source_sequence,
        include_deferred_content,
    )
}

pub fn parse_session_events(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<Vec<ConversationEvent>, String> {
    line_direct::parse_session_events(conn, home, source, session_id, include_deferred_content)
}

pub fn load_event_content(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    event_id: &str,
) -> Result<ConversationEventContentDto, String> {
    if let Some(content) =
        line_direct::try_load_event_content(conn, home, source, session_id, event_id)?
    {
        return Ok(content);
    }
    let (source, session, paths) = load_trusted_session_files(conn, home, source, session_id)?;
    let parsed = parse_conversation_files(source, &paths, session_id, true)?;
    ensure_matching_session(&parsed, &session)?;
    let event = parsed
        .events
        .into_iter()
        .find(|event| event.event_id == event_id)
        .ok_or_else(|| "原始文件中未找到该事件".to_string())?;
    Ok(ConversationEventContentDto {
        event_id: event.event_id,
        text: event.text,
        details: event.details,
    })
}

pub fn load_attachment(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    attachment_id: &str,
) -> Result<ConversationAttachmentContentDto, String> {
    let candidate = resolve_attachment(conn, home, source, session_id, attachment_id)?;
    let data_url = attachment_data_url(&candidate)?;
    Ok(ConversationAttachmentContentDto {
        attachment: candidate.attachment,
        data_url,
    })
}

pub fn load_attachment_thumbnail(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    attachment_id: &str,
) -> Result<ConversationAttachmentContentDto, String> {
    let candidate = resolve_attachment(conn, home, source, session_id, attachment_id)?;
    let data_url = attachment_thumbnail_data_url(&candidate)?;
    Ok(ConversationAttachmentContentDto {
        attachment: candidate.attachment,
        data_url,
    })
}

pub(crate) fn resolve_attachment(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    attachment_id: &str,
) -> Result<AttachmentCandidate, String> {
    if let Some(candidate) =
        line_direct::try_resolve_attachment(conn, home, source, session_id, attachment_id)?
    {
        return Ok(candidate);
    }
    let (source, session, paths) = load_trusted_session_files(conn, home, source, session_id)?;
    let parsed = parse_conversation_files(source, &paths, session_id, true)?;
    ensure_matching_session(&parsed, &session)?;
    let event = parsed
        .events
        .iter()
        .find(|event| {
            event
                .attachments
                .iter()
                .any(|attachment| attachment.id == attachment_id)
        })
        .ok_or_else(|| "原始文件中未找到该附件".to_string())?;
    let attachment_index = event
        .attachments
        .iter()
        .position(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| "原始文件中未找到该附件".to_string())?;
    let attachment = event.attachments[attachment_index].clone();
    if attachment.kind != AttachmentKind::Image {
        return Err("该附件不是可预览的图片".to_string());
    }
    let source_path = PathBuf::from(&event.source_file);
    let source_fragment = parse_conversation_file(source, &source_path, session_id, true)?;
    let payload = read_source_payload(source, &source_path, event.source_sequence)?;
    let mut candidate = attachment_candidates(
        event.source_sequence,
        &payload,
        &source_fragment.session.project,
    )
    .into_iter()
    .nth(attachment_index)
    .ok_or_else(|| "原始文件中未找到该附件".to_string())?;
    candidate.attachment = attachment;
    ensure_attachment_path_allowed(&candidate, &source_fragment.session.project)?;
    Ok(candidate)
}
