use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::domain::Source;
use crate::ingest;

use super::catalog::conversation_source_paths;
use super::cursor_bridge::{sync_cursor_hash_models, sync_cursor_usage_only_sessions};
use super::event_index;
use super::merge::{merge_indexed_files, summarize_for_index, IndexedFile};
use super::persist::{
    apply_incremental, persist_session_file_cursors, prepare_incremental, record_full_parse,
    write_session_file_events, IncrementalPrepare, PendingIncremental,
};
use super::session_store::{
    failed_session_paths, load_cached_fingerprints, load_session_files, mark_session_unavailable,
    tombstone_missing_sessions, update_session_files, upsert_session,
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
    let roots = ingest::source_scan_dirs(home, Source::Codex);
    refresh(conn, Source::Codex, &roots)
}

pub(crate) fn refresh(
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
    let mut persist_skip = blocked_session_ids;
    persist_skip.extend(incomplete_session_ids.iter().cloned());
    context_cache::persist_seen_sessions(conn, source, &seen_session_ids, &persist_skip)?;
    if blocking_issues.is_empty() {
        tombstone_missing_sessions(conn, source, &seen_session_ids)?;
    }
    if source == Source::CursorAgent {
        sync_cursor_usage_only_sessions(conn)?;
        sync_cursor_hash_models(conn)?;
    }
    Ok(issues)
}
