use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::Connection;

use crate::adapters::cursor_session::{
    apply_hash_enrichment, build_cursor_session_record, group_members_changed, group_transcripts,
    load_hash_enrichments, merge_parsed_sessions, parse_cursor_session_transcript, TranscriptGroup,
};
use crate::domain::{IngestIssue, IngestReport, Source};
use crate::store;

pub const SOURCE_LABEL: &str = "cursor-session";

pub use crate::cursor_session_query::{load_summary, sessions_page, summarize_cursor_sessions};

/// 全量摄取或重建 Cursor Agent 消耗记录时，同步刷新本机 Cursor 会话。
pub fn ingest_for_usage_source(
    conn: &Connection,
    home: &Path,
    source: Option<Source>,
    report: &mut IngestReport,
) {
    if source.is_none() || source == Some(Source::CursorAgent) {
        ingest(conn, home, report);
    }
}

pub fn ingest(conn: &Connection, home: &Path, report: &mut IngestReport) {
    let root = home.join(".cursor").join("projects");
    if !root.exists() {
        return;
    }

    let transcripts = match walk_transcripts(&root) {
        Ok(paths) => paths,
        Err(error) => {
            record_issue(report, &root.to_string_lossy(), &error);
            return;
        }
    };

    let current_fp = tracking_db_fingerprint(home);
    let stored_fp = store::cursor_tracking_fingerprint(conn).unwrap_or_default();
    let tracking_changed = current_fp != stored_fp;
    let force_rebuild = store::cursor_session_schema_version(conn).unwrap_or_default()
        != store::CURSOR_SESSION_SCHEMA_VERSION;

    let groups = group_transcripts(transcripts);
    let cached_files = store::cached_cursor_session_file_stats(conn).unwrap_or_default();
    let mut seen_session_paths = BTreeSet::new();
    let mut seen_file_paths = BTreeSet::new();
    let mut pending = Vec::new();
    let mut any_failed = false;

    for group in groups {
        let mut files = Vec::new();
        if let Some(parent) = group.parent.as_ref() {
            files.push(parent.clone());
            seen_session_paths.insert(parent.to_string_lossy().into_owned());
        }
        files.extend(group.subagents.iter().cloned());

        let mut metas = Vec::new();
        let mut group_ready = true;
        for path in &files {
            let path_key = path.to_string_lossy().to_string();
            report.files_seen += 1;
            seen_file_paths.insert(path_key.clone());
            match fs::metadata(path) {
                Ok(meta) => metas.push((path_key, modified_millis(&meta), meta.len() as i64)),
                Err(error) => {
                    record_issue(report, &path_key, &format!("读取文件元数据失败：{error}"));
                    any_failed = true;
                    group_ready = false;
                }
            }
        }
        if !group_ready {
            continue;
        }

        let current_keys: BTreeSet<String> =
            metas.iter().map(|(path, _, _)| path.clone()).collect();
        if !force_rebuild
            && !group_members_changed(&group.session_dir, &cached_files, &current_keys)
            && group_unchanged(
                conn,
                group.parent.as_deref(),
                &metas,
                report,
                &mut any_failed,
            )
        {
            continue;
        }
        pending.push((group, metas));
    }

    let enrichments = if tracking_changed || !pending.is_empty() || force_rebuild {
        match load_hash_enrichments(home) {
            Ok(map) => Some(map),
            Err(error) => {
                record_issue(report, &root.to_string_lossy(), &error);
                None
            }
        }
    } else {
        None
    };

    for (group, metas) in pending {
        if !ingest_group(conn, &group, &metas, enrichments.as_ref(), report) {
            any_failed = true;
        }
    }

    if !any_failed {
        match store::reconcile_cursor_sessions(conn, &seen_session_paths) {
            Ok(removed) => report.records_removed += removed,
            Err(error) => record_issue(report, &root.to_string_lossy(), &error),
        }
        if let Err(error) = store::reconcile_cursor_session_files(conn, &seen_file_paths) {
            record_issue(report, &root.to_string_lossy(), &error);
        }
        if force_rebuild {
            if let Err(error) =
                store::set_cursor_session_schema_version(conn, store::CURSOR_SESSION_SCHEMA_VERSION)
            {
                record_issue(report, &root.to_string_lossy(), &error);
            }
        }
    }

    if tracking_changed {
        if let Some(map) = enrichments.as_ref() {
            match refresh_hash_enrichments(conn, map) {
                Ok(()) => {
                    if let Err(error) = store::set_cursor_tracking_fingerprint(conn, &current_fp) {
                        record_issue(report, &root.to_string_lossy(), &error);
                    }
                }
                Err(error) => record_issue(report, &root.to_string_lossy(), &error),
            }
        }
    }

    let _ = store::set_cursor_session_as_of(conn, &chrono::Utc::now().to_rfc3339());
}

fn group_unchanged(
    conn: &Connection,
    parent: Option<&Path>,
    metas: &[(String, i64, i64)],
    report: &mut IngestReport,
    any_failed: &mut bool,
) -> bool {
    for (path_key, mtime_ms, size) in metas {
        let Ok(Some((cached_mtime, cached_size))) =
            store::cursor_session_file_fingerprint(conn, path_key)
        else {
            return false;
        };
        if cached_mtime != *mtime_ms || cached_size != *size {
            return false;
        }
    }

    if let Some(parent) = parent {
        match store::cursor_session_has_source_file(conn, &parent.to_string_lossy()) {
            Ok(true) => {
                report.files_skipped += metas.len() as u64;
                true
            }
            Ok(false) => false,
            Err(error) => {
                record_issue(report, &parent.to_string_lossy(), &error);
                *any_failed = true;
                true
            }
        }
    } else {
        report.files_skipped += metas.len() as u64;
        true
    }
}

fn ingest_group(
    conn: &Connection,
    group: &TranscriptGroup,
    metas: &[(String, i64, i64)],
    enrichments: Option<&BTreeMap<String, crate::adapters::cursor_session::SessionHashEnrichment>>,
    report: &mut IngestReport,
) -> bool {
    let Some(parent) = group.parent.as_ref() else {
        if let Err(error) = persist_cursor_session_group(conn, None, metas) {
            record_issue(report, &group.session_dir.to_string_lossy(), &error);
            return false;
        }
        report.files_parsed += metas.len() as u64;
        return true;
    };

    let parent_key = parent.to_string_lossy().to_string();
    let mut parsed = match read_and_parse(parent, &parent_key, report) {
        Some(parsed) => parsed,
        None => return false,
    };
    let mut subagent_count = 0i64;
    for extra in &group.subagents {
        let extra_key = extra.to_string_lossy().to_string();
        let Some(extra_parsed) = read_and_parse(extra, &extra_key, report) else {
            return false;
        };
        merge_parsed_sessions(&mut parsed, &extra_parsed);
        subagent_count += 1;
    }

    let parent_mtime = metas
        .iter()
        .find(|(path, _, _)| path == &parent_key)
        .map(|(_, mtime, _)| *mtime)
        .unwrap_or(0);
    let seen_at = millis_to_rfc3339(parent_mtime);
    let mut record = match build_cursor_session_record(&parent_key, &parsed, seen_at) {
        Ok(record) => record,
        Err(error) => {
            record_issue(report, &parent_key, &error);
            return false;
        }
    };
    record.subagent_count = subagent_count;
    if let Some(enrichment) = enrichments.and_then(|map| map.get(&record.session_id)) {
        if let Err(error) = apply_hash_enrichment(&mut record, enrichment) {
            record_issue(report, &parent_key, &error);
            return false;
        }
    }

    if let Err(error) = persist_cursor_session_group(conn, Some(&record), metas) {
        record_issue(report, &parent_key, &error);
        return false;
    }
    report.files_parsed += metas.len() as u64;
    true
}

fn persist_cursor_session_group(
    conn: &Connection,
    record: Option<&crate::domain::CursorSessionRecord>,
    metas: &[(String, i64, i64)],
) -> Result<(), String> {
    conn.execute_batch("SAVEPOINT cursor_session_group")
        .map_err(|error| error.to_string())?;
    let result = (|| {
        if let Some(record) = record {
            store::upsert_cursor_session(conn, record)?;
        }
        for (path, mtime_ms, size) in metas {
            store::upsert_cursor_session_file(conn, path, *mtime_ms, *size)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = conn.execute_batch(
            "ROLLBACK TO SAVEPOINT cursor_session_group; RELEASE SAVEPOINT cursor_session_group",
        );
        return Err(error);
    }
    conn.execute_batch("RELEASE SAVEPOINT cursor_session_group")
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn read_and_parse(
    path: &Path,
    path_key: &str,
    report: &mut IngestReport,
) -> Option<crate::adapters::cursor_session::ParsedCursorSession> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            record_issue(report, path_key, &format!("读取 transcript 失败：{error}"));
            return None;
        }
    };
    match parse_cursor_session_transcript(&content) {
        Ok(parsed) => Some(parsed),
        Err(error) => {
            record_issue(report, path_key, &error);
            None
        }
    }
}

fn refresh_hash_enrichments(
    conn: &Connection,
    enrichments: &BTreeMap<String, crate::adapters::cursor_session::SessionHashEnrichment>,
) -> Result<(), String> {
    let sessions = store::load_cursor_sessions(conn)?;
    for mut session in sessions {
        if let Some(enrichment) = enrichments.get(&session.session_id) {
            let previous = session.clone();
            apply_hash_enrichment(&mut session, enrichment)?;
            if session.models_json == previous.models_json
                && session.sources_json == previous.sources_json
                && session.extensions_json == previous.extensions_json
                && session.files_touched == previous.files_touched
                && session.first_seen_at == previous.first_seen_at
                && session.last_seen_at == previous.last_seen_at
            {
                continue;
            }
        } else if session.models_json != "[]"
            || session.files_touched != 0
            || session.sources_json != "[]"
            || session.extensions_json != "{}"
        {
            session.models_json = "[]".to_string();
            session.sources_json = "[]".to_string();
            session.extensions_json = "{}".to_string();
            session.files_touched = 0;
        } else {
            continue;
        }
        store::upsert_cursor_session(conn, &session)?;
    }
    Ok(())
}

/// 托盘心跳用：transcript 指纹或代码量 sqlite 变化时视为 stale。
/// 缓存由调用方在读锁内取出，本函数只扫盘比对，不再碰数据库。
pub(crate) fn scan_is_stale_cached(
    cached: &BTreeMap<String, (i64, i64)>,
    tracking_fingerprint: &str,
    home: &Path,
) -> Result<bool, String> {
    let root = home.join(".cursor").join("projects");
    let transcripts = if root.exists() {
        walk_transcripts(&root)?
    } else {
        Vec::new()
    };
    if transcripts.is_empty() && cached.is_empty() {
        return Ok(false);
    }
    let seen: BTreeSet<String> = transcripts
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    if cached.len() != seen.len() || cached.keys().any(|path| !seen.contains(path)) {
        return Ok(true);
    }
    for path in transcripts {
        let loc = path.to_string_lossy().to_string();
        let meta = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => return Ok(true),
        };
        match cached.get(&loc) {
            Some((mtime, size))
                if *mtime == modified_millis(&meta) && *size == meta.len() as i64 => {}
            _ => return Ok(true),
        }
    }

    Ok(tracking_db_fingerprint(home) != tracking_fingerprint)
}

fn tracking_db_fingerprint(home: &Path) -> String {
    let path = home.join(".cursor/ai-tracking/ai-code-tracking.db");
    match fs::metadata(&path) {
        Ok(meta) => format!("{}|{}", modified_millis(&meta), meta.len()),
        Err(_) => String::new(),
    }
}

fn walk_transcripts(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let entries = fs::read_dir(root)
        .map_err(|e| format!("扫描 Cursor 会话目录 {} 失败：{e}", root.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let transcripts = entry.path().join("agent-transcripts");
        if !transcripts.is_dir() {
            continue;
        }
        collect_transcript_jsonl(&transcripts, &mut files)?;
    }
    files.sort();
    Ok(files)
}

fn collect_transcript_jsonl(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir)
        .map_err(|e| format!("扫描 Cursor 会话目录 {} 失败：{e}", dir.display()))?
    {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect_transcript_jsonl(&path, files)?;
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.ends_with(".jsonl") {
            files.push(path);
        }
    }
    Ok(())
}

fn record_issue(report: &mut IngestReport, path: &str, message: &str) {
    report.files_failed += 1;
    report.partial_success = true;
    report.issues.push(IngestIssue {
        source: SOURCE_LABEL.to_string(),
        path: path.to_string(),
        message: message.to_string(),
        event_type: None,
        line: None,
    });
}

fn modified_millis(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn millis_to_rfc3339(millis: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(millis).map(|dt| dt.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::cursor_session::project_from_transcript_path;
    use crate::domain::CursorSessionRecord;

    #[test]
    fn project_from_transcript_path_decodes_slug() {
        let path =
            Path::new("/home/.cursor/projects/Users-test-project/agent-transcripts/s1/s1.jsonl");
        assert_eq!(project_from_transcript_path(path), "/Users/test/project");
    }

    fn sample_session(
        session_id: &str,
        project: &str,
        last_seen_at: &str,
        turn_count: i64,
        error_count: i64,
        models_json: &str,
        tool_calls_json: &str,
    ) -> CursorSessionRecord {
        CursorSessionRecord {
            session_id: session_id.to_string(),
            project: project.to_string(),
            turn_count,
            success_count: turn_count - error_count,
            error_count,
            aborted_count: 0,
            user_prompt_count: 1,
            subagent_count: 0,
            tool_calls_json: tool_calls_json.to_string(),
            models_json: models_json.to_string(),
            sources_json: r#"["composer"]"#.to_string(),
            extensions_json: r#"{"rs":1}"#.to_string(),
            first_seen_at: Some(last_seen_at.to_string()),
            last_seen_at: Some(last_seen_at.to_string()),
            files_touched: 1,
            source_file: format!("/tmp/{session_id}.jsonl"),
        }
    }

    #[test]
    fn summarize_includes_session_ids_newest_first() {
        let summary = summarize_cursor_sessions(&[
            sample_session(
                "sess-old",
                "/Users/test/alpha",
                "2026-08-16T10:00:00+00:00",
                2,
                1,
                r#"["grok-4.6"]"#,
                r#"{"Read":1}"#,
            ),
            sample_session(
                "sess-new",
                "/Users/test/beta",
                "2026-08-18T10:00:00+00:00",
                3,
                0,
                r#"["grok-4.6"]"#,
                r#"{"Read":2,"Shell":1}"#,
            ),
        ]);

        assert_eq!(summary.session_count, 2);
        assert_eq!(summary.active_project_count, 2);
        assert_eq!(summary.turn_count, 5);
        assert_eq!(summary.aborted_count, 0);
        assert_eq!(summary.user_prompt_count, 2);
        assert_eq!(summary.average_turns, Some(2.5));
        assert_eq!(summary.by_project.len(), 2);
        assert_eq!(summary.top_tools[0].name, "Read");
        assert_eq!(summary.top_tools[0].call_count, 3);
        assert_eq!(summary.tool_groups[0].name, "read");
        assert_eq!(summary.tool_groups[0].call_count, 3);
        assert_eq!(summary.by_source[0].name, "composer");
        let beta = summary
            .by_project
            .iter()
            .find(|row| row.name == "/Users/test/beta")
            .expect("beta project");
        assert_eq!(beta.session_count, 1);
        assert_eq!(beta.turn_count, 3);
        assert_eq!(beta.error_count, 0);
        assert_eq!(beta.files_touched, 1);
        assert_eq!(
            beta.last_seen_at.as_deref(),
            Some("2026-08-18T10:00:00+00:00")
        );
    }
}
