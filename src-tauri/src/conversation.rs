use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use base64::prelude::*;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde_json::Value;

use crate::domain::{
    ConversationAgentCapabilityStatus as AgentCapabilityStatus, ConversationAgentLink,
    ConversationAgentLinkStatus as AgentLinkStatus, ConversationAgentRelations,
    ConversationAttachment, ConversationAttachmentContentDto,
    ConversationAttachmentKind as AttachmentKind, ConversationAttachmentStatus as AttachmentStatus,
    ConversationDetailDto, ConversationDetailStateDto, ConversationEvent,
    ConversationEventActor as EventActor, ConversationEventCapabilityStatus as EventStatus,
    ConversationEventContentDto, ConversationEventContentStatus as ContentStatus,
    ConversationEventKind as EventKind, ConversationExportDto, ConversationExportFormat,
    ConversationIndexProgressDto, ConversationMessage, ConversationPage, ConversationParsedDetail,
    ConversationQuery, ConversationSessionRow, ConversationUsagePage, CursorSessionDetailDto,
    CursorSessionRecord, PriceTable, Source, UsageRecord,
};
use crate::ingest;
use crate::query;

mod claude;
mod copilot;
mod cursor;
mod droid;
mod dsh;
mod event_index;
mod event_page;
mod gemini;
mod grok;
mod incremental;
mod kimi;
mod opencode;
mod pi;
mod qwen;

const DEFAULT_PAGE_SIZE: u32 = 20;
const MAX_PAGE_SIZE: u32 = 200;
const TITLE_MAX_CHARS: usize = 80;
const CAPABILITY_MESSAGES: &str = "messages";
const CAPABILITY_EVENTS: &str = "events";
const CAPABILITY_USAGE: &str = "usage";
pub(crate) const CONVERSATION_SOURCES: &[Source] = &[
    Source::Codex,
    Source::Claude,
    Source::CursorAgent,
    Source::Dsh,
    Source::Factory,
    Source::Kimi,
    Source::Grok,
    Source::Pi,
    Source::Gemini,
    Source::Opencode,
    Source::Qwen,
    Source::Copilot,
];
const EXPERIMENTAL: &str = "experimental";
const DETAIL_READ_ATTEMPTS: usize = 3;
const LARGE_CONTENT_THRESHOLD: usize = 4_096;
const CONTENT_PREVIEW_CHARS: usize = 2_000;
const THUMBNAIL_MAX_WIDTH: u32 = 320;
const THUMBNAIL_MAX_HEIGHT: u32 = 240;
pub(crate) const CONVERSATION_ADAPTER_VERSION: i64 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationIndexIssue {
    pub path: String,
    pub message: String,
    pub event_type: Option<String>,
    pub line: Option<u64>,
}

struct CachedConversationFingerprint {
    session_id: String,
    source_file_mtime_ns: i64,
    source_file_size: i64,
    adapter_version: i64,
    source_revision: String,
    indexed_byte_offset: i64,
    indexed_line: i64,
    has_live_generation: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FileIndexCursor {
    pub byte_offset: i64,
    pub line: i64,
}

struct SessionFileCursorWrite<'a> {
    path: &'a Path,
    cursor: FileIndexCursor,
    max_sequence: Option<u32>,
    mtime_ns: i64,
    size: i64,
    source_revision: &'a str,
}

pub(crate) struct ParsedConversation {
    pub(crate) session: ConversationSessionRow,
    messages: Vec<ConversationMessage>,
    pub(crate) events: Vec<ConversationEvent>,
    is_top_level: bool,
    index_cursor: Option<FileIndexCursor>,
}

struct ConversationIndexBatch {
    conversations: Vec<ParsedConversation>,
    diagnostics: Vec<ConversationIndexIssue>,
}

/// 一个源文件在目录索引里的全部有效信息。见 `IndexedAgentEvent` 的说明。
struct IndexedFile {
    session: ConversationSessionRow,
    is_top_level: bool,
    agent_events: Vec<IndexedAgentEvent>,
}

type ConversationDiscoverFn = fn(&[PathBuf]) -> Result<Vec<PathBuf>, String>;
type ConversationIndexFn = fn(&Path) -> Result<ConversationIndexBatch, ConversationIndexIssue>;
type ConversationDetailFn = fn(&Path, &str, bool) -> Result<ParsedConversation, String>;
type ConversationRevisionFn = fn(&Path) -> Result<String, String>;

struct ConversationAdapter {
    source: Source,
    discover: ConversationDiscoverFn,
    index: ConversationIndexFn,
    detail: ConversationDetailFn,
    revision: ConversationRevisionFn,
    raw_extension: Option<&'static str>,
    reuse_unchanged_index: bool,
}

const CONVERSATION_ADAPTERS: &[ConversationAdapter] = &[
    ConversationAdapter {
        source: Source::Codex,
        discover: discover_jsonl,
        index: index_codex,
        detail: detail_codex,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Claude,
        discover: discover_jsonl,
        index: index_claude,
        detail: detail_claude,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::CursorAgent,
        discover: cursor::discover,
        index: cursor::index,
        detail: cursor::detail,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Dsh,
        discover: discover_dsh,
        index: dsh::index,
        detail: dsh::detail,
        revision: regular_source_revision,
        raw_extension: None,
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Factory,
        discover: discover_droid,
        index: droid::index,
        detail: droid::detail,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Kimi,
        discover: kimi::discover,
        index: kimi::index,
        detail: kimi::detail,
        revision: kimi::source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Grok,
        discover: grok::discover,
        index: grok::index,
        detail: grok::detail,
        revision: grok::source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Pi,
        discover: discover_jsonl,
        index: index_pi,
        detail: detail_pi,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Gemini,
        discover: discover_gemini,
        index: index_gemini,
        detail: detail_gemini,
        revision: regular_source_revision,
        raw_extension: Some("json"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Opencode,
        discover: discover_opencode,
        index: opencode::index,
        detail: opencode::detail,
        revision: opencode::source_revision,
        raw_extension: None,
        reuse_unchanged_index: false,
    },
    ConversationAdapter {
        source: Source::Qwen,
        discover: qwen::discover,
        index: qwen::index,
        detail: qwen::detail,
        revision: regular_source_revision,
        raw_extension: Some("json"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Copilot,
        discover: copilot::discover,
        index: copilot::index,
        detail: copilot::detail,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
];

fn conversation_adapter(source: Source) -> Result<&'static ConversationAdapter, String> {
    CONVERSATION_ADAPTERS
        .iter()
        .find(|adapter| adapter.source == source)
        .ok_or_else(|| "该来源尚未支持对话详情".to_string())
}

fn discover_extension(roots: &[PathBuf], extension: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        paths.extend(ingest::walk_files(root, extension)?);
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn discover_jsonl(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    discover_extension(roots, "jsonl")
}

fn discover_dsh(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(discover_extension(roots, "zstd")?
        .into_iter()
        .filter(|path| {
            path.file_name().and_then(|name| name.to_str()) == Some("session.jsonl.zstd")
        })
        .collect())
}

fn discover_droid(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(discover_jsonl(roots)?
        .into_iter()
        .filter(|path| {
            let Some(session_id) = path.file_stem().and_then(|name| name.to_str()) else {
                return false;
            };
            path.with_file_name(format!("{session_id}.settings.json"))
                .is_file()
        })
        .collect())
}

fn discover_gemini(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(discover_extension(roots, "json")?
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("session-"))
        })
        .collect())
}

fn discover_opencode(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(roots
        .iter()
        .filter(|path| path.is_file())
        .cloned()
        .collect())
}

fn regular_source_revision(path: &Path) -> Result<String, String> {
    fs::metadata(path)
        .map(|metadata| metadata_revision(&metadata))
        .map_err(|error| format!("读取原始文件元数据失败：{error}"))
}

fn single_index(
    path: &Path,
    parse: fn(&Path, bool) -> Result<ParsedConversation, String>,
) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    parse(path, false)
        .map(|conversation| ConversationIndexBatch {
            conversations: vec![conversation],
            diagnostics: Vec::new(),
        })
        .map_err(|message| ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message,
            event_type: None,
            line: None,
        })
}

fn single_detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
    parse: fn(&Path, bool) -> Result<ParsedConversation, String>,
) -> Result<ParsedConversation, String> {
    let parsed = parse(path, include_deferred_content)?;
    if parsed.session.session_id == session_id {
        Ok(parsed)
    } else {
        Err("原始文件中的会话 ID 与索引不一致".to_string())
    }
}

type DiagnosticParseFn =
    fn(&Path, bool) -> Result<(ParsedConversation, Vec<ConversationIndexIssue>), String>;

fn diagnostic_index(
    path: &Path,
    event_type: &str,
    parse: DiagnosticParseFn,
) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let (conversation, diagnostics) =
        parse(path, false).map_err(|message| ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message,
            event_type: Some(event_type.to_string()),
            line: None,
        })?;
    Ok(ConversationIndexBatch {
        conversations: vec![conversation],
        diagnostics,
    })
}

fn diagnostic_detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
    parse: DiagnosticParseFn,
) -> Result<ParsedConversation, String> {
    let (parsed, _) = parse(path, include_deferred_content)?;
    if parsed.session.session_id == session_id {
        Ok(parsed)
    } else {
        Err("原始文件中的会话 ID 与索引不一致".to_string())
    }
}

fn index_codex(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    parse_codex_file_mode(path, false, false).map(|conversation| ConversationIndexBatch {
        conversations: vec![conversation],
        diagnostics: Vec::new(),
    })
}

fn detail_codex(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, parse_codex_file)
}

fn index_claude(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    single_index(path, claude::parse)
}

fn detail_claude(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, claude::parse)
}

fn index_pi(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    single_index(path, pi::parse)
}

fn detail_pi(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, pi::parse)
}

fn index_gemini(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    single_index(path, gemini::parse)
}

fn detail_gemini(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, gemini::parse)
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct IndexedAgentMetadata {
    parent_session_ids: Vec<String>,
    spawn_attempts: Vec<IndexedSpawnAttempt>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct IndexedSpawnAttempt {
    launch_event_id: String,
    child_session_id: Option<String>,
}

struct PendingMessageDelta {
    sequence: u32,
    occurred_at: String,
    role: String,
    text: String,
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
        let fingerprint = cached
            .iter()
            .find(|row| row.indexed_byte_offset > 0)
            .or(cached.first())
            .map(|row| ConversationFileFingerprint {
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
            source == Source::Codex,
        ) == ConversationFileIndexPlan::Incremental
        {
            if let Some(row) = cached
                .iter()
                .find(|row| row.indexed_byte_offset > 0)
                .or(cached.first())
            {
                match prepare_incremental_codex(conn, &path, row) {
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
                    Ok(IncrementalPrepare::NeedFull) => {
                        match parse_codex_file_mode(&path, true, false) {
                            Ok(parsed) => {
                                record_full_parse(
                                    conn,
                                    source,
                                    parsed,
                                    &mut event_generations,
                                    &mut grouped,
                                    &mut file_cursors,
                                )?;
                                continue;
                            }
                            Err(issue) => {
                                blocking_issues.push(issue.clone());
                                issues.push(issue);
                                continue;
                            }
                        }
                    }
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
        if let Err(message) = apply_incremental_codex(conn, pending) {
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
    }
    Ok(issues)
}

enum IncrementalPrepare {
    Ready(Box<ParsedConversation>),
    NeedFull,
}

struct PendingIncremental {
    path: PathBuf,
    session_id: String,
    parsed: ParsedConversation,
    mtime_ns: i64,
    size: i64,
    source_revision: String,
}

fn record_full_parse(
    conn: &Connection,
    source: Source,
    parsed: ParsedConversation,
    event_generations: &mut BTreeMap<String, i64>,
    grouped: &mut BTreeMap<String, Vec<IndexedFile>>,
    file_cursors: &mut BTreeMap<(String, String), FileIndexCursor>,
) -> Result<(), String> {
    if let Some(cursor) = parsed.index_cursor {
        file_cursors.insert(
            (
                parsed.session.session_id.clone(),
                parsed.session.source_file.clone(),
            ),
            cursor,
        );
    }
    write_session_file_events(conn, source, &parsed, event_generations)?;
    grouped
        .entry(parsed.session.session_id.clone())
        .or_default()
        .push(summarize_for_index(parsed));
    Ok(())
}

fn prepare_incremental_codex(
    conn: &Connection,
    path: &Path,
    cached: &CachedConversationFingerprint,
) -> Result<IncrementalPrepare, String> {
    let parsed = match parse_codex_suffix(
        path,
        cached.indexed_byte_offset as u64,
        cached.indexed_line as u32,
        &cached.session_id,
    ) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(IncrementalPrepare::NeedFull),
    };
    if parsed.session.session_id != cached.session_id {
        return Ok(IncrementalPrepare::NeedFull);
    }
    if !event_index::has_live_generation(conn, Source::Codex, &cached.session_id)? {
        return Ok(IncrementalPrepare::NeedFull);
    }
    if event_index::live_index_would_rewind(
        conn,
        Source::Codex,
        &cached.session_id,
        &parsed.events,
    )? {
        return Ok(IncrementalPrepare::NeedFull);
    }
    Ok(IncrementalPrepare::Ready(Box::new(parsed)))
}

fn apply_incremental_codex(conn: &Connection, pending: PendingIncremental) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let result = apply_incremental_codex_in_tx(&tx, pending);
    match result {
        Ok(()) => tx.commit().map_err(|error| error.to_string()),
        Err(error) => {
            let _ = tx.rollback();
            Err(error)
        }
    }
}

fn apply_incremental_codex_in_tx(
    conn: &Connection,
    pending: PendingIncremental,
) -> Result<(), String> {
    let PendingIncremental {
        path,
        session_id,
        parsed,
        mtime_ns,
        size,
        source_revision,
    } = pending;
    let max_sequence =
        event_index::append_live_events(conn, Source::Codex, &session_id, &parsed.events)?;
    let cursor = parsed.index_cursor.unwrap_or(FileIndexCursor {
        byte_offset: size,
        line: 0,
    });
    persist_file_cursor(
        conn,
        Source::Codex,
        &session_id,
        &SessionFileCursorWrite {
            path: &path,
            cursor,
            max_sequence: Some(max_sequence),
            mtime_ns,
            size,
            source_revision: &source_revision,
        },
    )?;
    touch_session_after_append(conn, &session_id, &parsed, mtime_ns, size, &source_revision)
}

fn touch_session_after_append(
    conn: &Connection,
    session_id: &str,
    parsed: &ParsedConversation,
    mtime_ns: i64,
    size: i64,
    source_revision: &str,
) -> Result<(), String> {
    let ended_at = parsed.session.ended_at.as_str();
    conn.execute(
        r#"
        UPDATE conversation_sessions
        SET ended_at = CASE
                WHEN ?3 != '' AND (ended_at = '' OR ended_at < ?3) THEN ?3
                ELSE ended_at
            END,
            model = CASE WHEN ?4 != '' THEN ?4 ELSE model END,
            source_file_mtime_ns = ?5,
            source_file_size = ?6,
            source_revision = ?7,
            file_available = 1
        WHERE source = ?1 AND session_id = ?2
        "#,
        params![
            Source::Codex.as_str(),
            session_id,
            ended_at,
            parsed.session.model,
            mtime_ns,
            size,
            source_revision,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn persist_session_file_cursors(
    conn: &Connection,
    source: Source,
    session_id: &str,
    paths: &[PathBuf],
    file_cursors: &BTreeMap<(String, String), FileIndexCursor>,
) -> Result<(), String> {
    let max_sequence = conn
        .query_row(
            r#"
            SELECT MAX(sequence) FROM conversation_events
            WHERE source = ?1 AND session_id = ?2
              AND index_generation = (
                  SELECT event_index_generation FROM conversation_sessions
                  WHERE source = ?1 AND session_id = ?2
              )
            "#,
            params![source.as_str(), session_id],
            |row| row.get::<_, Option<u32>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    for path in paths {
        let metadata =
            fs::metadata(path).map_err(|error| format!("读取文件元数据失败：{error}"))?;
        let revision = (conversation_adapter(source)?.revision)(path)?;
        let cursor = file_cursors
            .get(&(session_id.to_string(), path.to_string_lossy().to_string()))
            .copied()
            .unwrap_or(FileIndexCursor {
                byte_offset: metadata.len() as i64,
                line: 0,
            });
        persist_file_cursor(
            conn,
            source,
            session_id,
            &SessionFileCursorWrite {
                path,
                cursor,
                max_sequence,
                mtime_ns: modified_nanos(&metadata),
                size: metadata.len() as i64,
                source_revision: &revision,
            },
        )?;
    }
    Ok(())
}

fn persist_file_cursor(
    conn: &Connection,
    source: Source,
    session_id: &str,
    write: &SessionFileCursorWrite<'_>,
) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO conversation_session_files(
            source, session_id, source_file, source_file_mtime_ns, source_file_size,
            adapter_version, source_revision, indexed_byte_offset, indexed_line, max_sequence
        ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ON CONFLICT(source, session_id, source_file) DO UPDATE SET
            source_file_mtime_ns = excluded.source_file_mtime_ns,
            source_file_size = excluded.source_file_size,
            adapter_version = excluded.adapter_version,
            source_revision = excluded.source_revision,
            indexed_byte_offset = excluded.indexed_byte_offset,
            indexed_line = excluded.indexed_line,
            max_sequence = excluded.max_sequence
        "#,
        params![
            source.as_str(),
            session_id,
            write.path.to_string_lossy().to_string(),
            write.mtime_ns,
            write.size,
            CONVERSATION_ADAPTER_VERSION,
            write.source_revision,
            write.cursor.byte_offset,
            write.cursor.line,
            write.max_sequence,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn write_session_file_events(
    conn: &Connection,
    source: Source,
    parsed: &ParsedConversation,
    generations: &mut BTreeMap<String, i64>,
) -> Result<(), String> {
    event_index::write_file_events(conn, source, parsed, generations)
}

fn conversation_source_paths(source: Source, roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    (conversation_adapter(source)?.discover)(roots)
}

fn sql_placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn catalog_filter_sql(query: &ConversationQuery) -> (String, Vec<rusqlite::types::Value>) {
    let mut clauses = vec!["sessions.is_top_level = 1".to_string()];
    let mut params = Vec::new();
    let search = query.search.as_deref().unwrap_or("").trim();
    if !search.is_empty() {
        let pattern = format!("%{}%", escape_like(search));
        clauses.push(
            "(sessions.title LIKE ? ESCAPE '\\' OR sessions.source LIKE ? ESCAPE '\\' \
             OR sessions.project LIKE ? ESCAPE '\\' OR sessions.model LIKE ? ESCAPE '\\' \
             OR sessions.session_id LIKE ? ESCAPE '\\' OR sessions.started_at LIKE ? ESCAPE '\\' \
             OR sessions.ended_at LIKE ? ESCAPE '\\')"
                .to_string(),
        );
        for _ in 0..7 {
            params.push(rusqlite::types::Value::Text(pattern.clone()));
        }
    }
    if !query.sources.is_empty() {
        clauses.push(format!(
            "sessions.source IN ({})",
            sql_placeholders(query.sources.len())
        ));
        for source in &query.sources {
            params.push(rusqlite::types::Value::Text(source.clone()));
        }
    }
    if !query.projects.is_empty() {
        clauses.push(format!(
            "sessions.project IN ({})",
            sql_placeholders(query.projects.len())
        ));
        for project in &query.projects {
            params.push(rusqlite::types::Value::Text(project.clone()));
        }
    }
    (clauses.join(" AND "), params)
}

pub fn sessions_page(
    conn: &Connection,
    query: &ConversationQuery,
) -> Result<ConversationPage, String> {
    sessions_page_with_prices(conn, query, &PriceTable::default())
}

pub fn indexed_events(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Vec<ConversationEvent>, String> {
    event_index::indexed_events(conn, source, session_id)
}

pub fn usage_records_page(
    conn: &Connection,
    source: &str,
    session_id: &str,
    page: u32,
    page_size: u32,
) -> Result<ConversationUsagePage, String> {
    let source = Source::parse(source).filter(|source| CONVERSATION_SOURCES.contains(source));
    let Some(source) = source else {
        return Err("该来源尚未支持对话详情".to_string());
    };
    let records = load_usage_records(conn, source, session_id)?;
    let total = records.len() as u32;
    let page = page.max(1);
    let page_size = page_size.clamp(1, MAX_PAGE_SIZE);
    let start = ((page - 1) * page_size) as usize;
    let rows = if start >= records.len() {
        Vec::new()
    } else {
        let end = (start + page_size as usize).min(records.len());
        records[start..end].to_vec()
    };
    Ok(ConversationUsagePage { rows, total })
}

pub fn sessions_page_with_prices(
    conn: &Connection,
    query: &ConversationQuery,
    prices: &PriceTable,
) -> Result<ConversationPage, String> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let offset = i64::from((page - 1) * page_size);
    let (predicate, mut params) = catalog_filter_sql(query);

    let total = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM conversation_sessions AS sessions WHERE {predicate}"),
            params_from_iter(params.iter()),
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())? as u32;

    params.push(rusqlite::types::Value::Integer(i64::from(page_size)));
    params.push(rusqlite::types::Value::Integer(offset));
    let sql = format!(
        r#"
        SELECT sessions.source, sessions.session_id, sessions.title, sessions.project, sessions.model,
               COALESCE(NULLIF(sessions.started_at, ''), cursor_times.first_seen_at, '') AS started_at,
               COALESCE(NULLIF(sessions.ended_at, ''), cursor_times.last_seen_at, cursor_times.first_seen_at, '') AS ended_at,
               sessions.source_file, sessions.capabilities_json, sessions.support_status, sessions.file_available
        FROM conversation_sessions AS sessions
        LEFT JOIN cursor_sessions AS cursor_times
          ON sessions.source = 'cursor_agent' AND sessions.session_id = cursor_times.session_id
        WHERE {predicate}
        ORDER BY COALESCE(
            NULLIF(sessions.ended_at, ''),
            NULLIF(sessions.started_at, ''),
            cursor_times.last_seen_at,
            cursor_times.first_seen_at,
            ''
        ) DESC, sessions.source ASC, sessions.session_id ASC
        LIMIT ? OFFSET ?
        "#
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(params_from_iter(params.iter()), row_from_sql)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for row in &mut rows {
        let paths = load_session_files(conn, &row.source, &row.session_id)?;
        if !paths.is_empty() {
            row.source_files = paths
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect();
        }
    }
    hydrate_catalog_usage(conn, prices, &mut rows)?;

    Ok(ConversationPage { rows, total })
}

fn hydrate_catalog_usage(
    conn: &Connection,
    prices: &PriceTable,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let keys = rows
        .iter()
        .map(|row| (row.source.clone(), row.session_id.clone()))
        .collect::<Vec<_>>();
    let totals = query::usage_rollups_for_sessions(conn, prices, &keys)?;
    for row in rows {
        let Some(usage) = totals.get(&(row.source.clone(), row.session_id.clone())) else {
            continue;
        };
        row.total_tokens = usage.total_tokens;
        row.cost = usage.cost;
        row.unpriced = usage.unpriced;
    }
    Ok(())
}

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

fn parsed_detail_to_dto(
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
    conn.query_row(
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
        |row| {
            Ok(ConversationIndexProgressDto {
                indexed: row.get::<_, i64>(1)? as u32,
                total: row.get::<_, i64>(0)? as u32,
            })
        },
    )
    .map_err(|error| error.to_string())
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
    loop {
        let Some((source, session_id)) = next_unready_session(conn, &skipped)? else {
            break;
        };
        match reindex_session_events(conn, home, &source, &session_id) {
            Ok(()) => completed += 1,
            Err(_) => {
                skipped.insert((source, session_id));
            }
        }
    }
    Ok(completed)
}

fn next_unready_session(
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

fn reindex_session_events(
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

fn conversation_source_roots(home: &Path, source: Source) -> Vec<PathBuf> {
    if source == Source::CursorAgent {
        vec![home.join(".cursor/projects")]
    } else {
        ingest::source_scan_dirs(home, source)
    }
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

pub fn load_event_content(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    event_id: &str,
) -> Result<ConversationEventContentDto, String> {
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

fn resolve_attachment(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    attachment_id: &str,
) -> Result<AttachmentCandidate, String> {
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

pub fn build_export(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    format: ConversationExportFormat,
) -> Result<ConversationExportDto, String> {
    let (source, session, paths) = load_trusted_session_files(conn, home, source, session_id)?;
    let parsed = parse_conversation_files(source, &paths, session_id, true)?;
    ensure_matching_session(&parsed, &session)?;
    let base_name = safe_export_name(&parsed.session.title, &session.session_id);
    match format {
        ConversationExportFormat::Json if conversation_adapter(source)?.raw_extension.is_none() => {
            Err("该来源不支持导出单一原始对话文件".to_string())
        }
        ConversationExportFormat::Json if paths.len() > 1 => {
            Err("该会话包含多个原始文件，无法导出为单一原始 JSONL".to_string())
        }
        ConversationExportFormat::Json if source == Source::Qwen => Ok(ConversationExportDto {
            default_name: format!("{base_name}.json"),
            content: qwen::export_session_records(&paths[0], session_id)?,
        }),
        ConversationExportFormat::Json => Ok(ConversationExportDto {
            default_name: format!(
                "{base_name}.{}",
                conversation_adapter(source)?
                    .raw_extension
                    .unwrap_or("jsonl")
            ),
            content: fs::read(&paths[0]).map_err(|error| format!("读取原始文件失败：{error}"))?,
        }),
        ConversationExportFormat::Markdown => Ok(ConversationExportDto {
            default_name: format!("{base_name}.md"),
            content: render_markdown_export(&parsed).into_bytes(),
        }),
    }
}

fn safe_export_name(title: &str, session_id: &str) -> String {
    let source = if title.trim().is_empty() {
        session_id
    } else {
        title.trim()
    };
    let name: String = source
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => character,
        })
        .take(100)
        .collect();
    if name.is_empty() {
        "conversation".to_string()
    } else {
        name
    }
}

fn render_markdown_export(parsed: &ParsedConversation) -> String {
    let session = &parsed.session;
    let mut markdown = format!(
        "# {}\n\n- 来源：{}\n- 会话 ID：`{}`\n- 项目：{}\n- 模型：{}\n- 开始：{}\n- 结束：{}\n\n",
        session.title,
        session.source,
        session.session_id,
        explicit_value(&session.project),
        explicit_value(&session.model),
        explicit_value(&session.started_at),
        explicit_value(&session.ended_at),
    );
    for event in &parsed.events {
        markdown.push_str(&format!(
            "---\n\n## {} · {}\n\n- 时间：{}\n",
            event.sequence,
            event.kind.as_str(),
            event.occurred_at.as_deref().unwrap_or("时间缺失")
        ));
        if let Some(actor) = event.actor {
            markdown.push_str(&format!("- 角色：{}\n", actor.as_str()));
        }
        if let Some(name) = &event.name {
            markdown.push_str(&format!("- 名称：`{name}`\n"));
        }
        if let Some(text) = &event.text {
            markdown.push('\n');
            markdown.push_str(text);
            markdown.push('\n');
        }
        if !event.attachments.is_empty() {
            markdown.push_str("\n### 附件\n\n");
            for attachment in &event.attachments {
                let status = match attachment.status {
                    AttachmentStatus::Available => "可用",
                    AttachmentStatus::Missing => "附件缺失",
                    AttachmentStatus::Embedded => "内嵌",
                    AttachmentStatus::Unsupported => "不支持应用内加载",
                };
                let size = attachment
                    .size_bytes
                    .map(|size| format!("{size} bytes"))
                    .unwrap_or_else(|| "大小未知".to_string());
                markdown.push_str(&format!(
                    "- **{}** · `{}` · {} · {} · {}\n",
                    attachment.name, attachment.original_path, attachment.media_type, size, status
                ));
            }
        }
        if let Some(details) = export_details(&event.details) {
            markdown.push_str("\n<details><summary>原始事件数据</summary>\n\n```json\n");
            markdown.push_str(&details);
            markdown.push_str("\n```\n\n</details>\n");
        }
    }
    markdown
}

fn explicit_value(value: &str) -> &str {
    if value.is_empty() {
        "缺失"
    } else {
        value
    }
}

fn export_details(details: &Value) -> Option<String> {
    let mut details = details.clone();
    if let Value::Object(object) = &mut details {
        object.remove("content");
        object.remove("message");
        object.remove("output");
        object.remove("result");
        if object.is_empty() {
            return None;
        }
    } else if details.is_null() {
        return None;
    }
    serde_json::to_string_pretty(&details).ok()
}

fn parse_codex_file(
    path: &Path,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    parse_codex_file_mode(path, true, include_deferred_content).map_err(|issue| issue.message)
}

fn parse_codex_file_mode(
    path: &Path,
    tolerate_incomplete_tail: bool,
    include_deferred_content: bool,
) -> Result<ParsedConversation, ConversationIndexIssue> {
    let content = fs::read_to_string(path).map_err(|error| ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message: format!("读取原始文件失败：{error}"),
        event_type: None,
        line: None,
    })?;
    parse_codex_content(
        path,
        &content,
        0,
        0,
        tolerate_incomplete_tail,
        include_deferred_content,
        None,
    )
}

fn parse_codex_suffix(
    path: &Path,
    byte_offset: u64,
    start_line: u32,
    session_id: &str,
) -> Result<ParsedConversation, ConversationIndexIssue> {
    let content = read_file_suffix(path, byte_offset)?;
    parse_codex_content(
        path,
        &content,
        byte_offset,
        start_line,
        true,
        false,
        Some(session_id.to_string()),
    )
}

fn read_file_suffix(path: &Path, byte_offset: u64) -> Result<String, ConversationIndexIssue> {
    let mut file = fs::File::open(path).map_err(|error| ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message: format!("读取原始文件失败：{error}"),
        event_type: None,
        line: None,
    })?;
    file.seek(SeekFrom::Start(byte_offset))
        .map_err(|error| ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message: format!("读取原始文件失败：{error}"),
            event_type: None,
            line: None,
        })?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|error| ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message: format!("读取原始文件失败：{error}"),
            event_type: None,
            line: None,
        })?;
    Ok(content)
}

fn next_line_index(content: &str) -> u32 {
    let newlines = content.bytes().filter(|&byte| byte == b'\n').count() as u32;
    if content.is_empty() || content.ends_with('\n') {
        newlines
    } else {
        newlines + 1
    }
}

fn parse_codex_content(
    path: &Path,
    content: &str,
    start_byte: u64,
    start_line: u32,
    tolerate_incomplete_tail: bool,
    include_deferred_content: bool,
    session_hint: Option<String>,
) -> Result<ParsedConversation, ConversationIndexIssue> {
    let mut session_id = session_hint.clone().unwrap_or_default();
    let mut title = String::new();
    let mut project = String::new();
    let mut model = String::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut response_messages = Vec::new();
    let mut event_messages = Vec::new();
    let mut events = Vec::new();
    let mut pending_delta = None;
    let last_line_index = content.lines().count().saturating_sub(1);
    let has_unterminated_tail = !content.ends_with('\n');
    let mut skipped_incomplete = false;

    for (index, raw) in content.lines().enumerate() {
        let line = start_line as usize + index;
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(raw) {
            Ok(value) => value,
            Err(error)
                if tolerate_incomplete_tail
                    && has_unterminated_tail
                    && index == last_line_index
                    && error.classify() == serde_json::error::Category::Eof =>
            {
                skipped_incomplete = true;
                break;
            }
            Err(error) => {
                return Err(ConversationIndexIssue {
                    path: path.to_string_lossy().to_string(),
                    message: format!("JSON 无效：{error}"),
                    event_type: Some("json_line".to_string()),
                    line: Some((line + 1) as u64),
                });
            }
        };
        let timestamp = text_field(&value, "timestamp");
        if !timestamp.is_empty() {
            if started_at.is_empty() {
                started_at = timestamp.clone();
            }
            ended_at = timestamp.clone();
        }
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let payload = value.get("payload").unwrap_or(&Value::Null);
        match kind {
            "session_meta" => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                session_id = first_text(payload, &["id", "session_id"]);
                project = first_text(payload, &["cwd"]);
                title = first_text(payload, &["title", "name"]);
                events.push(semantic_event(
                    line,
                    EventKind::SystemStatus,
                    &timestamp,
                    None,
                    Some("session_started".to_string()),
                    None,
                    payload.clone(),
                ));
            }
            "turn_context" => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                let next_project = first_text(payload, &["cwd"]);
                if !next_project.is_empty() {
                    project = next_project;
                }
                let next_model = first_text(payload, &["model"]);
                if !next_model.is_empty() && next_model != model {
                    events.push(semantic_event(
                        line,
                        EventKind::ModelChange,
                        &timestamp,
                        None,
                        Some(next_model.clone()),
                        None,
                        payload.clone(),
                    ));
                    model = next_model;
                }
            }
            "response_item" => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                if let Some(message) = response_message(payload, &timestamp) {
                    events.push(message_event(line, &message, payload.clone()));
                    response_messages.push(message);
                } else if let Some(event) =
                    response_semantic_event(line, &timestamp, payload, include_deferred_content)
                {
                    events.push(event);
                }
            }
            "event_msg" => {
                let event_kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
                match event_kind {
                    "agent_message_delta" => append_message_delta(
                        &mut pending_delta,
                        line,
                        &timestamp,
                        "assistant",
                        payload,
                    ),
                    "token_count" | "heartbeat" => {}
                    _ => {
                        flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                        if let Some(message) = event_message(payload, &timestamp) {
                            events.push(message_event(line, &message, payload.clone()));
                            event_messages.push(message);
                        } else {
                            events.push(event_msg_semantic_event(
                                line, &timestamp, event_kind, payload,
                            ));
                        }
                    }
                }
            }
            _ => {
                flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
                events.push(unadapted_event(line, &timestamp, kind, value.clone()));
            }
        }
    }
    flush_message_delta(&mut pending_delta, &mut event_messages, &mut events);
    populate_attachments(&mut events, &project);
    strip_message_bodies_from_details(&mut events);
    deduplicate_message_channels(&mut events);
    let source_file = path.to_string_lossy().to_string();
    assign_event_provenance(&mut events, &source_file);
    events.sort_by(compare_event_order);

    if session_id.is_empty() {
        return Err(ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message: "缺少 Codex 会话 ID".to_string(),
            event_type: Some("session_meta".to_string()),
            line: None,
        });
    }
    let messages = if response_messages.is_empty() {
        event_messages
    } else {
        response_messages
    };
    if title.is_empty() {
        title = messages
            .iter()
            .find(|message| message.role == "user")
            .map(|message| truncate_title(&strip_prompt_wrappers(&message.text)))
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| session_id.clone());
    }
    let mut capabilities = Vec::new();
    if !messages.is_empty() {
        capabilities.push(CAPABILITY_MESSAGES.to_string());
    }
    if !events.is_empty() {
        capabilities.push(CAPABILITY_EVENTS.to_string());
    }
    capabilities.push(CAPABILITY_USAGE.to_string());
    let session = ConversationSessionRow {
        source: Source::Codex.as_str().to_string(),
        session_id,
        title,
        project,
        model,
        started_at,
        ended_at,
        source_file: path.to_string_lossy().to_string(),
        source_files: vec![path.to_string_lossy().to_string()],
        capabilities,
        support_status: EXPERIMENTAL.to_string(),
        file_available: true,
        ..Default::default()
    };
    let (consumed_bytes, consumed_lines) = if skipped_incomplete {
        match content.rfind('\n') {
            Some(pos) => (
                (pos + 1) as i64,
                i64::from(next_line_index(&content[..=pos])),
            ),
            None => (0, 0),
        }
    } else {
        (content.len() as i64, i64::from(next_line_index(content)))
    };
    Ok(ParsedConversation {
        session,
        messages,
        events,
        is_top_level: true,
        index_cursor: Some(FileIndexCursor {
            byte_offset: start_byte as i64 + consumed_bytes,
            line: i64::from(start_line) + consumed_lines,
        }),
    })
}

fn tag_source_events(
    events: &mut [ConversationEvent],
    source_sequence: usize,
    native_identity: Option<&str>,
) {
    for event in events {
        event.source_sequence = source_sequence as u32;
        if let (Some(native_identity), Value::Object(details)) =
            (native_identity, &mut event.details)
        {
            details.insert(
                "native_id".to_string(),
                Value::String(native_identity.to_string()),
            );
        }
    }
}

fn assign_native_event_ids(events: &mut [ConversationEvent], source: Source, session_id: &str) {
    for event in events {
        if !event.event_id.is_empty() {
            continue;
        }
        if let Some(native_id) = optional_text(
            &event.details,
            &[
                "native_id",
                "call_id",
                "message_id",
                "prompt_id",
                "event_id",
            ],
        ) {
            event.event_id = format!(
                "{}:{session_id}:{}:{native_id}",
                source.as_str(),
                event.kind.as_str()
            );
        }
    }
}

fn assign_event_provenance(events: &mut [ConversationEvent], source_file: &str) {
    let mut occurrences = BTreeMap::<u32, u32>::new();
    for event in events {
        let occurrence = occurrences.entry(event.source_sequence).or_default();
        event.source_file = source_file.to_string();
        if event.event_id.is_empty() {
            let base_id = event_id_for(source_file, event.source_sequence);
            event.event_id = if *occurrence == 0 {
                base_id
            } else {
                format!("{base_id}:{}", *occurrence)
            };
        }
        *occurrence += 1;
        for (index, attachment) in event.attachments.iter_mut().enumerate() {
            attachment.id = format!("{}:{index}", event.event_id);
        }
    }
}

fn parse_jsonl_conversation_values(path: &Path) -> Result<Vec<(usize, Value)>, String> {
    let content = fs::read_to_string(path).map_err(|error| format!("读取原始文件失败：{error}"))?;
    content
        .lines()
        .enumerate()
        .filter(|(_, raw)| !raw.trim().is_empty())
        .map(|(index, raw)| {
            serde_json::from_str(raw.trim())
                .map(|value| (index, value))
                .map_err(|error| format!("第 {} 行 JSON 无效：{error}", index + 1))
        })
        .collect()
}

fn update_time_bounds(timestamp: &str, started_at: &mut String, ended_at: &mut String) {
    if timestamp.is_empty() {
        return;
    }
    if started_at.is_empty() || compare_timestamps(timestamp, started_at).is_lt() {
        *started_at = timestamp.to_string();
    }
    if ended_at.is_empty() || compare_timestamps(timestamp, ended_at).is_gt() {
        *ended_at = timestamp.to_string();
    }
}

fn push_projected_message(
    sequence: usize,
    timestamp: &str,
    role: &str,
    content: &Value,
    details: Value,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
) {
    let text = content_text(content);
    if text.is_empty() {
        return;
    }
    let message = ConversationMessage {
        role: role.to_string(),
        occurred_at: timestamp.to_string(),
        text,
    };
    events.push(message_event(sequence, &message, details));
    messages.push(message);
}

fn append_capability_degradation_status(
    sequence: usize,
    messages: &[ConversationMessage],
    model: &str,
    events: &mut Vec<ConversationEvent>,
) {
    let mut missing = Vec::new();
    if !messages.iter().any(|message| message.role == "user") {
        missing.push("user_message");
    }
    if model.is_empty() {
        missing.push("model");
    }
    let tool_results = events
        .iter()
        .filter(|event| event.kind == EventKind::ToolResult)
        .filter_map(|event| event.details.get("call_id").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    if events
        .iter()
        .filter(|event| event.kind == EventKind::ToolCall)
        .any(|event| {
            event
                .details
                .get("call_id")
                .and_then(Value::as_str)
                .is_none_or(|call_id| !tool_results.contains(call_id))
        })
    {
        missing.push("tool_result");
    }
    if events.iter().any(|event| {
        matches!(
            event.capability_status,
            EventStatus::MissingTimestamp | EventStatus::UnadaptedMissingTimestamp
        )
    }) {
        missing.push("timestamp");
    }
    append_declared_capability_degradation_status(sequence, &missing, events);
}

fn append_declared_capability_degradation_status(
    sequence: usize,
    missing: &[&str],
    events: &mut Vec<ConversationEvent>,
) {
    if missing.is_empty() {
        return;
    }
    let occurred_at = events
        .iter()
        .filter_map(|event| event.occurred_at.as_deref())
        .max()
        .unwrap_or("");
    events.push(semantic_event(
        sequence,
        EventKind::SystemStatus,
        occurred_at,
        None,
        Some("capability_degraded".to_string()),
        Some(missing.join(", ")),
        serde_json::json!({ "missing": missing }),
    ));
}

#[allow(clippy::too_many_arguments)]
fn finish_source_conversation(
    source: Source,
    path: &Path,
    session_id: String,
    mut title: String,
    project: String,
    model: String,
    started_at: String,
    ended_at: String,
    messages: Vec<ConversationMessage>,
    mut events: Vec<ConversationEvent>,
    is_top_level: bool,
) -> Result<ParsedConversation, String> {
    if session_id.is_empty() {
        return Err(format!("缺少 {} 会话 ID", source.application_name()));
    }
    populate_attachments(&mut events, &project);
    strip_message_bodies_from_details(&mut events);
    deduplicate_message_channels(&mut events);
    let source_file = path.to_string_lossy().to_string();
    assign_event_provenance(&mut events, &source_file);
    events.sort_by(compare_event_order);
    if title.is_empty() {
        title = messages
            .iter()
            .find(|message| message.role == "user")
            .map(|message| truncate_title(&strip_prompt_wrappers(&message.text)))
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| session_id.clone());
    }
    let mut capabilities = Vec::new();
    if !messages.is_empty() {
        capabilities.push(CAPABILITY_MESSAGES.to_string());
    }
    if !events.is_empty() {
        capabilities.push(CAPABILITY_EVENTS.to_string());
    }
    // Capabilities describe supported detail surfaces; an empty usage list is valid data, not a degraded parser.
    capabilities.push(CAPABILITY_USAGE.to_string());
    Ok(ParsedConversation {
        session: ConversationSessionRow {
            source: source.as_str().to_string(),
            session_id,
            title,
            project,
            model,
            started_at,
            ended_at,
            source_file: source_file.clone(),
            source_files: vec![source_file],
            capabilities,
            support_status: EXPERIMENTAL.to_string(),
            file_available: true,
            ..Default::default()
        },
        messages,
        events,
        is_top_level,
        index_cursor: None,
    })
}

fn normalize_tool_call_details(item: &Value) -> Value {
    let mut details = item.clone();
    if let Value::Object(object) = &mut details {
        if !object.contains_key("call_id") {
            if let Some(id) = object.get("id").cloned() {
                object.insert("call_id".to_string(), id);
            }
        }
    }
    details
}

fn normalize_tool_result_details(item: &Value) -> Value {
    let mut details = item.clone();
    if let Value::Object(object) = &mut details {
        if !object.contains_key("call_id") {
            if let Some(id) = object
                .get("tool_use_id")
                .or_else(|| object.get("toolCallId"))
                .or_else(|| object.get("id"))
                .cloned()
            {
                object.insert("call_id".to_string(), id);
            }
        }
        if !object.contains_key("agent_id") {
            if let Some(agent_id) = object.get("agentId").cloned() {
                object.insert("agent_id".to_string(), agent_id);
            }
        }
        if !object.contains_key("output") {
            if let Some(content) = object.get("content").or_else(|| object.get("result")) {
                object.insert("output".to_string(), Value::String(content_text(content)));
            }
        }
    }
    details
}

fn parse_conversation_file(
    source: Source,
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    (conversation_adapter(source)?.detail)(path, session_id, include_deferred_content)
}

fn parse_conversation_files(
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

/// 把同一会话散落在多个文件里的会话行合成一行。`rows` 必须已按 `source_file` 排序。
fn merge_session_rows(rows: &[ConversationSessionRow]) -> ConversationSessionRow {
    let mut session = rows[0].clone();
    session.started_at = rows
        .iter()
        .map(|row| row.started_at.as_str())
        .filter(|value| !value.is_empty())
        .min_by(|left, right| compare_timestamps(left, right))
        .unwrap_or("")
        .to_string();
    session.ended_at = rows
        .iter()
        .map(|row| row.ended_at.as_str())
        .filter(|value| !value.is_empty())
        .max_by(|left, right| compare_timestamps(left, right))
        .unwrap_or("")
        .to_string();
    if let Some(latest_model) = rows
        .iter()
        .filter(|row| !row.model.is_empty())
        .max_by(|left, right| compare_timestamps(&left.ended_at, &right.ended_at))
    {
        session.model = latest_model.model.clone();
    }
    session.source_files = rows.iter().map(|row| row.source_file.clone()).collect();
    let capability_set = rows
        .iter()
        .flat_map(|row| row.capabilities.iter().cloned())
        .collect::<BTreeSet<_>>();
    let mut capabilities = [CAPABILITY_MESSAGES, CAPABILITY_EVENTS, CAPABILITY_USAGE]
        .into_iter()
        .filter(|capability| capability_set.contains(*capability))
        .map(str::to_string)
        .collect::<Vec<_>>();
    capabilities.extend(capability_set.into_iter().filter(|capability| {
        !matches!(
            capability.as_str(),
            CAPABILITY_MESSAGES | CAPABILITY_EVENTS | CAPABILITY_USAGE
        )
    }));
    session.capabilities = capabilities;
    session
}

fn merge_parsed_conversations(mut parsed_files: Vec<ParsedConversation>) -> ParsedConversation {
    parsed_files.sort_by(|left, right| left.session.source_file.cmp(&right.session.source_file));
    let session = merge_session_rows(
        &parsed_files
            .iter()
            .map(|parsed| parsed.session.clone())
            .collect::<Vec<_>>(),
    );

    let mut messages = parsed_files
        .iter()
        .flat_map(|parsed| parsed.messages.iter().cloned())
        .collect::<Vec<_>>();
    messages.sort_by(|left, right| {
        compare_timestamps(&left.occurred_at, &right.occurred_at)
            .then_with(|| left.role.cmp(&right.role))
            .then_with(|| left.text.cmp(&right.text))
    });
    let mut seen_messages = BTreeSet::new();
    messages.retain(|message| {
        seen_messages.insert((
            message.occurred_at.clone(),
            message.role.clone(),
            message.text.clone(),
        ))
    });

    let is_top_level = parsed_files.iter().all(|parsed| parsed.is_top_level);
    let mut sourced_events = Vec::new();
    for parsed in parsed_files {
        let source_file = parsed.session.source_file;
        let mut occurrences = BTreeMap::<String, u32>::new();
        for event in parsed.events {
            let identity = event_identity(&event);
            let occurrence = occurrences.entry(identity.clone()).or_default();
            let dedupe_key = format!("{identity}\u{1f}{}", *occurrence);
            *occurrence += 1;
            sourced_events.push((source_file.clone(), dedupe_key, event));
        }
    }
    let mut seen_events = BTreeSet::new();
    sourced_events.retain(|(_, dedupe_key, _)| seen_events.insert(dedupe_key.clone()));
    sourced_events.sort_by(|(left_path, _, left), (right_path, _, right)| {
        compare_event_timestamps(left, right)
            .then_with(|| left_path.cmp(right_path))
            .then_with(|| left.source_sequence.cmp(&right.source_sequence))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    let events = sourced_events
        .into_iter()
        .enumerate()
        .map(|(sequence, (_, _, mut event))| {
            event.sequence = sequence as u32;
            event
        })
        .collect();

    ParsedConversation {
        session,
        messages,
        events,
        is_top_level,
        index_cursor: None,
    }
}

fn extract_agent_metadata(events: &[ConversationEvent]) -> IndexedAgentMetadata {
    let indexed = events
        .iter()
        .filter_map(index_agent_event)
        .collect::<Vec<_>>();
    fold_agent_metadata(indexed.iter())
}

fn summarize_for_index(parsed: ParsedConversation) -> IndexedFile {
    IndexedFile {
        is_top_level: parsed.is_top_level,
        agent_events: parsed.events.iter().filter_map(index_agent_event).collect(),
        session: parsed.session,
    }
}

/// 与 `merge_parsed_conversations` 的事件合并保持同一套语义：先按文件内出现次数生成去重键，
/// 跨文件保留首次出现，再按 (时间, 文件名, 文件内序号, event_id) 排序。
fn merge_indexed_files(
    mut files: Vec<IndexedFile>,
) -> (ConversationSessionRow, bool, IndexedAgentMetadata) {
    files.sort_by(|left, right| left.session.source_file.cmp(&right.session.source_file));
    let session = merge_session_rows(
        &files
            .iter()
            .map(|file| file.session.clone())
            .collect::<Vec<_>>(),
    );
    let is_top_level = files.iter().all(|file| file.is_top_level);

    let mut sourced = Vec::new();
    for file in files {
        let source_file = file.session.source_file;
        let mut occurrences = BTreeMap::<u64, u32>::new();
        for event in file.agent_events {
            let occurrence = occurrences.entry(event.identity).or_default();
            let dedupe_key = (event.identity, *occurrence);
            *occurrence += 1;
            sourced.push((source_file.clone(), dedupe_key, event));
        }
    }
    let mut seen = BTreeSet::new();
    sourced.retain(|(_, dedupe_key, _)| seen.insert(*dedupe_key));
    sourced.sort_by(|(left_path, _, left), (right_path, _, right)| {
        compare_optional_timestamps(&left.occurred_at, &right.occurred_at)
            .then_with(|| left_path.cmp(right_path))
            .then_with(|| left.source_sequence.cmp(&right.source_sequence))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });

    let agent = fold_agent_metadata(sourced.iter().map(|(_, _, event)| event));
    (session, is_top_level, agent)
}

/// 目录索引真正要写库的只有会话行和父子关系；事件正文另写入 `conversation_events`（ADR 0011）。
///
/// 但父子关系必须等一个会话的全部文件合并、去重、排序之后才算得对，所以每解析完一个文件，
/// 就把相关事件压成这个形状：保留合并所需的去重键与排序键，正文只留 `fold_agent_metadata`
/// 真正读的那几个字段。整份 `events`/`messages` 随即释放——否则扫描期间整个来源的全部
/// 事件会同时活着，一次「重建全部」就是几个 GB。
struct IndexedAgentEvent {
    /// 只用于去重比较，所以存哈希而不是 `event_identity` 的完整字符串——
    /// 一条 tool_result 的身份串可以有几十 KB，而它是要一直留到合并阶段的。
    identity: u64,
    event_id: String,
    source_sequence: u32,
    occurred_at: Option<String>,
    role: IndexedAgentRole,
}

enum IndexedAgentRole {
    SessionStarted {
        parent_session_ids: Vec<String>,
    },
    SpawnCall {
        call_id: String,
    },
    ToolResult {
        call_id: String,
        child_session_id: Option<String>,
    },
}

/// 事件是否与父子关系有关，完全由 `kind`/`name`/`details` 决定，而这三者都参与
/// `event_identity`——所以「先过滤再去重」和「先去重再过滤」结果一致。
fn index_agent_event(event: &ConversationEvent) -> Option<IndexedAgentEvent> {
    let role = if event.kind == EventKind::SystemStatus
        && event.name.as_deref() == Some("session_started")
    {
        IndexedAgentRole::SessionStarted {
            parent_session_ids: [
                event.details.get("parent_id"),
                event.details.get("parent_session_id"),
                event.details.pointer("/source/subagent/parent_id"),
                event.details.pointer("/source/subagent/parent_session_id"),
            ]
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        }
    } else {
        let call_id = event.details.get("call_id").and_then(Value::as_str)?;
        match event.kind {
            EventKind::ToolCall
                if matches!(
                    event.name.as_deref(),
                    Some("spawn_agent" | "Agent" | "Task")
                ) =>
            {
                IndexedAgentRole::SpawnCall {
                    call_id: call_id.to_string(),
                }
            }
            EventKind::ToolResult => IndexedAgentRole::ToolResult {
                call_id: call_id.to_string(),
                child_session_id: structured_agent_id(&event.details),
            },
            _ => return None,
        }
    };
    Some(IndexedAgentEvent {
        identity: {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&event_identity(event), &mut hasher);
            std::hash::Hasher::finish(&hasher)
        },
        event_id: event.event_id.clone(),
        source_sequence: event.source_sequence,
        occurred_at: event.occurred_at.clone(),
        role,
    })
}

fn fold_agent_metadata<'a>(
    events: impl Iterator<Item = &'a IndexedAgentEvent>,
) -> IndexedAgentMetadata {
    let mut parent_session_ids = BTreeSet::new();
    let mut spawn_calls = BTreeMap::new();
    let mut spawn_results = BTreeMap::new();
    for event in events {
        match &event.role {
            IndexedAgentRole::SessionStarted {
                parent_session_ids: ids,
            } => {
                parent_session_ids.extend(ids.iter().cloned());
            }
            IndexedAgentRole::SpawnCall { call_id } => {
                spawn_calls.insert(call_id.clone(), event.event_id.clone());
            }
            IndexedAgentRole::ToolResult {
                call_id,
                child_session_id,
            } => {
                spawn_results.insert(call_id.clone(), child_session_id.clone());
            }
        }
    }
    let spawn_attempts = spawn_calls
        .into_iter()
        .map(|(call_id, launch_event_id)| IndexedSpawnAttempt {
            launch_event_id,
            child_session_id: spawn_results.get(&call_id).cloned().flatten(),
        })
        .collect();
    IndexedAgentMetadata {
        parent_session_ids: parent_session_ids.into_iter().collect(),
        spawn_attempts,
    }
}

fn structured_agent_id(value: &Value) -> Option<String> {
    if let Some(agent_id) = value
        .as_object()
        .and_then(|object| object.get("agent_id"))
        .and_then(Value::as_str)
        .filter(|agent_id| !agent_id.is_empty())
    {
        return Some(agent_id.to_string());
    }
    for key in ["output", "result"] {
        let Some(candidate) = value.get(key) else {
            continue;
        };
        if let Some(agent_id) = structured_agent_id(candidate) {
            return Some(agent_id);
        }
        if let Some(text) = candidate.as_str() {
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                if let Some(agent_id) = structured_agent_id(&parsed) {
                    return Some(agent_id);
                }
            }
        }
    }
    None
}

fn event_id_for(source_file: &str, source_sequence: u32) -> String {
    format!(
        "{}:{source_sequence}",
        BASE64_URL_SAFE_NO_PAD.encode(source_file.as_bytes())
    )
}

pub(crate) fn event_identity(event: &ConversationEvent) -> String {
    let mut normalized = event.clone();
    normalized.event_id.clear();
    normalized.sequence = 0;
    normalized.source_file.clear();
    normalized.source_sequence = 0;
    for (index, attachment) in normalized.attachments.iter_mut().enumerate() {
        attachment.id = index.to_string();
    }
    serde_json::to_string(&normalized).unwrap_or_default()
}

fn compare_event_timestamps(
    left: &ConversationEvent,
    right: &ConversationEvent,
) -> std::cmp::Ordering {
    compare_optional_timestamps(&left.occurred_at, &right.occurred_at)
}

/// 缺时间的事件一律排在有时间的之后。
fn compare_optional_timestamps(
    left: &Option<String>,
    right: &Option<String>,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left_time), Some(right_time)) => compare_timestamps(left_time, right_time),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageChannel {
    Response,
    Event,
    Delta,
}

fn deduplicate_message_channels(events: &mut Vec<ConversationEvent>) {
    let mut current_actor = None;
    let mut seen: Vec<(String, MessageChannel)> = Vec::new();
    events.retain(|event| {
        if event.kind != EventKind::Message {
            return true;
        }
        let Some(actor) = event.actor.as_ref() else {
            return true;
        };
        let Some(text) = event.text.as_ref() else {
            return true;
        };
        if current_actor.as_ref() != Some(actor) {
            current_actor = Some(*actor);
            seen.clear();
        }
        let channel = match event.details.get("type").and_then(Value::as_str) {
            Some("message") => MessageChannel::Response,
            Some("user_message" | "agent_message") => MessageChannel::Event,
            _ => MessageChannel::Delta,
        };
        if seen
            .iter()
            .any(|(seen_text, seen_channel)| seen_text == text && *seen_channel != channel)
        {
            return false;
        }
        seen.push((text.clone(), channel));
        true
    });
}

fn compare_event_order(left: &ConversationEvent, right: &ConversationEvent) -> std::cmp::Ordering {
    match (&left.occurred_at, &right.occurred_at) {
        (Some(left_time), Some(right_time)) => compare_timestamps(left_time, right_time)
            .then_with(|| left.sequence.cmp(&right.sequence)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.sequence.cmp(&right.sequence),
    }
}

fn compare_timestamps(left: &str, right: &str) -> std::cmp::Ordering {
    match (
        chrono::DateTime::parse_from_rfc3339(left),
        chrono::DateTime::parse_from_rfc3339(right),
    ) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

fn append_message_delta(
    pending: &mut Option<PendingMessageDelta>,
    sequence: usize,
    occurred_at: &str,
    role: &str,
    payload: &Value,
) {
    let delta = first_text(payload, &["delta", "message", "text"]);
    if delta.is_empty() {
        return;
    }
    match pending {
        Some(current) if current.role == role => current.text.push_str(&delta),
        Some(_) => {}
        None => {
            *pending = Some(PendingMessageDelta {
                sequence: sequence as u32,
                occurred_at: occurred_at.to_string(),
                role: role.to_string(),
                text: delta,
            });
        }
    }
}

fn flush_message_delta(
    pending: &mut Option<PendingMessageDelta>,
    messages: &mut Vec<ConversationMessage>,
    events: &mut Vec<ConversationEvent>,
) {
    let Some(delta) = pending.take() else {
        return;
    };
    let Some(message) = message(&delta.role, &delta.occurred_at, &Value::String(delta.text)) else {
        return;
    };
    events.push(message_event(
        delta.sequence as usize,
        &message,
        Value::Null,
    ));
    messages.push(message);
}

fn message_event(
    sequence: usize,
    message: &ConversationMessage,
    details: Value,
) -> ConversationEvent {
    let actor = match message.role.as_str() {
        "user" => EventActor::User,
        "assistant" => EventActor::Assistant,
        _ => unreachable!("conversation messages only contain user or assistant roles"),
    };
    semantic_event(
        sequence,
        EventKind::Message,
        &message.occurred_at,
        Some(actor),
        None,
        Some(message.text.clone()),
        details,
    )
}

fn semantic_event(
    sequence: usize,
    kind: EventKind,
    occurred_at: &str,
    actor: Option<EventActor>,
    name: Option<String>,
    text: Option<String>,
    details: Value,
) -> ConversationEvent {
    ConversationEvent {
        event_id: String::new(),
        sequence: sequence as u32,
        source_file: String::new(),
        source_sequence: sequence as u32,
        kind,
        occurred_at: (!occurred_at.is_empty()).then(|| occurred_at.to_string()),
        actor,
        name,
        text,
        details,
        attachments: Vec::new(),
        capability_status: if occurred_at.is_empty() {
            EventStatus::MissingTimestamp
        } else {
            EventStatus::Complete
        },
        content_status: ContentStatus::Complete,
    }
}

fn response_semantic_event(
    sequence: usize,
    occurred_at: &str,
    payload: &Value,
    include_deferred_content: bool,
) -> Option<ConversationEvent> {
    let kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "message" => None,
        "function_call" | "custom_tool_call" | "web_search_call" | "local_shell_call" => {
            Some(semantic_event(
                sequence,
                EventKind::ToolCall,
                occurred_at,
                Some(EventActor::Assistant),
                optional_text(payload, &["name", "tool", "type"]),
                optional_text(payload, &["arguments", "input", "query", "command"]),
                payload.clone(),
            ))
        }
        "function_call_output" | "custom_tool_call_output" => Some(tool_result_event(
            sequence,
            occurred_at,
            payload,
            include_deferred_content,
        )),
        "reasoning" => Some(semantic_event(
            sequence,
            EventKind::Plan,
            occurred_at,
            Some(EventActor::Assistant),
            None,
            optional_text(payload, &["summary", "text", "content"]),
            payload.clone(),
        )),
        "developer" | "system" => None,
        _ => Some(unadapted_event(
            sequence,
            occurred_at,
            kind,
            payload.clone(),
        )),
    }
}

fn tool_result_event(
    sequence: usize,
    occurred_at: &str,
    payload: &Value,
    include_deferred_content: bool,
) -> ConversationEvent {
    let text = optional_text(payload, &["output", "result"]);
    let should_defer = !include_deferred_content
        && text
            .as_ref()
            .is_some_and(|text| text.len() > LARGE_CONTENT_THRESHOLD);
    let mut details = payload.clone();
    let rendered_text = if should_defer {
        if let Value::Object(object) = &mut details {
            object.remove("output");
            object.remove("result");
        }
        text.map(|text| text.chars().take(CONTENT_PREVIEW_CHARS).collect())
    } else {
        text
    };
    let mut event = semantic_event(
        sequence,
        EventKind::ToolResult,
        occurred_at,
        Some(EventActor::Tool),
        optional_text(payload, &["name"]),
        rendered_text,
        details,
    );
    if should_defer {
        event.content_status = ContentStatus::Deferred;
    }
    event
}

fn event_msg_semantic_event(
    sequence: usize,
    occurred_at: &str,
    kind: &str,
    payload: &Value,
) -> ConversationEvent {
    match kind {
        "plan_update" | "agent_reasoning" => semantic_event(
            sequence,
            EventKind::Plan,
            occurred_at,
            Some(EventActor::Assistant),
            None,
            optional_text(payload, &["explanation", "message", "text"]),
            payload.clone(),
        ),
        "error" | "stream_error" => semantic_event(
            sequence,
            EventKind::Error,
            occurred_at,
            None,
            optional_text(payload, &["code", "type"]),
            optional_text(payload, &["message", "error"]),
            payload.clone(),
        ),
        "task_started" | "task_complete" | "turn_aborted" | "context_compacted" | "warning" => {
            semantic_event(
                sequence,
                EventKind::SystemStatus,
                occurred_at,
                None,
                Some(kind.to_string()),
                optional_text(payload, &["message", "reason", "text"]),
                payload.clone(),
            )
        }
        _ => unadapted_event(sequence, occurred_at, kind, payload.clone()),
    }
}

fn unadapted_event(
    sequence: usize,
    occurred_at: &str,
    raw_kind: &str,
    details: Value,
) -> ConversationEvent {
    let mut event = semantic_event(
        sequence,
        EventKind::Unadapted,
        occurred_at,
        None,
        Some(if raw_kind.is_empty() {
            "unknown".to_string()
        } else {
            raw_kind.to_string()
        }),
        None,
        details,
    );
    event.capability_status = if occurred_at.is_empty() {
        EventStatus::UnadaptedMissingTimestamp
    } else {
        EventStatus::Unadapted
    };
    event
}

struct AttachmentCandidate {
    attachment: ConversationAttachment,
    source: String,
    resolved_path: Option<PathBuf>,
}

fn populate_attachments(events: &mut [ConversationEvent], project: &str) {
    for event in events {
        event.attachments = attachment_candidates(event.sequence, &event.details, project)
            .into_iter()
            .map(|candidate| candidate.attachment)
            .collect();
    }
}

fn strip_message_bodies_from_details(events: &mut [ConversationEvent]) {
    for event in events {
        if event.kind != EventKind::Message {
            continue;
        }
        if let Value::Object(object) = &mut event.details {
            object.remove("content");
            object.remove("message");
            object.remove("attachments");
        }
    }
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

fn read_source_payload(source: Source, path: &Path, sequence: u32) -> Result<Value, String> {
    if source == Source::Gemini {
        let content =
            fs::read_to_string(path).map_err(|error| format!("读取原始文件失败：{error}"))?;
        let root: Value = serde_json::from_str(&content)
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

fn attachment_candidates(
    sequence: u32,
    payload: &Value,
    project: &str,
) -> Vec<AttachmentCandidate> {
    let mut values = Vec::new();
    for key in ["content", "attachments"] {
        match payload.get(key) {
            Some(Value::Array(items)) => values.extend(items),
            Some(value @ Value::Object(_)) => values.push(value),
            _ => {}
        }
    }
    values
        .into_iter()
        .filter_map(|value| attachment_candidate(value, project))
        .enumerate()
        .map(|(index, mut candidate)| {
            candidate.attachment.id = format!("{sequence}:{index}");
            candidate
        })
        .collect()
}

fn attachment_candidate(value: &Value, project: &str) -> Option<AttachmentCandidate> {
    let object = value.as_object()?;
    let raw_type = object.get("type").and_then(Value::as_str).unwrap_or("");
    let kind = if raw_type.contains("image") {
        AttachmentKind::Image
    } else if raw_type.contains("file") || raw_type.contains("attachment") {
        AttachmentKind::File
    } else {
        return None;
    };
    let source = ["file_path", "path", "url", "image_url"]
        .iter()
        .find_map(|key| object.get(*key).and_then(attachment_source_value))?;
    let embedded = source.starts_with("data:");
    let remote = source.starts_with("http://") || source.starts_with("https://");
    let resolved_path = if embedded || remote {
        None
    } else {
        let path = PathBuf::from(&source);
        Some(if path.is_absolute() || project.is_empty() {
            path
        } else {
            PathBuf::from(project).join(path)
        })
    };
    let metadata = resolved_path
        .as_ref()
        .and_then(|path| fs::metadata(path).ok());
    let status = if embedded {
        AttachmentStatus::Embedded
    } else if remote {
        AttachmentStatus::Unsupported
    } else if metadata.is_some() {
        AttachmentStatus::Available
    } else {
        AttachmentStatus::Missing
    };
    let original_path = if embedded {
        "内嵌图片数据".to_string()
    } else {
        source.clone()
    };
    let name = first_text(value, &["name", "file_name"]);
    let name = if name.is_empty() {
        Path::new(&original_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(if kind == AttachmentKind::Image {
                "image"
            } else {
                "attachment"
            })
            .to_string()
    } else {
        name
    };
    let media_type = optional_text(value, &["mime_type", "media_type"])
        .unwrap_or_else(|| infer_media_type(&name, kind));
    Some(AttachmentCandidate {
        attachment: ConversationAttachment {
            id: String::new(),
            kind,
            name,
            original_path,
            media_type,
            size_bytes: metadata.map(|metadata| metadata.len()),
            status,
        },
        source,
        resolved_path,
    })
}

fn attachment_source_value(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.get("url").and_then(Value::as_str).map(str::to_string))
}

fn infer_media_type(name: &str, kind: AttachmentKind) -> String {
    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "md" => "text/markdown",
        "txt" => "text/plain",
        _ if kind == AttachmentKind::Image => "image/*",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn ensure_attachment_path_allowed(
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

fn attachment_data_url(candidate: &AttachmentCandidate) -> Result<String, String> {
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

fn attachment_thumbnail_data_url(candidate: &AttachmentCandidate) -> Result<String, String> {
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

fn attachment_bytes(candidate: &AttachmentCandidate) -> Result<Vec<u8>, String> {
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

fn optional_text(value: &Value, keys: &[&str]) -> Option<String> {
    let text = first_text(value, keys);
    (!text.is_empty()).then_some(text)
}

fn response_message(payload: &Value, occurred_at: &str) -> Option<ConversationMessage> {
    if payload.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    let role = payload.get("role").and_then(Value::as_str)?;
    if role != "user" && role != "assistant" {
        return None;
    }
    message(role, occurred_at, payload.get("content")?)
}

fn event_message(payload: &Value, occurred_at: &str) -> Option<ConversationMessage> {
    let role = match payload.get("type").and_then(Value::as_str)? {
        "user_message" => "user",
        "agent_message" => "assistant",
        _ => return None,
    };
    message(role, occurred_at, payload.get("message")?)
}

fn message(role: &str, occurred_at: &str, content: &Value) -> Option<ConversationMessage> {
    let text = content_text(content).trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(ConversationMessage {
        role: role.to_string(),
        occurred_at: occurred_at.to_string(),
        text,
    })
}

fn content_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(content_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                [
                    "content",
                    "output",
                    "result",
                    "response",
                    "functionResponse",
                ]
                .iter()
                .find_map(|key| object.get(*key).map(content_text))
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn first_text(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

fn text_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn strip_prompt_wrappers(text: &str) -> String {
    let mut remaining = text;
    let mut stripped = String::new();
    while let Some(start) = remaining.find("<timestamp>") {
        stripped.push_str(&remaining[..start]);
        let after = &remaining[start + "<timestamp>".len()..];
        match after.find("</timestamp>") {
            Some(end) => remaining = &after[end + "</timestamp>".len()..],
            None => {
                remaining = "";
                break;
            }
        }
    }
    stripped.push_str(remaining);
    stripped
        .replace("<user_query>", " ")
        .replace("</user_query>", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_title(text: &str) -> String {
    let mut chars = text.chars();
    let title: String = chars.by_ref().take(TITLE_MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{title}…")
    } else {
        title
    }
}

fn upsert_session(
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

fn failed_session_paths(
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

fn update_session_files(
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

fn load_agent_relations(
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

fn load_agent_catalog(
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

fn build_parent_link(
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

fn agent_link_status(
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

fn agent_path_exists(
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

fn sync_cursor_usage_only_sessions(conn: &Connection) -> Result<(), String> {
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

fn load_usage_records(
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

fn usage_record_identity(record: &UsageRecord) -> String {
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

fn load_session(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Option<ConversationSessionRow>, String> {
    let mut session = conn
        .query_row(
            r#"
            SELECT source, session_id, title, project, model, started_at, ended_at,
                   source_file, capabilities_json, support_status, file_available
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
    }
    Ok(session)
}

fn load_session_files(
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

fn load_trusted_session_files(
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

fn ensure_matching_session(
    parsed: &ParsedConversation,
    session: &ConversationSessionRow,
) -> Result<(), String> {
    if parsed.session.session_id == session.session_id {
        Ok(())
    } else {
        Err("原始文件中的会话 ID 与索引不一致".to_string())
    }
}

fn tombstone_missing_sessions(
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

fn mark_session_unavailable(
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

fn row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationSessionRow> {
    let capabilities_json: String = row.get(8)?;
    let source_file: String = row.get(7)?;
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
        ..Default::default()
    })
}

fn load_cached_fingerprints(
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

fn modified_nanos(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
        .unwrap_or(0)
}

fn metadata_revision(metadata: &fs::Metadata) -> String {
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{modified_ns}:{}", metadata.len())
}

fn ensure_trusted_path(path: &Path, roots: &[PathBuf]) -> Result<(), String> {
    let canonical_path =
        fs::canonicalize(path).map_err(|error| format!("无法验证原始文件路径：{error}"))?;
    ensure_canonical_path_in_roots(&canonical_path, roots)
}

fn ensure_canonical_path_in_roots(canonical_path: &Path, roots: &[PathBuf]) -> Result<(), String> {
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

fn session_source_paths(session: &ConversationSessionRow) -> Result<Vec<PathBuf>, String> {
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

fn trusted_paths_for_session(
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

fn files_revision(source: Source, paths: &[PathBuf]) -> Result<String, String> {
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

fn detail_files_revision(
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

fn detail_file_revision(
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

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
