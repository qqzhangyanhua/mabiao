use std::path::{Path, PathBuf};

use super::metadata_revision;
use crate::ingest;
use std::fs;

use super::toolbox::ParsedConversation;
use super::{claude, gemini, omp, pi, ConversationIndexBatch, ConversationIndexIssue};

pub(crate) fn discover_extension(
    roots: &[PathBuf],
    extension: &str,
) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        paths.extend(ingest::walk_files(root, extension)?);
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub(crate) fn discover_jsonl(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    discover_extension(roots, "jsonl")
}

pub(crate) fn discover_dsh(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(discover_extension(roots, "zstd")?
        .into_iter()
        .filter(|path| {
            path.file_name().and_then(|name| name.to_str()) == Some("session.jsonl.zstd")
        })
        .collect())
}

pub(crate) fn discover_droid(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
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

pub(crate) fn discover_gemini(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(discover_extension(roots, "json")?
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("session-"))
        })
        .collect())
}

pub(crate) fn discover_opencode(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    Ok(roots
        .iter()
        .filter(|path| path.is_file())
        .cloned()
        .collect())
}

pub(crate) fn regular_source_revision(path: &Path) -> Result<String, String> {
    fs::metadata(path)
        .map(|metadata| metadata_revision(&metadata))
        .map_err(|error| format!("读取原始文件元数据失败：{error}"))
}

pub(crate) fn single_index(
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

pub(crate) fn single_detail(
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

pub(crate) type DiagnosticParseFn =
    fn(&Path, bool) -> Result<(ParsedConversation, Vec<ConversationIndexIssue>), String>;

pub(crate) fn diagnostic_index(
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

pub(crate) fn diagnostic_detail(
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

pub(crate) fn index_claude(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    single_index(path, claude::parse)
}

pub(crate) fn detail_claude(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, claude::parse)
}

pub(crate) fn index_pi(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    single_index(path, pi::parse)
}

pub(crate) fn detail_pi(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, pi::parse)
}

pub(crate) fn index_omp(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    single_index(path, omp::parse)
}

pub(crate) fn detail_omp(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, omp::parse)
}

pub(crate) fn index_gemini(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    single_index(path, gemini::parse)
}

pub(crate) fn detail_gemini(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, gemini::parse)
}
