use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use base64::prelude::*;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde_json::Value;

use crate::domain::{
    ConversationAgentCapabilityStatus as AgentCapabilityStatus, ConversationAgentLink,
    ConversationAgentLinkStatus as AgentLinkStatus, ConversationAgentRelations,
    ConversationAttachmentKind as AttachmentKind, ConversationAttachmentStatus as AttachmentStatus,
    ConversationEvent, ConversationMatchField, ConversationSessionRow, CursorSessionRecord, Source,
    UsageRecord,
};
use crate::ingest;

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
mod toolbox;

use merge::{
    extract_agent_metadata, merge_indexed_files, merge_parsed_conversations, summarize_for_index,
    IndexedAgentMetadata, IndexedFile,
};
use toolbox::{
    AttachmentCandidate, FileIndexCursor, ParsedConversation, CAPABILITY_EVENTS, CAPABILITY_USAGE,
    EXPERIMENTAL,
};

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
    backfill_event_index_step_skipping, catalog_roots, conversation_source_roots,
    event_index_ready, finish_prepared_detail, load_prepared_parsed, prepare_detail,
    prepare_detail_read,
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
pub(crate) const THUMBNAIL_MAX_WIDTH: u32 = 320;
pub(crate) const THUMBNAIL_MAX_HEIGHT: u32 = 240;
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

/// 按行号流式读取源文件的一行。行号从 0 计；只保留当前行，不把整份文件读进内存。
pub(crate) fn read_source_line(path: &Path, line_index: u32) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|error| format!("读取原始文件失败：{error}"))?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut index = 0u32;
    loop {
        buffer.clear();
        let bytes = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| format!("读取原始文件失败：{error}"))?;
        if bytes == 0 {
            return Err(format!("原始文件中未找到第 {} 行", line_index + 1));
        }
        if index == line_index {
            let line = std::str::from_utf8(&buffer)
                .map_err(|error| format!("读取原始文件失败：{error}"))?;
            let line = line.strip_suffix('\n').unwrap_or(line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            return Ok(line.to_string());
        }
        index += 1;
    }
}

pub(super) fn read_source_payload(
    source: Source,
    path: &Path,
    sequence: u32,
) -> Result<Value, String> {
    if source == Source::Gemini {
        let file = fs::File::open(path).map_err(|error| format!("读取原始文件失败：{error}"))?;
        let root: Value = serde_json::from_reader(BufReader::new(file))
            .map_err(|error| format!("附件所在事件 JSON 无效：{error}"))?;
        return root
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.get(sequence as usize))
            .cloned()
            .ok_or_else(|| "原始文件中未找到附件所在事件".to_string());
    }
    let raw = read_source_line(path, sequence)?;
    let value: Value =
        serde_json::from_str(&raw).map_err(|error| format!("附件所在事件 JSON 无效：{error}"))?;
    Ok(value.get("payload").cloned().unwrap_or(value))
}

pub(super) fn ensure_attachment_path_allowed(
    candidate: &AttachmentCandidate,
    project: &str,
) -> Result<(), String> {
    if candidate.attachment.status != AttachmentStatus::Available {
        return Ok(());
    }
    let path = candidate
        .resolved_path
        .as_ref()
        .ok_or_else(|| "附件路径不可用".to_string())?;
    let canonical_path =
        fs::canonicalize(path).map_err(|_| "原附件已不存在，无法加载图片".to_string())?;
    let project_path = Path::new(project);
    if !project_path.is_absolute() {
        return Err("附件路径不在会话项目允许的目录内".to_string());
    }
    let project_root =
        fs::canonicalize(project_path).map_err(|_| "会话项目目录不可用".to_string())?;
    if project_root.parent().is_some() && canonical_path.starts_with(project_root) {
        Ok(())
    } else {
        Err("附件路径不在会话项目允许的目录内".to_string())
    }
}

pub(crate) fn attachment_data_url(candidate: &AttachmentCandidate) -> Result<String, String> {
    if candidate.attachment.status == AttachmentStatus::Embedded {
        if candidate.source.starts_with("data:image/") {
            return Ok(candidate.source.clone());
        }
        return Err("内嵌附件不是可预览的图片".to_string());
    }
    let bytes = attachment_bytes(candidate)?;
    Ok(format!(
        "data:{};base64,{}",
        candidate.attachment.media_type,
        BASE64_STANDARD.encode(bytes)
    ))
}

pub(crate) fn attachment_thumbnail_data_url(
    candidate: &AttachmentCandidate,
) -> Result<String, String> {
    let bytes = attachment_bytes(candidate)?;
    let image =
        image::load_from_memory(&bytes).map_err(|error| format!("图片格式无效：{error}"))?;
    let thumbnail = image.thumbnail(
        image.width().min(THUMBNAIL_MAX_WIDTH),
        image.height().min(THUMBNAIL_MAX_HEIGHT),
    );
    let mut encoded = Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|error| format!("生成图片缩略图失败：{error}"))?;
    Ok(format!(
        "data:image/png;base64,{}",
        BASE64_STANDARD.encode(encoded.into_inner())
    ))
}

pub(crate) fn attachment_bytes(candidate: &AttachmentCandidate) -> Result<Vec<u8>, String> {
    match candidate.attachment.status {
        AttachmentStatus::Embedded => {
            let (metadata, encoded) = candidate
                .source
                .split_once(',')
                .ok_or_else(|| "内嵌图片数据无效".to_string())?;
            if !metadata.starts_with("data:image/") || !metadata.ends_with(";base64") {
                return Err("内嵌附件不是可预览的图片".to_string());
            }
            BASE64_STANDARD
                .decode(encoded)
                .map_err(|error| format!("内嵌图片数据无效：{error}"))
        }
        AttachmentStatus::Missing => Err("原附件已不存在，无法加载图片".to_string()),
        AttachmentStatus::Unsupported => Err("远程附件不在应用内加载".to_string()),
        AttachmentStatus::Available => {
            let path = candidate
                .resolved_path
                .as_ref()
                .ok_or_else(|| "附件路径不可用".to_string())?;
            fs::read(path).map_err(|error| format!("读取原附件失败：{error}"))
        }
    }
}

pub(crate) fn upsert_session(
    conn: &Connection,
    session: &ConversationSessionRow,
    is_top_level: bool,
    agent_metadata: &IndexedAgentMetadata,
    source_file_mtime_ns: i64,
    source_file_size: i64,
    source_revision: &str,
) -> Result<(), String> {
    let capabilities = serde_json::to_string(&session.capabilities).map_err(|e| e.to_string())?;
    let agent_metadata = serde_json::to_string(agent_metadata).map_err(|e| e.to_string())?;
    conn.execute(
        r#"
        INSERT INTO conversation_sessions(
            source, session_id, title, project, model, started_at, ended_at,
            source_file, capabilities_json, support_status, file_available,
            source_file_mtime_ns, source_file_size, adapter_version, source_revision,
            is_top_level, agent_metadata_json
        ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
        ON CONFLICT(source, session_id) DO UPDATE SET
            title = excluded.title,
            project = excluded.project,
            model = excluded.model,
            started_at = excluded.started_at,
            ended_at = excluded.ended_at,
            source_file = excluded.source_file,
            capabilities_json = excluded.capabilities_json,
            support_status = excluded.support_status,
            file_available = excluded.file_available,
            source_file_mtime_ns = excluded.source_file_mtime_ns,
            source_file_size = excluded.source_file_size,
            adapter_version = excluded.adapter_version,
            source_revision = excluded.source_revision,
            is_top_level = excluded.is_top_level,
            agent_metadata_json = excluded.agent_metadata_json
        "#,
        params![
            session.source,
            session.session_id,
            session.title,
            session.project,
            session.model,
            session.started_at,
            session.ended_at,
            session.source_file,
            capabilities,
            session.support_status,
            session.file_available,
            source_file_mtime_ns,
            source_file_size,
            CONVERSATION_ADAPTER_VERSION,
            source_revision,
            is_top_level,
            agent_metadata,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn failed_session_paths(
    conn: &Connection,
    source: Source,
    issues: &[ConversationIndexIssue],
) -> Result<BTreeMap<String, BTreeSet<PathBuf>>, String> {
    let mut paths_by_session = BTreeMap::new();
    let mut statement = conn
        .prepare(
            r#"
            SELECT session_id FROM conversation_session_files
            WHERE source = ?1 AND source_file = ?2
            "#,
        )
        .map_err(|error| error.to_string())?;
    for issue in issues {
        let session_ids = statement
            .query_map(params![source.as_str(), issue.path], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        for session_id in session_ids {
            paths_by_session
                .entry(session_id)
                .or_insert_with(BTreeSet::new)
                .insert(PathBuf::from(&issue.path));
        }
    }
    Ok(paths_by_session)
}

pub(crate) fn update_session_files(
    conn: &Connection,
    source: Source,
    session_id: &str,
    paths: &[PathBuf],
    replace: bool,
) -> Result<(), String> {
    if replace {
        conn.execute(
            "DELETE FROM conversation_session_files WHERE source = ?1 AND session_id = ?2",
            params![source.as_str(), session_id],
        )
        .map_err(|error| error.to_string())?;
    }
    for path in paths {
        let metadata =
            fs::metadata(path).map_err(|error| format!("读取文件元数据失败：{error}"))?;
        let source_revision = (conversation_adapter(source)?.revision)(path)?;
        conn.execute(
            r#"
            INSERT INTO conversation_session_files(
                source, session_id, source_file, source_file_mtime_ns, source_file_size,
                adapter_version, source_revision
            ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(source, session_id, source_file) DO UPDATE SET
                source_file_mtime_ns = excluded.source_file_mtime_ns,
                source_file_size = excluded.source_file_size,
                adapter_version = excluded.adapter_version,
                source_revision = excluded.source_revision
            "#,
            params![
                source.as_str(),
                session_id,
                path.to_string_lossy().to_string(),
                modified_nanos(&metadata),
                metadata.len() as i64,
                CONVERSATION_ADAPTER_VERSION,
                source_revision,
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
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

pub(crate) fn load_usage_records(
    conn: &Connection,
    source: Source,
    session_id: &str,
) -> Result<Vec<UsageRecord>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT occurred_at, model, provider, project, session_id, source_file,
                   input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                   reasoning_tokens, total_tokens, native_cost
            FROM usage_records
            WHERE source = ?1 AND session_id = ?2
            ORDER BY occurred_at ASC, source_file ASC, rowid ASC
            "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params![source.as_str(), session_id], |row| {
            Ok(UsageRecord {
                occurred_at: row.get(0)?,
                source,
                model: row.get(1)?,
                provider: row.get(2)?,
                project: row.get(3)?,
                session_id: row.get(4)?,
                source_file: row.get(5)?,
                input_tokens: row.get(6)?,
                output_tokens: row.get(7)?,
                cache_read_tokens: row.get(8)?,
                cache_creation_tokens: row.get(9)?,
                reasoning_tokens: row.get(10)?,
                total_tokens: row.get(11)?,
                native_cost: row.get(12)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let mut records = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut seen = BTreeSet::new();
    records.retain(|record| seen.insert(usage_record_identity(record)));
    Ok(records)
}

pub(crate) fn usage_record_identity(record: &UsageRecord) -> String {
    serde_json::json!({
        "occurred_at": record.occurred_at,
        "source": record.source,
        "model": record.model,
        "provider": record.provider,
        "project": record.project,
        "session_id": record.session_id,
        "input_tokens": record.input_tokens,
        "output_tokens": record.output_tokens,
        "cache_read_tokens": record.cache_read_tokens,
        "cache_creation_tokens": record.cache_creation_tokens,
        "reasoning_tokens": record.reasoning_tokens,
        "total_tokens": record.total_tokens,
        "native_cost_bits": record.native_cost.map(f64::to_bits),
    })
    .to_string()
}

pub(crate) fn load_session(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Option<ConversationSessionRow>, String> {
    let mut session = conn
        .query_row(
            r#"
            SELECT source, session_id, title, project, model, started_at, ended_at,
                   source_file, capabilities_json, support_status, file_available,
                   0, -1
            FROM conversation_sessions WHERE source = ?1 AND session_id = ?2
            "#,
            params![source, session_id],
            row_from_sql,
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(session) = &mut session {
        let paths = load_session_files(conn, source, session_id)?;
        if !paths.is_empty() {
            session.source_files = paths
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect();
        }
        fill_empty_cursor_hash_model(conn, session)?;
    }
    Ok(session)
}

pub(crate) fn load_session_files(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Vec<PathBuf>, String> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT source_file FROM conversation_session_files
            WHERE source = ?1 AND session_id = ?2
            ORDER BY source_file ASC
            "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![source, session_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .map(|result| result.map(PathBuf::from))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

pub(super) fn load_trusted_session_files(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
) -> Result<(Source, ConversationSessionRow, Vec<PathBuf>), String> {
    let source = Source::parse(source).filter(|source| CONVERSATION_SOURCES.contains(source));
    let Some(source) = source else {
        return Err("该来源尚未支持对话详情".to_string());
    };
    let session = load_session(conn, source.as_str(), session_id)?
        .ok_or_else(|| "未找到该对话记录".to_string())?;
    let roots = conversation_source_roots(home, source);
    let representative = PathBuf::from(&session.source_file);
    if !representative.exists() {
        return Err("原始文件已不存在，无法读取对话详情".to_string());
    }
    ensure_trusted_path(&representative, &roots)?;
    let mut paths = load_session_files(conn, source.as_str(), session_id)?;
    if !paths.is_empty() && !paths.iter().any(|path| path == &representative) {
        return Err("会话索引的代表文件与来源清单不一致".to_string());
    }
    if paths.is_empty() {
        paths.push(representative);
    }
    for path in &paths {
        if !path.exists() {
            return Err("原始文件已不存在，无法读取对话详情".to_string());
        }
        ensure_trusted_path(path, &roots)?;
    }
    Ok((source, session, paths))
}

pub(super) fn ensure_matching_session(
    parsed: &ParsedConversation,
    session: &ConversationSessionRow,
) -> Result<(), String> {
    if parsed.session.session_id == session.session_id {
        Ok(())
    } else {
        Err("原始文件中的会话 ID 与索引不一致".to_string())
    }
}

pub(crate) fn tombstone_missing_sessions(
    conn: &Connection,
    source: Source,
    seen_session_ids: &BTreeSet<String>,
) -> Result<(), String> {
    let cached = conn
        .prepare("SELECT session_id FROM conversation_sessions WHERE source = ?1")
        .map_err(|e| e.to_string())?
        .query_map(params![source.as_str()], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for session_id in cached {
        if seen_session_ids.contains(&session_id) {
            continue;
        }
        mark_session_unavailable(conn, source, &session_id)?;
        event_index::clear_session_events(conn, source, &session_id)?;
    }
    Ok(())
}

pub(crate) fn mark_session_unavailable(
    conn: &Connection,
    source: Source,
    session_id: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE conversation_sessions SET file_available = 0 WHERE source = ?1 AND session_id = ?2",
        params![source.as_str(), session_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationSessionRow> {
    let capabilities_json: String = row.get(8)?;
    let source_file: String = row.get(7)?;
    let match_rank: i64 = row.get(12)?;
    Ok(ConversationSessionRow {
        source: row.get(0)?,
        session_id: row.get(1)?,
        title: row.get(2)?,
        project: row.get(3)?,
        model: row.get(4)?,
        started_at: row.get(5)?,
        ended_at: row.get(6)?,
        source_file: source_file.clone(),
        source_files: vec![source_file],
        capabilities: serde_json::from_str(&capabilities_json).unwrap_or_default(),
        support_status: row.get(9)?,
        file_available: row.get(10)?,
        event_index_ready: row.get::<_, i64>(11)? != 0,
        match_field: match match_rank {
            0 => Some(ConversationMatchField::Title),
            1 => Some(ConversationMatchField::Body),
            _ => None,
        },
        ..Default::default()
    })
}

pub(crate) fn load_cached_fingerprints(
    conn: &Connection,
    source: Source,
    path: &Path,
) -> Result<Vec<CachedConversationFingerprint>, String> {
    let mut stmt = conn
        .prepare(
            r#"
        SELECT DISTINCT session_id, source_file_mtime_ns, source_file_size, adapter_version,
                        source_revision, indexed_byte_offset, indexed_line, has_live_generation
        FROM (
            SELECT files.session_id, files.source_file_mtime_ns, files.source_file_size,
                   files.adapter_version, files.source_revision,
                   files.indexed_byte_offset, files.indexed_line,
                   CASE WHEN sessions.event_index_generation IS NULL THEN 0 ELSE 1 END
                     AS has_live_generation
            FROM conversation_session_files AS files
            JOIN conversation_sessions AS sessions
              ON sessions.source = files.source AND sessions.session_id = files.session_id
            WHERE files.source = ?1 AND files.source_file = ?2 AND sessions.file_available = 1
            UNION ALL
            SELECT session_id, source_file_mtime_ns, source_file_size, adapter_version,
                   source_revision, 0, 0, 0
            FROM conversation_sessions
            WHERE source = ?1 AND source_file = ?2 AND file_available = 1
        )
        "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(
            params![source.as_str(), path.to_string_lossy().to_string()],
            |row| {
                Ok(CachedConversationFingerprint {
                    session_id: row.get(0)?,
                    source_file_mtime_ns: row.get(1)?,
                    source_file_size: row.get(2)?,
                    adapter_version: row.get(3)?,
                    source_revision: row.get(4)?,
                    indexed_byte_offset: row.get(5)?,
                    indexed_line: row.get(6)?,
                    has_live_generation: row.get::<_, i64>(7)? != 0,
                })
            },
        )
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub(crate) fn modified_nanos(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
        .unwrap_or(0)
}

pub(crate) fn metadata_revision(metadata: &fs::Metadata) -> String {
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{modified_ns}:{}", metadata.len())
}

pub(crate) fn ensure_trusted_path(path: &Path, roots: &[PathBuf]) -> Result<(), String> {
    let canonical_path =
        fs::canonicalize(path).map_err(|error| format!("无法验证原始文件路径：{error}"))?;
    ensure_canonical_path_in_roots(&canonical_path, roots)
}

pub(crate) fn ensure_canonical_path_in_roots(
    canonical_path: &Path,
    roots: &[PathBuf],
) -> Result<(), String> {
    for root in roots {
        let Ok(canonical_root) = fs::canonicalize(root) else {
            continue;
        };
        if canonical_path.starts_with(canonical_root) {
            return Ok(());
        }
    }
    Err("原始文件不在该来源允许的扫描目录内".to_string())
}

pub(crate) fn session_source_paths(
    session: &ConversationSessionRow,
) -> Result<Vec<PathBuf>, String> {
    let representative = PathBuf::from(&session.source_file);
    let paths = if session.source_files.is_empty() {
        vec![representative.clone()]
    } else {
        session.source_files.iter().map(PathBuf::from).collect()
    };
    if !paths.iter().any(|path| path == &representative) {
        return Err("会话索引的代表文件与来源清单不一致".to_string());
    }
    Ok(paths)
}

pub(crate) fn trusted_paths_for_session(
    home: &Path,
    source: Source,
    session: &ConversationSessionRow,
) -> Result<Vec<PathBuf>, String> {
    let roots = conversation_source_roots(home, source);
    let representative = PathBuf::from(&session.source_file);
    if !representative.exists() {
        return Err("原文件已删除，详情不可读取".to_string());
    }
    ensure_trusted_path(&representative, &roots)?;
    let paths = session_source_paths(session)?;
    for path in &paths {
        if !path.exists() {
            return Err("原文件已删除，详情不可读取".to_string());
        }
        ensure_trusted_path(path, &roots)?;
    }
    Ok(paths)
}

pub(crate) fn files_revision(source: Source, paths: &[PathBuf]) -> Result<String, String> {
    let adapter = conversation_adapter(source)?;
    let revisions = paths
        .iter()
        .map(|path| {
            (adapter.revision)(path).map(|revision| (path.to_string_lossy().to_string(), revision))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let [(_, revision)] = revisions.as_slice() {
        return Ok(revision.clone());
    }
    serde_json::to_string(&revisions).map_err(|error| error.to_string())
}

pub(crate) fn detail_files_revision(
    source: Source,
    paths: &[PathBuf],
    roots: &[PathBuf],
) -> Result<Option<String>, String> {
    let mut revisions = Vec::with_capacity(paths.len());
    for path in paths {
        let Some(revision) = detail_file_revision(source, path, roots)? else {
            return Ok(None);
        };
        revisions.push((path.to_string_lossy().to_string(), revision));
    }
    if let [(_, revision)] = revisions.as_slice() {
        return Ok(Some(revision.clone()));
    }
    serde_json::to_string(&revisions)
        .map(Some)
        .map_err(|error| error.to_string())
}

pub(crate) fn detail_file_revision(
    source: Source,
    path: &Path,
    roots: &[PathBuf],
) -> Result<Option<String>, String> {
    let revision = conversation_adapter(source)?.revision;
    checked_detail_file_revision(
        roots,
        || fs::canonicalize(path),
        |canonical_path| revision(canonical_path).map_err(std::io::Error::other),
    )
}

pub(crate) fn checked_detail_file_revision(
    roots: &[PathBuf],
    canonicalize_file: impl FnOnce() -> std::io::Result<PathBuf>,
    read_revision: impl FnOnce(&Path) -> std::io::Result<String>,
) -> Result<Option<String>, String> {
    let canonical_path = match canonicalize_file() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法验证原始文件路径：{error}")),
    };
    ensure_canonical_path_in_roots(&canonical_path, roots)?;
    match read_revision(&canonical_path) {
        Ok(revision) => Ok(Some(revision)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("读取原始文件元数据失败：{error}")),
    }
}

pub(super) fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
