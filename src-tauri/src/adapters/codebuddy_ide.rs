use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{finish, has_billable_tokens};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

const PATH_ENV: &str = "CODEBUDDY_IDE_DIR";
const MAX_WALK_DEPTH: usize = 12;
const WORKSPACE_FOLDER_PREFIX: &str = "Workspace Folder:";

/// CodeBuddy IDE 把每条消息写成独立 JSON，用量在助手消息 `extra`（字符串）里的累计 `statsSnapshot`。
///
/// `$CODEBUDDY_IDE_DIR` 覆盖 Data 根。未覆盖时扫 Electron / VS Code 风格的
/// `CodeBuddyExtension/Data`（Linux `.config`、macOS Application Support、Windows Roaming 与 Local）。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides.get(PATH_ENV).cloned().unwrap_or_else(|| {
        vec![
            home.join(".config/CodeBuddyExtension/Data"),
            home.join("Library/Application Support/CodeBuddyExtension/Data"),
            home.join("AppData/Roaming/CodeBuddyExtension/Data"),
            home.join("AppData/Local/CodeBuddyExtension/Data"),
        ]
    })
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut conversations = Vec::new();
    for root in roots {
        find_conversations(root, 0, &mut conversations)?;
    }
    conversations.sort();
    conversations.dedup();
    Ok(conversations)
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    fingerprint_messages(path)
}

pub fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    Ok(parse_conversation(path)?.into_iter().collect())
}

pub(crate) fn source_revision(path: &Path) -> Result<String, String> {
    Ok(fingerprint_messages(path))
}

pub(crate) fn messages_dir(conversation: &Path) -> PathBuf {
    if conversation.file_name().and_then(|name| name.to_str()) == Some("messages") {
        conversation.to_path_buf()
    } else {
        conversation.join("messages")
    }
}

fn find_conversations(root: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > MAX_WALK_DEPTH || !root.exists() {
        return Ok(());
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            return Err(format!("扫描目录 {} 失败：{error}", root.display()));
        }
    };
    for entry in entries {
        let path = entry
            .map_err(|error| format!("扫描目录 {} 失败：{error}", root.display()))?
            .path();
        let file_type = fs::symlink_metadata(&path)
            .map_err(|error| format!("扫描目录 {} 失败：{error}", path.display()))?
            .file_type();
        if file_type.is_symlink() {
            continue;
        }
        if !file_type.is_dir() {
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some("messages") {
            if message_json_files(&path)?.next().is_some() {
                if let Some(conversation) = path.parent() {
                    out.push(conversation.to_path_buf());
                }
            }
            continue;
        }
        find_conversations(&path, depth + 1, out)?;
    }
    Ok(())
}

fn fingerprint_messages(conversation: &Path) -> String {
    let Ok(mut files) = message_json_files(&messages_dir(conversation)) else {
        return String::new();
    };
    let mut parts: Vec<String> = files
        .by_ref()
        .map(|path| ingest::metadata_fingerprint(&path))
        .collect();
    parts.sort();
    parts.join("|")
}

fn message_json_files(dir: &Path) -> Result<impl Iterator<Item = PathBuf>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new().into_iter());
    }
    let mut files = Vec::new();
    let entries =
        fs::read_dir(dir).map_err(|error| format!("读取 {} 失败：{error}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|error| format!("读取 {} 失败：{error}", dir.display()))?
            .path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") && path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files.into_iter())
}

struct ParsedMessage {
    role: String,
    message: String,
    extra: Value,
    created_at: String,
}

fn parse_conversation(path: &Path) -> Result<Option<UsageRecord>, String> {
    let messages = read_messages(path)?;
    let Some(latest) = latest_usage(&messages) else {
        return Ok(None);
    };
    let extra = &latest.extra;
    let snapshot = extra.get("statsSnapshot").cloned().unwrap_or(Value::Null);
    let cache_read = first_number(
        &snapshot,
        &["cachedInputTokens"],
        extra,
        &["lastStepCachedInputTokens"],
    );
    let input = if snapshot.get("cacheMissTokens").is_some() {
        json_number(&snapshot, "cacheMissTokens")
    } else {
        (first_number(&snapshot, &[], extra, &["lastStepInputTokens"]) - cache_read).max(0)
    };
    let output = first_number(
        &snapshot,
        &["lastOutputTokens"],
        extra,
        &["lastStepOutputTokens"],
    );
    let cache_write = json_number(&snapshot, "cacheWriteTokens");
    let reasoning = json_number(&snapshot, "thinkingTokens");
    let model = extra
        .get("modelId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or("")
        .to_string();
    if model.is_empty() {
        return Ok(None);
    }
    let session_id = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    let record = finish(UsageRecord {
        occurred_at: parse_iso_timestamp(&latest.created_at),
        source: Source::CodeBuddyIde,
        model,
        provider: String::new(),
        project: workspace_folder(&messages).unwrap_or_default(),
        session_id,
        source_file: path.to_string_lossy().into_owned(),
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_write,
        cache_creation_1h_tokens: 0,
        reasoning_tokens: reasoning,
        total_tokens: 0,
        // statsSnapshot.credit 是 CodeBuddy 积分，不是 USD。
        native_cost: None,
    });
    Ok(has_billable_tokens(&record).then_some(record))
}

fn read_messages(conversation: &Path) -> Result<Vec<ParsedMessage>, String> {
    let mut parsed = Vec::new();
    for path in message_json_files(&messages_dir(conversation))? {
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        let role = value
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let extra = match value.get("extra").and_then(Value::as_str) {
            Some(raw) => serde_json::from_str(raw).unwrap_or(Value::Null),
            None => value.get("extra").cloned().unwrap_or(Value::Null),
        };
        let created_at = value
            .get("createdAt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        parsed.push(ParsedMessage {
            role,
            message,
            extra,
            created_at,
        });
    }
    Ok(parsed)
}

fn latest_usage(messages: &[ParsedMessage]) -> Option<&ParsedMessage> {
    messages
        .iter()
        .filter(|message| message.role == "assistant")
        .filter(|message| {
            message.extra.get("statsSnapshot").is_some()
                || message.extra.get("lastStepInputTokens").is_some()
        })
        .max_by_key(|message| parse_iso_timestamp(&message.created_at))
}

fn workspace_folder(messages: &[ParsedMessage]) -> Option<String> {
    for message in messages {
        if message.role != "user" {
            continue;
        }
        let text = user_message_text(&message.message);
        if let Some(folder) = text.lines().find_map(|line| {
            line.trim()
                .strip_prefix(WORKSPACE_FOLDER_PREFIX)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        }) {
            return Some(folder);
        }
    }
    None
}

pub(crate) fn user_message_text(raw: &str) -> String {
    if let Ok(inner) = serde_json::from_str::<Value>(raw) {
        if let Some(content) = inner.get("content") {
            let mut parts = Vec::new();
            if let Some(blocks) = content.as_array() {
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) == Some("text") {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            parts.push(text.to_string());
                        }
                    }
                }
            } else if let Some(text) = content.as_str() {
                parts.push(text.to_string());
            }
            if !parts.is_empty() {
                return parts.join("\n");
            }
        }
        if let Some(text) = inner.get("text").and_then(Value::as_str) {
            return text.to_string();
        }
    }
    raw.to_string()
}

pub(crate) fn parse_iso_timestamp(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|_| raw.to_string())
}

fn first_number(
    primary: &Value,
    primary_keys: &[&str],
    fallback: &Value,
    fallback_keys: &[&str],
) -> i64 {
    for key in primary_keys {
        if primary.get(*key).is_some() {
            return json_number(primary, key);
        }
    }
    for key in fallback_keys {
        if fallback.get(*key).is_some() {
            return json_number(fallback, key);
        }
    }
    0
}

fn json_number(value: &Value, key: &str) -> i64 {
    match value.get(key) {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|n| n as i64))
            .unwrap_or(0)
            .max(0),
        Some(Value::String(text)) => text
            .parse::<f64>()
            .ok()
            .map(|n| n as i64)
            .unwrap_or(0)
            .max(0),
        _ => 0,
    }
}

pub(crate) struct MessageView {
    pub role: String,
    pub text: String,
    pub extra: Value,
    pub created_at: String,
}

pub(crate) fn conversation_messages(path: &Path) -> Result<Vec<MessageView>, String> {
    Ok(read_messages(path)?
        .into_iter()
        .map(|message| MessageView {
            text: if message.role == "user" {
                user_message_text(&message.message)
            } else {
                assistant_text(&message.message)
            },
            role: message.role,
            extra: message.extra,
            created_at: parse_iso_timestamp(&message.created_at),
        })
        .collect())
}

fn assistant_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return String::new();
    }
    if let Ok(inner) = serde_json::from_str::<Value>(trimmed) {
        if let Some(content) = inner.get("content") {
            let mut parts = Vec::new();
            if let Some(blocks) = content.as_array() {
                for block in blocks {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            parts.push(text.to_string());
                        }
                    }
                }
            } else if let Some(text) = content.as_str() {
                parts.push(text.to_string());
            }
            if !parts.is_empty() {
                return parts.join("\n");
            }
        }
        if let Some(text) = inner.get("text").and_then(Value::as_str) {
            return text.to_string();
        }
    }
    trimmed.to_string()
}
