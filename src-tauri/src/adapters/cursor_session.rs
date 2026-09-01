use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::adapters::project;
use crate::domain::CursorSessionRecord;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionHashEnrichment {
    pub models: BTreeSet<String>,
    pub files: BTreeSet<String>,
    pub sources: BTreeSet<String>,
    pub extensions: BTreeMap<String, i64>,
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedCursorSession {
    pub turn_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub aborted_count: i64,
    pub user_prompt_count: i64,
    pub tool_calls: BTreeMap<String, i64>,
    pub read_paths: BTreeSet<String>,
    pub write_paths: BTreeSet<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TranscriptGroup {
    pub session_dir: PathBuf,
    pub parent: Option<PathBuf>,
    pub subagents: Vec<PathBuf>,
}

/// 从 agent-transcripts jsonl 解析单会话聚合；不读取 user/assistant 正文。
pub fn parse_cursor_session_transcript(content: &str) -> Result<ParsedCursorSession, String> {
    let mut values = Vec::new();
    let mut parse_errors = 0usize;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) => values.push(value),
            Err(_) => parse_errors += 1,
        }
    }
    if parse_errors > 0 {
        return Err(format!(
            "Cursor 会话 transcript JSON 解析失败：{parse_errors} 行无效"
        ));
    }

    let mut parsed = ParsedCursorSession::default();

    for value in &values {
        if value.get("type").and_then(|v| v.as_str()) == Some("turn_ended") {
            parsed.turn_count += 1;
            match value.get("status").and_then(|v| v.as_str()) {
                Some("success") => parsed.success_count += 1,
                Some("error") => parsed.error_count += 1,
                Some("aborted") => parsed.aborted_count += 1,
                _ => {}
            }
            continue;
        }
        if value.get("role").and_then(|v| v.as_str()) == Some("user") {
            parsed.user_prompt_count += 1;
            continue;
        }
        if value.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let Some(blocks) = value
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_array())
        else {
            continue;
        };
        for block in blocks {
            if block.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
                continue;
            }
            let name = block
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            *parsed.tool_calls.entry(name.clone()).or_insert(0) += 1;
            if let Some(kind) = path_kind(&name) {
                let input = block
                    .get("input")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                for path in tool_input_paths(&input) {
                    match kind {
                        "read" => {
                            parsed.read_paths.insert(path);
                        }
                        "write" => {
                            parsed.write_paths.insert(path);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(parsed)
}

pub fn merge_parsed_sessions(into: &mut ParsedCursorSession, other: &ParsedCursorSession) {
    into.turn_count += other.turn_count;
    into.success_count += other.success_count;
    into.error_count += other.error_count;
    into.aborted_count += other.aborted_count;
    into.user_prompt_count += other.user_prompt_count;
    for (name, count) in &other.tool_calls {
        *into.tool_calls.entry(name.clone()).or_insert(0) += count;
    }
    into.read_paths.extend(other.read_paths.iter().cloned());
    into.write_paths.extend(other.write_paths.iter().cloned());
}

pub fn is_subagent_transcript(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "subagents")
}

/// 同一 `session_id` 出现在多条 transcript 时，选更像正式会话的那条。
/// 子代理路径和 Cursor 的 `empty-window` 副本都不应盖过真实项目里的父 jsonl。
pub fn prefer_new_cursor_session_path(existing: &str, new: &str) -> bool {
    cursor_session_path_rank(new) >= cursor_session_path_rank(existing)
}

fn cursor_session_path_rank(path: &str) -> u8 {
    let normalized = path.replace('\\', "/");
    if normalized.split('/').any(|part| part == "subagents") {
        return 0;
    }
    if normalized.split('/').any(|part| part == "empty-window") {
        return 1;
    }
    2
}

pub fn session_dir_from_transcript(path: &Path) -> Option<PathBuf> {
    let mut dir = path.parent()?.to_path_buf();
    if dir.file_name()?.to_str() == Some("subagents") {
        dir = dir.parent()?.to_path_buf();
    }
    let parent = dir.parent()?;
    if parent.file_name()?.to_str() == Some("agent-transcripts") {
        Some(dir)
    } else {
        None
    }
}

pub fn group_transcripts(paths: Vec<PathBuf>) -> Vec<TranscriptGroup> {
    let mut groups: BTreeMap<PathBuf, TranscriptGroup> = BTreeMap::new();
    for path in paths {
        let Some(dir) = session_dir_from_transcript(&path) else {
            continue;
        };
        let entry = groups
            .entry(dir.clone())
            .or_insert_with(|| TranscriptGroup {
                session_dir: dir,
                parent: None,
                subagents: Vec::new(),
            });
        if is_subagent_transcript(&path) {
            entry.subagents.push(path);
            continue;
        }
        let session_id = entry.session_dir.file_name().and_then(|name| name.to_str());
        let stem = path.file_stem().and_then(|name| name.to_str());
        if session_id.is_some() && stem == session_id {
            entry.parent = Some(path);
        }
    }
    groups.into_values().collect()
}

pub fn group_members_changed(
    session_dir: &Path,
    cached_files: &[(String, i64, i64)],
    current_keys: &BTreeSet<String>,
) -> bool {
    let prefix = format!("{}{}", session_dir.display(), std::path::MAIN_SEPARATOR);
    cached_files
        .iter()
        .any(|(path, _, _)| path.starts_with(&prefix) && !current_keys.contains(path))
}

pub fn path_kind(name: &str) -> Option<&'static str> {
    match tool_group(name) {
        "read" => Some("read"),
        "write" => Some("write"),
        _ => None,
    }
}

pub fn tool_input_paths(input: &serde_json::Value) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(path) = input.get("path").and_then(|value| value.as_str()) {
        if !path.is_empty() {
            paths.push(path.to_string());
        }
    }
    if let Some(items) = input.get("paths").and_then(|value| value.as_array()) {
        for item in items {
            if let Some(path) = item.as_str() {
                if !path.is_empty() {
                    paths.push(path.to_string());
                }
            }
        }
    }
    paths
}

pub fn load_hash_files(
    home: &Path,
    conversation_id: &str,
) -> Result<Vec<crate::domain::CursorSessionHashFile>, String> {
    use crate::domain::CursorSessionHashFile;

    let db_path = home.join(".cursor/ai-tracking/ai-code-tracking.db");
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    let conn = open_readonly(&db_path)?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT fileName, fileExtension, source
            FROM ai_code_hashes
            WHERE conversationId = ?1 AND fileName IS NOT NULL AND fileName != ''
            "#,
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([conversation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    for row in rows {
        let (path, extension, source) = row.map_err(|e| e.to_string())?;
        if !seen.insert(path.clone()) {
            continue;
        }
        files.push(CursorSessionHashFile {
            path,
            extension: extension.unwrap_or_default(),
            source: source.unwrap_or_default(),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

pub fn tool_group(name: &str) -> &'static str {
    match name {
        "Read" | "ReadFile" | "Grep" | "Glob" | "rg" | "SemanticSearch" | "SearchConversations" => {
            "read"
        }
        "StrReplace" | "Write" | "Delete" | "ApplyPatch" => "write",
        "Shell" | "AwaitShell" | "Await" | "gh" => "shell",
        "WebFetch" | "WebSearch" => "web",
        "Task" | "Subagent" => "agent",
        _ => "other",
    }
}

pub fn project_from_transcript_path(path: &Path) -> String {
    let mut saw_projects = false;
    for component in path.components() {
        let part = component.as_os_str().to_string_lossy();
        if saw_projects {
            return project::decode_dashed_dir(&part);
        }
        if part == "projects" {
            saw_projects = true;
        }
    }
    String::new()
}

pub fn build_cursor_session_record(
    source_file: &str,
    parsed: &ParsedCursorSession,
    seen_at: Option<String>,
) -> Result<CursorSessionRecord, String> {
    let tool_calls_json = serde_json::to_string(&parsed.tool_calls).map_err(|e| e.to_string())?;
    Ok(CursorSessionRecord {
        session_id: project::session_id_from_source_file(source_file),
        project: project_from_transcript_path(Path::new(source_file)),
        turn_count: parsed.turn_count,
        success_count: parsed.success_count,
        error_count: parsed.error_count,
        aborted_count: parsed.aborted_count,
        user_prompt_count: parsed.user_prompt_count,
        subagent_count: 0,
        tool_calls_json,
        models_json: "[]".to_string(),
        sources_json: "[]".to_string(),
        extensions_json: "{}".to_string(),
        first_seen_at: seen_at.clone(),
        last_seen_at: seen_at,
        files_touched: 0,
        source_file: source_file.to_string(),
    })
}

/// 只读加载 ai_code_hashes，按 conversationId 聚合 enrich 字段。
pub fn load_hash_enrichments(
    home: &Path,
) -> Result<BTreeMap<String, SessionHashEnrichment>, String> {
    let db_path = home.join(".cursor/ai-tracking/ai-code-tracking.db");
    if !db_path.exists() {
        return Ok(BTreeMap::new());
    }
    let conn = open_readonly(&db_path)?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT conversationId, model, timestamp, fileName, source, fileExtension
            FROM ai_code_hashes
            WHERE conversationId IS NOT NULL AND conversationId != ''
            "#,
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut enrichments: BTreeMap<String, SessionHashEnrichment> = BTreeMap::new();
    for row in rows {
        let (conversation_id, model, timestamp, file_name, source, extension) =
            row.map_err(|e| e.to_string())?;
        let entry = enrichments.entry(conversation_id).or_default();
        if let Some(model) = model.filter(|value| !value.is_empty()) {
            entry.models.insert(model);
        }
        if let Some(source) = source.filter(|value| !value.is_empty() && value != "human") {
            entry.sources.insert(source);
        }
        if let Some(file_name) = file_name.filter(|value| !value.is_empty()) {
            if entry.files.insert(file_name) {
                let ext = extension
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "unknown".to_string());
                *entry.extensions.entry(ext).or_insert(0) += 1;
            }
        }
        if let Some(timestamp) = timestamp {
            entry.first_ms = Some(match entry.first_ms {
                Some(current) => current.min(timestamp),
                None => timestamp,
            });
            entry.last_ms = Some(match entry.last_ms {
                Some(current) => current.max(timestamp),
                None => timestamp,
            });
        }
    }

    Ok(enrichments)
}

pub fn apply_hash_enrichment(
    record: &mut CursorSessionRecord,
    enrichment: &SessionHashEnrichment,
) -> Result<(), String> {
    record.models_json = serde_json::to_string(&enrichment.models.iter().collect::<Vec<_>>())
        .map_err(|e| e.to_string())?;
    record.sources_json = serde_json::to_string(&enrichment.sources.iter().collect::<Vec<_>>())
        .map_err(|e| e.to_string())?;
    record.extensions_json =
        serde_json::to_string(&enrichment.extensions).map_err(|e| e.to_string())?;
    record.files_touched = enrichment.files.len() as i64;
    if let Some(ms) = enrichment.first_ms {
        record.first_seen_at = millis_to_rfc3339(ms);
    }
    if let Some(ms) = enrichment.last_ms {
        record.last_seen_at = millis_to_rfc3339(ms);
    }
    Ok(())
}

fn open_readonly(path: &Path) -> Result<rusqlite::Connection, String> {
    let uri = format!("file:{}?mode=ro", path.to_string_lossy());
    rusqlite::Connection::open_with_flags(
        uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| e.to_string())
}

fn millis_to_rfc3339(millis: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(millis).map(|dt| dt.to_rfc3339())
}
