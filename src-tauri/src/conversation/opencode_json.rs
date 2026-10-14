use std::fs;
use std::path::Path;

use serde_json::Value;

use super::super::toolbox::{CAPABILITY_EVENTS, CAPABILITY_MESSAGES, CAPABILITY_USAGE};
use super::project::project_snapshot;
use super::{
    diagnostic, fatal_issue, ConversationIndexBatch, ConversationIndexIssue, DatabaseSnapshot,
    MessageRow, ParsedConversation, PartRow, SessionRow,
};

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    match load(path, None, false) {
        Ok((conversations, diagnostics)) => Ok(ConversationIndexBatch {
            conversations,
            diagnostics,
        }),
        Err(message) => Err(fatal_issue(path, message)),
    }
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    load(path, Some(session_id), include_deferred_content)?
        .0
        .into_iter()
        .next()
        .ok_or_else(|| "OpenCode 会话文件中未找到该会话".to_string())
}

fn load(
    path: &Path,
    session_filter: Option<&str>,
    include_deferred_content: bool,
) -> Result<(Vec<ParsedConversation>, Vec<ConversationIndexIssue>), String> {
    let session = read_session(path)?;
    let mut diagnostics = Vec::new();
    let messages = read_messages(path, &session.id, &mut diagnostics)?;
    let parts = read_parts(path, &messages, &mut diagnostics)?;
    let snapshot = DatabaseSnapshot {
        sessions: vec![session],
        messages,
        parts,
        capabilities: vec![
            CAPABILITY_MESSAGES.to_string(),
            CAPABILITY_EVENTS.to_string(),
            CAPABILITY_USAGE.to_string(),
        ],
        diagnostics: diagnostics.clone(),
    };
    let conversations = project_snapshot(path, snapshot, session_filter, include_deferred_content)?;
    Ok((conversations, diagnostics))
}

fn read_session(path: &Path) -> Result<SessionRow, String> {
    let data = read_json(path)?;
    let id = text(&data, &["id"]);
    if id.is_empty() {
        return Err("OpenCode 会话文件缺少 id".to_string());
    }
    let time = data.get("time").cloned().unwrap_or(Value::Null);
    Ok(SessionRow {
        id,
        title: text(&data, &["title"]),
        directory: text(&data, &["directory"]),
        parent_id: text(&data, &["parentID", "parent_id"]),
        created_at: time.get("created").and_then(Value::as_i64),
        updated_at: time.get("updated").and_then(Value::as_i64),
    })
}

fn read_messages(
    path: &Path,
    session_id: &str,
    diagnostics: &mut Vec<ConversationIndexIssue>,
) -> Result<Vec<MessageRow>, String> {
    let Some(storage) = legacy_storage(path) else {
        return Err("OpenCode 旧会话不在 storage/session 下".to_string());
    };
    let dir = storage.join("message").join(session_id);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = json_files(&dir)?;
    files.sort();
    let mut messages = Vec::new();
    for file in files {
        let id = file
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        let data = match read_json(&file) {
            Ok(data) => Some(data),
            Err(error) => {
                diagnostics.push(diagnostic(
                    path,
                    &format!("OpenCode 消息文件 {} 无效：{error}", file.display()),
                    "message_json",
                ));
                None
            }
        };
        let created_at = data
            .as_ref()
            .and_then(|data| data.get("time"))
            .and_then(|time| time.get("created"))
            .and_then(Value::as_i64);
        messages.push(MessageRow {
            id,
            session_id: session_id.to_string(),
            created_at,
            data,
        });
    }
    Ok(messages)
}

fn read_parts(
    path: &Path,
    messages: &[MessageRow],
    diagnostics: &mut Vec<ConversationIndexIssue>,
) -> Result<Vec<PartRow>, String> {
    let Some(storage) = legacy_storage(path) else {
        return Ok(Vec::new());
    };
    let mut parts = Vec::new();
    for message in messages {
        let dir = storage.join("part").join(&message.id);
        if !dir.exists() {
            continue;
        }
        let mut files = json_files(&dir)?;
        files.sort();
        for file in files {
            let id = file
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            let data = match read_json(&file) {
                Ok(data) => Some(data),
                Err(error) => {
                    diagnostics.push(diagnostic(
                        path,
                        &format!("OpenCode part 文件 {} 无效：{error}", file.display()),
                        "part_json",
                    ));
                    None
                }
            };
            let created_at = data
                .as_ref()
                .and_then(|data| data.get("time"))
                .and_then(|time| time.get("created"))
                .and_then(Value::as_i64)
                .or(message.created_at);
            parts.push(PartRow {
                id,
                message_id: message.id.clone(),
                session_id: message.session_id.clone(),
                created_at,
                data,
            });
        }
    }
    Ok(parts)
}

fn json_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut files = Vec::new();
    for entry in
        fs::read_dir(dir).map_err(|error| format!("读取 OpenCode 旧会话目录失败：{error}"))?
    {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
    }
    Ok(files)
}

fn legacy_storage(path: &Path) -> Option<std::path::PathBuf> {
    path.parent()?.parent()?.parent().map(Path::to_path_buf)
}

fn read_json(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw)
        .map_err(|error| format!("line {}, column {}", error.line(), error.column()))
}

fn text(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}
