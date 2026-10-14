use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::toolbox::*;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::ingest;

#[cfg(test)]
#[path = "opencode_test.rs"]
mod tests;

#[path = "opencode_json.rs"]
mod json_store;

#[path = "opencode_snapshot.rs"]
mod snapshot;

#[path = "opencode_project.rs"]
mod project;

#[derive(Debug)]
struct SessionRow {
    id: String,
    title: String,
    directory: String,
    parent_id: String,
    created_at: Option<i64>,
    updated_at: Option<i64>,
}

#[derive(Debug)]
struct MessageRow {
    id: String,
    session_id: String,
    created_at: Option<i64>,
    data: Option<Value>,
}

#[derive(Debug)]
struct PartRow {
    id: String,
    message_id: String,
    session_id: String,
    created_at: Option<i64>,
    data: Option<Value>,
}

struct DatabaseSnapshot {
    sessions: Vec<SessionRow>,
    messages: Vec<MessageRow>,
    parts: Vec<PartRow>,
    capabilities: Vec<String>,
    diagnostics: Vec<ConversationIndexIssue>,
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    if crate::adapters::opencode::is_legacy_session(path) {
        return json_store::index(path);
    }
    let snapshot = snapshot::read_snapshot(path).map_err(|message| fatal_issue(path, message))?;
    let diagnostics = snapshot.diagnostics.clone();
    let conversations = project::project_snapshot(path, snapshot, None, false)
        .map_err(|message| fatal_issue(path, message))?;
    Ok(ConversationIndexBatch {
        conversations,
        diagnostics,
    })
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    if crate::adapters::opencode::is_legacy_session(path) {
        return json_store::detail(path, session_id, include_deferred_content);
    }
    let snapshot = snapshot::read_snapshot(path)?;
    project::project_snapshot(path, snapshot, Some(session_id), include_deferred_content)?
        .into_iter()
        .next()
        .ok_or_else(|| "OpenCode 数据库中未找到该会话".to_string())
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    if crate::adapters::opencode::is_legacy_session(path) {
        fs::metadata(path).map_err(|error| format!("读取 OpenCode 会话文件元数据失败：{error}"))?;
        let main = ingest::metadata_fingerprint(path);
        let extra = crate::adapters::opencode::legacy_sidecar(path);
        return Ok(format!("{main}|{extra}"));
    }
    fs::metadata(path).map_err(|error| format!("读取 OpenCode 数据库元数据失败：{error}"))?;
    let main = ingest::metadata_fingerprint(path);
    let wal_path = sidecar_path(path, "-wal");
    let wal = match fs::metadata(&wal_path) {
        Ok(_) => ingest::metadata_fingerprint(&wal_path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "missing".to_string(),
        Err(error) => return Err(format!("读取 OpenCode WAL 元数据失败：{error}")),
    };
    serde_json::to_string(&(main, wal)).map_err(|error| error.to_string())
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn fatal_issue(path: &Path, message: String) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message,
        event_type: Some("opencode_schema".to_string()),
        line: None,
    }
}

fn diagnostic(path: &Path, message: &str, event_type: &str) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message: message.to_string(),
        event_type: Some(event_type.to_string()),
        line: None,
    }
}
