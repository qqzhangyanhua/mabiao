use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, params_from_iter, Connection};

use crate::domain::{
    ConversationAgentCapabilityStatus as AgentCapabilityStatus, ConversationAgentLink,
    ConversationAgentLinkStatus as AgentLinkStatus, ConversationAgentRelations, ConversationEvent,
    ConversationSessionRow, CursorSessionRecord, Source, UsageRecord,
};
use crate::ingest;

pub(crate) mod attachments;
mod catalog;
mod catalog_search;
mod claude;
mod codex;
mod copilot;
mod cursor;
mod discover;
mod droid;
mod dsh;
mod event_index;
mod event_page;
mod export;
mod gemini;
mod grok;
mod incremental;
mod kimi;
mod line_direct;
mod merge;
mod omp;
mod opencode;
mod persist;
mod pi;
mod qwen;
mod read;
mod session_store;
mod toolbox;
pub(crate) mod trusted_path;

use merge::{
    extract_agent_metadata, merge_indexed_files, merge_parsed_conversations, summarize_for_index,
    IndexedAgentMetadata, IndexedFile,
};
use session_store::{
    failed_session_paths, load_cached_fingerprints, load_session, load_session_files,
    load_usage_records, mark_session_unavailable, tombstone_missing_sessions, update_session_files,
    upsert_session,
};
use toolbox::{
    FileIndexCursor, ParsedConversation, CAPABILITY_EVENTS, CAPABILITY_USAGE, EXPERIMENTAL,
};
use trusted_path::modified_nanos;

pub use catalog::{
    catalog_tool_names, indexed_events, sessions_page, sessions_page_with_prices,
    usage_records_page,
};
pub(crate) use catalog::{conversation_source_paths, finish_catalog_rows, sql_placeholders};
pub(crate) use discover::{
    detail_claude, detail_gemini, detail_omp, detail_pi, diagnostic_detail, diagnostic_index,
    discover_droid, discover_dsh, discover_extension, discover_gemini, discover_jsonl,
    discover_opencode, index_claude, index_gemini, index_omp, index_pi, regular_source_revision,
    single_detail,
};
pub(crate) use persist::{
    apply_incremental, persist_session_file_cursors, prepare_incremental, record_full_parse,
    write_session_file_events, IncrementalPrepare, PendingIncremental,
};
#[cfg(test)]
pub(crate) use read::read_consistent_snapshot;
pub use read::{
    backfill_event_index, backfill_event_index_step, detail_state, event_index_progress,
    load_attachment, load_attachment_thumbnail, load_detail, load_event_content,
    load_parsed_detail, parse_session_events, rebuild_events_from_line,
};
pub(crate) use read::{
    backfill_event_index_step_skipping, catalog_roots, event_index_ready, finish_prepared_detail,
    load_prepared_parsed, prepare_detail, prepare_detail_read,
};

pub use export::build_export;
#[cfg(test)]
pub(crate) use export::parsed_export;
pub(crate) use export::{export_default_name, write_conversation_export};

pub(super) const DEFAULT_PAGE_SIZE: u32 = 20;
pub(super) const MAX_PAGE_SIZE: u32 = 200;
pub(crate) const CONVERSATION_SOURCES: &[Source] = &[
    Source::Codex,
    Source::Claude,
    Source::CursorAgent,
    Source::Dsh,
    Source::Factory,
    Source::Kimi,
    Source::Grok,
    Source::Pi,
    Source::Omp,
    Source::Gemini,
    Source::Opencode,
    Source::Qwen,
    Source::Copilot,
];
pub(crate) const DETAIL_READ_ATTEMPTS: usize = 3;
pub(crate) const CONVERSATION_ADAPTER_VERSION: i64 = 14;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationIndexIssue {
    pub path: String,
    pub message: String,
    pub event_type: Option<String>,
    pub line: Option<u64>,
}

pub(crate) struct CachedConversationFingerprint {
    session_id: String,
    source_file_mtime_ns: i64,
    source_file_size: i64,
    adapter_version: i64,
    source_revision: String,
    indexed_byte_offset: i64,
    indexed_line: i64,
    has_live_generation: bool,
}

pub(crate) struct SessionFileCursorWrite<'a> {
    path: &'a Path,
    cursor: FileIndexCursor,
    max_sequence: Option<u32>,
    mtime_ns: i64,
    size: i64,
    source_revision: &'a str,
}

pub(crate) struct ConversationIndexBatch {
    conversations: Vec<ParsedConversation>,
    diagnostics: Vec<ConversationIndexIssue>,
}

pub(crate) type ConversationDiscoverFn = fn(&[PathBuf]) -> Result<Vec<PathBuf>, String>;
pub(crate) type ConversationIndexFn =
    fn(&Path) -> Result<ConversationIndexBatch, ConversationIndexIssue>;
pub(crate) type ConversationIndexSuffixFn =
    fn(&Path, u64, u32, &str) -> Result<ParsedConversation, ConversationIndexIssue>;
pub(crate) type ConversationDetailFn = fn(&Path, &str, bool) -> Result<ParsedConversation, String>;
pub(crate) type ConversationRevisionFn = fn(&Path) -> Result<String, String>;

pub(crate) struct ConversationAdapter {
    source: Source,
    discover: ConversationDiscoverFn,
    index: ConversationIndexFn,
    /// 只解析后缀、不写库。未填则刷新时一律全量 index。
    index_suffix: Option<ConversationIndexSuffixFn>,
    detail: ConversationDetailFn,
    revision: ConversationRevisionFn,
    raw_extension: Option<&'static str>,
    reuse_unchanged_index: bool,
}

pub(crate) const CONVERSATION_ADAPTERS: &[ConversationAdapter] = &[
    ConversationAdapter {
        source: Source::Codex,
        discover: discover_jsonl,
        index: codex::index,
        index_suffix: Some(codex::index_suffix),
        detail: codex::detail,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Claude,
        discover: discover_jsonl,
        index: index_claude,
        index_suffix: None,
        detail: detail_claude,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::CursorAgent,
        discover: cursor::discover,
        index: cursor::index,
        index_suffix: None,
        detail: cursor::detail,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Dsh,
        discover: discover_dsh,
        index: dsh::index,
        index_suffix: None,
        detail: dsh::detail,
        revision: regular_source_revision,
        raw_extension: None,
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Factory,
        discover: discover_droid,
        index: droid::index,
        index_suffix: None,
        detail: droid::detail,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Kimi,
        discover: kimi::discover,
        index: kimi::index,
        index_suffix: None,
        detail: kimi::detail,
        revision: kimi::source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Grok,
        discover: grok::discover,
        index: grok::index,
        index_suffix: None,
        detail: grok::detail,
        revision: grok::source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Pi,
        discover: discover_jsonl,
        index: index_pi,
        index_suffix: None,
        detail: detail_pi,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Omp,
        discover: discover_jsonl,
        index: index_omp,
        index_suffix: None,
        detail: detail_omp,
        revision: omp::source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Gemini,
        discover: discover_gemini,
        index: index_gemini,
        index_suffix: None,
        detail: detail_gemini,
        revision: regular_source_revision,
        raw_extension: Some("json"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Opencode,
        discover: discover_opencode,
        index: opencode::index,
        index_suffix: None,
        detail: opencode::detail,
        revision: opencode::source_revision,
        raw_extension: None,
        reuse_unchanged_index: false,
    },
    ConversationAdapter {
        source: Source::Qwen,
        discover: qwen::discover,
        index: qwen::index,
        index_suffix: None,
        detail: qwen::detail,
        revision: regular_source_revision,
        raw_extension: Some("json"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Copilot,
        discover: copilot::discover,
        index: copilot::index,
        index_suffix: None,
        detail: copilot::detail,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
];

pub(crate) fn conversation_adapter(source: Source) -> Result<&'static ConversationAdapter, String> {
    CONVERSATION_ADAPTERS
        .iter()
        .find(|adapter| adapter.source == source)
        .ok_or_else(|| "该来源尚未支持对话详情".to_string())
}

pub(super) fn raw_export_extension(source: Source) -> Result<Option<&'static str>, String> {
    Ok(conversation_adapter(source)?.raw_extension)
}

pub(crate) struct PreparedConversationDetail {
    source: Source,
    session: ConversationSessionRow,
    usage_records: Vec<UsageRecord>,
    agent_relations: ConversationAgentRelations,
    cursor_session_stats: Option<CursorSessionRecord>,
}

pub(crate) enum PreparedDetailRead {
    Indexed {
        prepared: PreparedConversationDetail,
        event_count: u32,
    },
    Parsed {
        prepared: PreparedConversationDetail,
    },
}

pub fn load_events(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    anchor: crate::domain::ConversationEventAnchor,
    limit: u32,
) -> Result<crate::domain::ConversationEventPage, String> {
    event_page::load_events(conn, home, source, session_id, anchor, limit)
}

pub(crate) fn prepare_events_read(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    anchor: &crate::domain::ConversationEventAnchor,
    limit: u32,
) -> Result<event_page::PreparedEventsRead, String> {
    event_page::prepare_events_read(conn, home, source, session_id, anchor, limit)
}

pub(crate) fn finish_prepared_events(
    home: &Path,
    read: event_page::PreparedEventsRead,
    anchor: &crate::domain::ConversationEventAnchor,
    limit: u32,
) -> Result<crate::domain::ConversationEventPage, String> {
    event_page::finish_prepared_events(home, read, anchor, limit)
}

pub(crate) use incremental::{
    plan_conversation_file_index, ConversationFileFingerprint, ConversationFileIndexPlan,
};

pub fn refresh_codex(
    conn: &Connection,
    home: &Path,
) -> Result<Vec<ConversationIndexIssue>, String> {
    let roots = ingest::source_scan_dirs(home, Source::Codex);
    refresh_source_in_roots(conn, Source::Codex, &roots)
}

/// 本机 bench 用：只跑 Codex 对话整文件 index，返回事件数。
pub fn codex_index_for_bench(path: &Path) -> Result<usize, String> {
    match codex::index(path) {
        Ok(batch) => Ok(batch
            .conversations
            .iter()
            .map(|conversation| conversation.events.len())
            .sum()),
        Err(issue) => Err(issue.message),
    }
}

/// 本机 bench 用：只跑 Codex 对话后缀 index，返回新事件数。
pub fn codex_index_suffix_for_bench(
    path: &Path,
    byte_offset: u64,
    start_line: u32,
    expected_session_id: &str,
) -> Result<usize, String> {
    match codex::index_suffix(path, byte_offset, start_line, expected_session_id) {
        Ok(parsed) => Ok(parsed.events.len()),
        Err(issue) => Err(issue.message),
    }
}

pub(crate) fn refresh_source_in_roots(
    conn: &Connection,
    source: Source,
    roots: &[PathBuf],
) -> Result<Vec<ConversationIndexIssue>, String> {
    let adapter = conversation_adapter(source)?;
    let mut issues = Vec::new();
    let mut blocking_issues = Vec::new();
    // 存摘要而不是 `ParsedConversation`：扫描期间只有当前文件的事件活着，
    // 而不是整个来源的全部事件。
    let mut grouped: BTreeMap<String, Vec<IndexedFile>> = BTreeMap::new();
    let mut unchanged_paths: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut event_generations: BTreeMap<String, i64> = BTreeMap::new();
    let mut incremental_paths: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut pending_incrementals: Vec<PendingIncremental> = Vec::new();
    let mut file_cursors: BTreeMap<(String, String), FileIndexCursor> = BTreeMap::new();
    for path in conversation_source_paths(source, roots)? {
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                let issue = ConversationIndexIssue {
                    path: path.to_string_lossy().to_string(),
                    message: format!("读取文件元数据失败：{error}"),
                    event_type: None,
                    line: None,
                };
                blocking_issues.push(issue.clone());
                issues.push(issue);
                continue;
            }
        };
        let mtime_ns = modified_nanos(&metadata);
        let size = metadata.len() as i64;
        let source_revision = match (adapter.revision)(&path) {
            Ok(revision) => revision,
            Err(message) => {
                let issue = ConversationIndexIssue {
                    path: path.to_string_lossy().to_string(),
                    message,
                    event_type: None,
                    line: None,
                };
                blocking_issues.push(issue.clone());
                issues.push(issue);
                continue;
            }
        };
        let cached = if adapter.reuse_unchanged_index {
            load_cached_fingerprints(conn, source, &path)?
        } else {
            Vec::new()
        };
        if !cached.is_empty()
            && cached.iter().all(|cached| {
                cached.source_file_mtime_ns == mtime_ns
                    && cached.source_file_size == size
                    && cached.adapter_version != 0
                    && cached.source_revision == source_revision
            })
        {
            for cached in cached {
                unchanged_paths
                    .entry(cached.session_id)
                    .or_default()
                    .push(path.clone());
            }
            continue;
        }
        let cached_row = cached
            .iter()
            .find(|row| row.indexed_byte_offset > 0)
            .or(cached.first());
        let fingerprint = cached_row.map(|row| ConversationFileFingerprint {
            mtime_ns: row.source_file_mtime_ns,
            size: row.source_file_size,
            revision: row.source_revision.clone(),
            indexed_byte_offset: row.indexed_byte_offset,
            has_live_generation: row.has_live_generation,
        });
        if plan_conversation_file_index(
            fingerprint.as_ref(),
            mtime_ns,
            size,
            &source_revision,
            adapter.index_suffix.is_some(),
        ) == ConversationFileIndexPlan::Incremental
        {
            if let (Some(index_suffix), Some(row)) = (adapter.index_suffix, cached_row) {
                match prepare_incremental(conn, source, index_suffix, &path, row) {
                    Ok(IncrementalPrepare::Ready(parsed)) => {
                        incremental_paths
                            .entry(row.session_id.clone())
                            .or_default()
                            .push(path.clone());
                        pending_incrementals.push(PendingIncremental {
                            path: path.clone(),
                            session_id: row.session_id.clone(),
                            parsed: *parsed,
                            mtime_ns,
                            size,
                            source_revision: source_revision.clone(),
                        });
                        continue;
                    }
                    Ok(IncrementalPrepare::NeedFull) => {}
                    Err(message) => {
                        let issue = ConversationIndexIssue {
                            path: path.to_string_lossy().to_string(),
                            message,
                            event_type: None,
                            line: None,
                        };
                        blocking_issues.push(issue.clone());
                        issues.push(issue);
                        continue;
                    }
                }
            }
        }
        match (adapter.index)(&path) {
            Ok(batch) => {
                issues.extend(batch.diagnostics);
                for parsed in batch.conversations {
                    record_full_parse(
                        conn,
                        source,
                        parsed,
                        &mut event_generations,
                        &mut grouped,
                        &mut file_cursors,
                    )?;
                }
            }
            Err(issue) => {
                blocking_issues.push(issue.clone());
                issues.push(issue);
            }
        }
    }

    let failed_paths_by_session = failed_session_paths(conn, source, &blocking_issues)?;
    let mut blocked_session_ids = failed_paths_by_session
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut scanned_paths_by_session: BTreeMap<String, BTreeSet<PathBuf>> = unchanged_paths
        .iter()
        .map(|(session_id, paths)| (session_id.clone(), paths.iter().cloned().collect()))
        .collect();
    for (session_id, indexed_files) in &grouped {
        scanned_paths_by_session
            .entry(session_id.clone())
            .or_default()
            .extend(
                indexed_files
                    .iter()
                    .map(|file| PathBuf::from(&file.session.source_file)),
            );
    }
    for (session_id, paths) in &incremental_paths {
        scanned_paths_by_session
            .entry(session_id.clone())
            .or_default()
            .extend(paths.iter().cloned());
    }
    for (session_id, failed_paths) in &failed_paths_by_session {
        scanned_paths_by_session
            .entry(session_id.clone())
            .or_default()
            .extend(failed_paths.iter().cloned());
    }
    let mut incomplete_session_ids = BTreeSet::new();
    for (session_id, scanned_paths) in &scanned_paths_by_session {
        let indexed_paths = load_session_files(conn, source.as_str(), session_id)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        if !indexed_paths.is_empty() && !indexed_paths.is_subset(scanned_paths) {
            let scanned_paths = scanned_paths.iter().cloned().collect::<Vec<_>>();
            update_session_files(conn, source, session_id, &scanned_paths, false)?;
            mark_session_unavailable(conn, source, session_id)?;
            incomplete_session_ids.insert(session_id.clone());
        }
    }
    for session_id in &incomplete_session_ids {
        grouped.remove(session_id);
        unchanged_paths.remove(session_id);
    }

    for (session_id, paths) in std::mem::take(&mut unchanged_paths) {
        let indexed_paths = load_session_files(conn, source.as_str(), &session_id)?;
        let scanned = paths.iter().cloned().collect::<BTreeSet<_>>();
        let indexed = indexed_paths.into_iter().collect::<BTreeSet<_>>();
        if grouped.contains_key(&session_id) || scanned != indexed {
            for path in paths {
                match (adapter.index)(&path) {
                    Ok(batch) => {
                        issues.extend(batch.diagnostics);
                        for parsed in batch.conversations {
                            if parsed.session.session_id != session_id {
                                continue;
                            }
                            if let Some(cursor) = parsed.index_cursor {
                                file_cursors.insert(
                                    (session_id.clone(), parsed.session.source_file.clone()),
                                    cursor,
                                );
                            }
                            write_session_file_events(
                                conn,
                                source,
                                &parsed,
                                &mut event_generations,
                            )?;
                            grouped
                                .entry(session_id.clone())
                                .or_default()
                                .push(summarize_for_index(parsed));
                        }
                    }
                    Err(issue) => {
                        blocked_session_ids.insert(session_id.clone());
                        blocking_issues.push(issue.clone());
                        issues.push(issue);
                    }
                }
            }
        } else {
            unchanged_paths.insert(session_id, scanned.into_iter().collect());
        }
    }

    for pending in pending_incrementals {
        if blocked_session_ids.contains(&pending.session_id)
            || grouped.contains_key(&pending.session_id)
            || incomplete_session_ids.contains(&pending.session_id)
        {
            continue;
        }
        let path = pending.path.to_string_lossy().to_string();
        if let Err(message) = apply_incremental(conn, source, pending) {
            let issue = ConversationIndexIssue {
                path,
                message,
                event_type: None,
                line: None,
            };
            blocking_issues.push(issue.clone());
            issues.push(issue);
        }
    }

    let seen_session_ids = unchanged_paths
        .keys()
        .chain(grouped.keys())
        .chain(incremental_paths.keys())
        .chain(incomplete_session_ids.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    let associated_failed_paths = failed_paths_by_session
        .values()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>();
    let has_unmapped_failures = blocking_issues
        .iter()
        .any(|issue| !associated_failed_paths.contains(&PathBuf::from(&issue.path)));
    for (session_id, indexed_files) in grouped {
        if blocked_session_ids.contains(&session_id) {
            continue;
        }
        let source_files = indexed_files
            .iter()
            .map(|file| PathBuf::from(&file.session.source_file))
            .collect::<Vec<_>>();
        let (merged_session, is_top_level, agent_metadata) = merge_indexed_files(indexed_files);
        let representative_metadata = fs::metadata(&merged_session.source_file)
            .map_err(|error| format!("读取文件元数据失败：{error}"))?;
        let representative_revision = (adapter.revision)(Path::new(&merged_session.source_file))?;
        upsert_session(
            conn,
            &merged_session,
            is_top_level,
            &agent_metadata,
            modified_nanos(&representative_metadata),
            representative_metadata.len() as i64,
            &representative_revision,
        )?;
        update_session_files(
            conn,
            source,
            &session_id,
            &source_files,
            blocking_issues.is_empty(),
        )?;
        if let Some(&generation) = event_generations.get(&session_id) {
            let publish = !has_unmapped_failures
                || event_index::has_live_generation(conn, source, &session_id)?;
            if publish {
                event_index::finalize_session_events(conn, source, &session_id, generation)?;
                persist_session_file_cursors(
                    conn,
                    source,
                    &session_id,
                    &source_files,
                    &file_cursors,
                )?;
            }
        }
    }
    if blocking_issues.is_empty() {
        tombstone_missing_sessions(conn, source, &seen_session_ids)?;
    }
    if source == Source::CursorAgent {
        sync_cursor_usage_only_sessions(conn)?;
        sync_cursor_hash_models(conn)?;
    }
    Ok(issues)
}

pub(crate) fn parse_conversation_file(
    source: Source,
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    (conversation_adapter(source)?.detail)(path, session_id, include_deferred_content)
}

pub(super) fn parse_conversation_files(
    source: Source,
    paths: &[PathBuf],
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let parsed = paths
        .iter()
        .map(|path| parse_conversation_file(source, path, session_id, include_deferred_content))
        .collect::<Result<Vec<_>, _>>()?;
    if parsed.is_empty() {
        return Err("对话没有可读取的原始文件".to_string());
    }
    Ok(merge_parsed_conversations(parsed))
}

pub(crate) fn load_agent_relations(
    conn: &Connection,
    source: Source,
    current_session_id: &str,
    current_events: &[ConversationEvent],
) -> Result<ConversationAgentRelations, String> {
    let mut catalog = load_agent_catalog(conn, source)?;
    if !current_events.is_empty() {
        catalog
            .entry(current_session_id.to_string())
            .and_modify(|(_, metadata)| *metadata = extract_agent_metadata(current_events));
    }

    let mut parent_claims = BTreeMap::<String, BTreeSet<String>>::new();
    for (session_id, (_, metadata)) in &catalog {
        for parent_id in &metadata.parent_session_ids {
            parent_claims
                .entry(session_id.clone())
                .or_default()
                .insert(parent_id.clone());
        }
        for attempt in &metadata.spawn_attempts {
            if let Some(child_id) = &attempt.child_session_id {
                parent_claims
                    .entry(child_id.clone())
                    .or_default()
                    .insert(session_id.clone());
            }
        }
    }

    let current_metadata = &catalog
        .get(current_session_id)
        .ok_or_else(|| "未找到该对话记录".to_string())?
        .1;
    let mut child_launches = BTreeMap::<String, Option<String>>::new();
    for attempt in &current_metadata.spawn_attempts {
        if let Some(child_id) = &attempt.child_session_id {
            child_launches
                .entry(child_id.clone())
                .or_insert_with(|| Some(attempt.launch_event_id.clone()));
        }
    }
    for (child_id, parents) in &parent_claims {
        if parents.contains(current_session_id) {
            child_launches.entry(child_id.clone()).or_insert(None);
        }
    }

    let mut children = child_launches
        .into_iter()
        .map(|(child_id, launch_event_id)| {
            let status = agent_link_status(current_session_id, &child_id, &catalog, &parent_claims);
            let session = (status == AgentLinkStatus::Linked)
                .then(|| catalog.get(&child_id).map(|(session, _)| session.clone()))
                .flatten();
            ConversationAgentLink {
                relationship_id: launch_event_id
                    .clone()
                    .unwrap_or_else(|| format!("metadata:{current_session_id}:{child_id}")),
                session_id: Some(child_id),
                launch_event_id,
                status,
                session,
            }
        })
        .collect::<Vec<_>>();
    children.extend(
        current_metadata
            .spawn_attempts
            .iter()
            .filter(|attempt| attempt.child_session_id.is_none())
            .map(|attempt| ConversationAgentLink {
                relationship_id: attempt.launch_event_id.clone(),
                session_id: None,
                launch_event_id: Some(attempt.launch_event_id.clone()),
                status: AgentLinkStatus::Unresolved,
                session: None,
            }),
    );
    children.sort_by(|left, right| left.relationship_id.cmp(&right.relationship_id));

    let parent = build_parent_link(current_session_id, &catalog, &parent_claims);
    let statuses = parent
        .iter()
        .map(|link| link.status)
        .chain(children.iter().map(|link| link.status))
        .collect::<Vec<_>>();
    let has_linked = statuses.contains(&AgentLinkStatus::Linked);
    let has_unavailable = statuses
        .iter()
        .any(|status| *status != AgentLinkStatus::Linked);
    let capability_status = match (has_linked, has_unavailable) {
        (true, true) => AgentCapabilityStatus::Partial,
        (false, true) => AgentCapabilityStatus::Unavailable,
        _ => AgentCapabilityStatus::Complete,
    };

    Ok(ConversationAgentRelations {
        capability_status,
        parent,
        children,
    })
}

pub(crate) fn load_agent_catalog(
    conn: &Connection,
    source: Source,
) -> Result<BTreeMap<String, (ConversationSessionRow, IndexedAgentMetadata)>, String> {
    let indexed = {
        let mut statement = conn
            .prepare(
                "SELECT session_id, agent_metadata_json FROM conversation_sessions WHERE source = ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![source.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        rows
    };
    let mut catalog = BTreeMap::new();
    for (session_id, metadata_json) in indexed {
        let Some(session) = load_session(conn, source.as_str(), &session_id)? else {
            continue;
        };
        let metadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        catalog.insert(session_id, (session, metadata));
    }
    Ok(catalog)
}

pub(crate) fn build_parent_link(
    child_id: &str,
    catalog: &BTreeMap<String, (ConversationSessionRow, IndexedAgentMetadata)>,
    parent_claims: &BTreeMap<String, BTreeSet<String>>,
) -> Option<ConversationAgentLink> {
    let claims = parent_claims.get(child_id)?;
    if claims.len() != 1 {
        return Some(ConversationAgentLink {
            relationship_id: format!("conflict:{child_id}"),
            session_id: None,
            launch_event_id: None,
            status: AgentLinkStatus::Conflict,
            session: None,
        });
    }
    let parent_id = claims.iter().next()?.clone();
    let launch_event_id = catalog.get(&parent_id).and_then(|(_, metadata)| {
        metadata
            .spawn_attempts
            .iter()
            .find(|attempt| attempt.child_session_id.as_deref() == Some(child_id))
            .map(|attempt| attempt.launch_event_id.clone())
    });
    let status = agent_link_status(&parent_id, child_id, catalog, parent_claims);
    let session = (status == AgentLinkStatus::Linked)
        .then(|| catalog.get(&parent_id).map(|(session, _)| session.clone()))
        .flatten();
    Some(ConversationAgentLink {
        relationship_id: launch_event_id
            .clone()
            .unwrap_or_else(|| format!("metadata:{parent_id}:{child_id}")),
        session_id: Some(parent_id),
        launch_event_id,
        status,
        session,
    })
}

pub(crate) fn agent_link_status(
    parent_id: &str,
    child_id: &str,
    catalog: &BTreeMap<String, (ConversationSessionRow, IndexedAgentMetadata)>,
    parent_claims: &BTreeMap<String, BTreeSet<String>>,
) -> AgentLinkStatus {
    if !catalog.contains_key(parent_id) || !catalog.contains_key(child_id) {
        return AgentLinkStatus::MissingSource;
    }
    if parent_claims
        .get(child_id)
        .is_some_and(|claims| claims.len() > 1)
    {
        return AgentLinkStatus::Conflict;
    }
    if parent_id == child_id || agent_path_exists(child_id, parent_id, parent_claims) {
        return AgentLinkStatus::Cycle;
    }
    AgentLinkStatus::Linked
}

pub(crate) fn agent_path_exists(
    from: &str,
    target: &str,
    parent_claims: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    let mut pending = vec![from.to_string()];
    let mut visited = BTreeSet::new();
    while let Some(parent) = pending.pop() {
        if !visited.insert(parent.clone()) {
            continue;
        }
        for (child, parents) in parent_claims {
            if parents.contains(&parent) {
                if child == target {
                    return true;
                }
                pending.push(child.clone());
            }
        }
    }
    false
}

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
