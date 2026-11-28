use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::domain::Source;
use crate::scan_paths::ScanEnv;

use super::catalog::conversation_source_paths;
use super::cursor_bridge::{sync_cursor_hash_models, sync_cursor_usage_only_sessions};
use super::event_store::{
    commit_incremental_append, commit_session, has_live_generation, prepare_incremental,
    IncrementalPrepare, PendingIncremental, SessionCommit, SessionEventStager,
};
use super::session_store::{
    failed_session_paths, load_cached_fingerprints, load_session_files, mark_session_unavailable,
    tombstone_missing_sessions, update_session_files,
};
use super::toolbox::FileIndexCursor;
use super::trusted_path::modified_nanos;
use super::{
    context_cache, conversation_adapter, plan_conversation_file_index, ConversationFileFingerprint,
    ConversationFileIndexPlan, ConversationIndexIssue,
};

pub fn refresh_codex(
    conn: &Connection,
    home: &Path,
) -> Result<Vec<ConversationIndexIssue>, String> {
    let roots = ScanEnv::capture_for_home(home).conversation_catalog_roots(Source::Codex);
    refresh(conn, Source::Codex, &roots)
}

pub fn refresh_claude(
    conn: &Connection,
    home: &Path,
) -> Result<Vec<ConversationIndexIssue>, String> {
    let roots = ScanEnv::capture_for_home(home).conversation_catalog_roots(Source::Claude);
    refresh(conn, Source::Claude, &roots)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshMode {
    /// 需要整份解析的会话连事件、正文倒排一起重写。
    Full,
    /// 需要整份解析的会话只更新会话行与会话文件清单，事件留给补建。
    CatalogOnly,
}

pub(crate) fn refresh(
    conn: &Connection,
    source: Source,
    roots: &[PathBuf],
) -> Result<Vec<ConversationIndexIssue>, String> {
    refresh_with_mode(conn, source, roots, RefreshMode::Full)
}

/// 与 `refresh` 同一套发现、指纹与对账，但需要整份解析的会话不写事件、不写正文倒排、
/// 不换 `event_index_generation`：旧一代事件原样保留，会话标成待补建。会话文件的指纹照常
/// 推进，源文件不再变化时下一轮直接跳过；事件游标不推进，待补建会话也不走增量，
/// 要等补建整份重写。未待补建的可追加会话照常增量。
///
/// `catalog_writes_events` 的来源（OpenCode 整库快照）库一变就整份解析全部会话，若也标待补建，
/// 补建要为每个会话各读一遍整库，所以这类来源仍按整份写事件。
pub(crate) fn refresh_catalog(
    conn: &Connection,
    source: Source,
    roots: &[PathBuf],
) -> Result<Vec<ConversationIndexIssue>, String> {
    let mode = if conversation_adapter(source)?.catalog_writes_events {
        RefreshMode::Full
    } else {
        RefreshMode::CatalogOnly
    };
    refresh_with_mode(conn, source, roots, mode)
}

fn refresh_with_mode(
    conn: &Connection,
    source: Source,
    roots: &[PathBuf],
    mode: RefreshMode,
) -> Result<Vec<ConversationIndexIssue>, String> {
    let adapter = conversation_adapter(source)?;
    let mut issues = Vec::new();
    let mut blocking_issues = Vec::new();
    // 存摘要而不是 `ParsedConversation`：扫描期间只有当前文件的事件活着，
    // 而不是整个来源的全部事件。
    let mut stager = SessionEventStager::default();
    let mut unchanged_paths: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut incremental_paths: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut pending_incrementals: Vec<PendingIncremental> = Vec::new();
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
        let cached = load_cached_fingerprints(conn, source, &path)?;
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
                            origin: FileIndexCursor {
                                byte_offset: row.indexed_byte_offset,
                                line: row.indexed_line,
                            },
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
                    match mode {
                        RefreshMode::Full => stager.stage_full_parse(conn, source, parsed)?,
                        RefreshMode::CatalogOnly => stager.stage_catalog_parse(parsed),
                    }
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
    for (session_id, indexed_files) in &stager.grouped {
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
        stager.grouped.remove(session_id);
        unchanged_paths.remove(session_id);
    }

    for (session_id, paths) in std::mem::take(&mut unchanged_paths) {
        let indexed_paths = load_session_files(conn, source.as_str(), &session_id)?;
        let scanned = paths.iter().cloned().collect::<BTreeSet<_>>();
        let indexed = indexed_paths.into_iter().collect::<BTreeSet<_>>();
        if stager.grouped.contains_key(&session_id) || scanned != indexed {
            for path in paths {
                match (adapter.index)(&path) {
                    Ok(batch) => {
                        issues.extend(batch.diagnostics);
                        for parsed in batch.conversations {
                            if parsed.session.session_id != session_id {
                                continue;
                            }
                            match mode {
                                RefreshMode::Full => {
                                    stager.stage_full_parse(conn, source, parsed)?
                                }
                                RefreshMode::CatalogOnly => stager.stage_catalog_parse(parsed),
                            }
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

    let mut refreshed_session_ids = BTreeSet::new();
    for pending in pending_incrementals {
        if blocked_session_ids.contains(&pending.session_id)
            || stager.grouped.contains_key(&pending.session_id)
            || incomplete_session_ids.contains(&pending.session_id)
        {
            continue;
        }
        let path = pending.path.to_string_lossy().to_string();
        let session_id = pending.session_id.clone();
        if let Err(message) = commit_incremental_append(conn, source, pending) {
            let issue = ConversationIndexIssue {
                path,
                message,
                event_type: None,
                line: None,
            };
            blocking_issues.push(issue.clone());
            issues.push(issue);
        } else {
            refreshed_session_ids.insert(session_id);
        }
    }

    let seen_session_ids = unchanged_paths
        .keys()
        .chain(stager.grouped.keys())
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
    let grouped = std::mem::take(&mut stager.grouped);
    for (session_id, indexed_files) in grouped {
        if blocked_session_ids.contains(&session_id) {
            continue;
        }
        refreshed_session_ids.insert(session_id.clone());
        let publish_generation = match stager.generation_for(&session_id) {
            Some(generation)
                if !has_unmapped_failures || has_live_generation(conn, source, &session_id)? =>
            {
                Some(generation)
            }
            _ => None,
        };
        commit_session(
            conn,
            SessionCommit {
                source,
                session_id: &session_id,
                indexed_files,
                replace_files: blocking_issues.is_empty(),
                publish_generation,
                file_cursors: stager.file_cursors(),
                mark_dirty: mode == RefreshMode::CatalogOnly,
            },
        )?;
    }
    let mut persist_skip = blocked_session_ids;
    persist_skip.extend(incomplete_session_ids.iter().cloned());
    context_cache::persist_seen_sessions(
        conn,
        source,
        &seen_session_ids,
        &refreshed_session_ids,
        &persist_skip,
    )?;
    if blocking_issues.is_empty() {
        tombstone_missing_sessions(conn, source, &seen_session_ids)?;
    }
    if source == Source::CursorAgent {
        sync_cursor_usage_only_sessions(conn)?;
        sync_cursor_hash_models(conn)?;
    }
    Ok(issues)
}
