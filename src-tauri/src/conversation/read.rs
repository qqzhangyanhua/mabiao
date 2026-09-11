//! 打开一条对话记录详情。
//!
//! 公开面只做取详情：`load_detail` / `load_parsed_detail` / `detail_state`，
//! 以及命令层放连接用的「准备」与「收尾」。DTO 拼装只有 `assemble_detail` 一处。
//! 索引与解析分流、Cursor transcript 缺失、上下文清单接线都留在 implementation 里。
//! 事件分页、按行重建、导出只用「准备」，不走「收尾」。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{
    ConversationAgentRelations, ConversationContextItem, ConversationDetailDto,
    ConversationDetailStateDto, ConversationEvent, ConversationEventKind as EventKind,
    ConversationParsedDetail, ConversationSessionRow, CursorSessionDetailDto, CursorSessionRecord,
    Source, UsageRecord,
};

use super::agent_graph::load_agent_relations;
use super::scan_roots::conversation_source_roots;
use super::session_store::{
    ensure_matching_session, load_session, load_usage_records, usage_record_identity,
};
use super::toolbox::{compare_event_order, semantic_event, ParsedConversation};
use super::trusted_path::{
    detail_file_revision, detail_files_revision, files_revision, session_source_paths,
    trusted_paths_for_session,
};
use super::{
    context_cache, context_first_use, context_manifest, conversation_adapter, cursor, event_index,
    parse_conversation_files, PreparedConversationDetail, PreparedDetailRead,
    CONVERSATION_ADAPTER_VERSION, CONVERSATION_SOURCES, DETAIL_READ_ATTEMPTS,
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
    let context_metrics = if context_manifest::supported_source(prepared.source) {
        context_cache::load(conn, prepared.source, session_id)?
    } else {
        None
    };
    if event_index_ready(conn, home, &prepared)? {
        let event_count = event_index::indexed_event_count(conn, source, session_id)?;
        let supported = context_manifest::supported_source(prepared.source);
        let observed_context = if supported {
            context_manifest::observed_from_index(conn, prepared.source, session_id)?
        } else {
            Vec::new()
        };
        let first_use_events = if supported {
            context_first_use::candidates_from_index(conn, prepared.source, session_id)?
        } else {
            Vec::new()
        };
        return Ok(PreparedDetailRead::Indexed {
            prepared,
            event_count,
            observed_context,
            context_metrics,
            first_use_events,
        });
    }
    Ok(PreparedDetailRead::Parsed {
        prepared,
        context_metrics,
    })
}

pub(crate) fn finish_prepared_detail(
    home: &Path,
    read: PreparedDetailRead,
) -> Result<ConversationDetailDto, String> {
    match read {
        PreparedDetailRead::Indexed {
            prepared,
            event_count,
            observed_context,
            context_metrics,
            first_use_events,
        } => assemble_indexed_detail(
            home,
            prepared,
            event_count,
            observed_context,
            context_metrics,
            first_use_events,
        ),
        PreparedDetailRead::Parsed {
            prepared,
            context_metrics,
        } => load_prepared_detail(home, prepared, context_metrics),
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

fn load_prepared_detail(
    home: &Path,
    prepared: PreparedConversationDetail,
    context_metrics: Option<context_cache::CachedContextMetrics>,
) -> Result<ConversationDetailDto, String> {
    let usage_record_count = prepared.usage_records.len() as u32;
    let source = prepared.source;
    let parsed = load_prepared_parsed(home, prepared)?;
    let observed = context_manifest::observed_from_events(&parsed.events);
    let first_use_events = context_first_use::candidates_from_events(&parsed.events);
    Ok(assemble_detail(
        home,
        source,
        DetailParts {
            revision: parsed.revision,
            session: parsed.session,
            event_count: parsed.events.len() as u32,
            usage_record_count,
            agent_relations: parsed.agent_relations,
            cursor_behavior: parsed.cursor_behavior,
            observed,
            cached: context_metrics,
            first_use_events: &first_use_events,
        },
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

fn assemble_indexed_detail(
    home: &Path,
    prepared: PreparedConversationDetail,
    event_count: u32,
    observed_context: Vec<ConversationContextItem>,
    context_metrics: Option<context_cache::CachedContextMetrics>,
    first_use_events: Vec<context_first_use::Candidate>,
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
    Ok(assemble_detail(
        home,
        source,
        DetailParts {
            revision,
            session,
            event_count,
            usage_record_count: usage_records.len() as u32,
            agent_relations,
            cursor_behavior,
            observed: observed_context,
            cached: context_metrics,
            first_use_events: &first_use_events,
        },
    ))
}

struct DetailParts<'a> {
    revision: String,
    session: ConversationSessionRow,
    event_count: u32,
    usage_record_count: u32,
    agent_relations: ConversationAgentRelations,
    cursor_behavior: Option<CursorSessionDetailDto>,
    observed: Vec<ConversationContextItem>,
    cached: Option<context_cache::CachedContextMetrics>,
    first_use_events: &'a [context_first_use::Candidate],
}

fn assemble_detail(home: &Path, source: Source, parts: DetailParts<'_>) -> ConversationDetailDto {
    let mut dto = ConversationDetailDto {
        revision: parts.revision,
        session: parts.session,
        event_count: parts.event_count,
        usage_record_count: parts.usage_record_count,
        agent_relations: parts.agent_relations,
        cursor_behavior: parts.cursor_behavior,
        context_manifest: None,
    };
    dto.context_manifest =
        context_manifest::for_session(home, source, &dto.session, parts.observed, parts.cached)
            .map(|mut manifest| {
                manifest.first_uses =
                    context_first_use::collect(parts.first_use_events, &manifest.items);
                manifest
            });
    dto
}

fn stored_revisions_match(
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

fn load_exact_cursor_session(
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

fn cursor_behavior_dto(
    home: &Path,
    stats: Option<&CursorSessionRecord>,
) -> Option<CursorSessionDetailDto> {
    stats.map(|record| crate::cursor_session_detail::detail_from_record(home, record))
}

fn cursor_missing_transcript_events(session: &ConversationSessionRow) -> Vec<ConversationEvent> {
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

fn cursor_metadata_revision(
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

fn parse_conversation_files_with_revision(
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
