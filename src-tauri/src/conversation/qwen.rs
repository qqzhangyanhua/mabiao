use std::collections::BTreeMap;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::toolbox::*;
use super::{discover_extension, ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::project::project_from_source_file;

#[cfg(test)]
#[path = "qwen_test.rs"]
mod tests;

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    let (conversations, diagnostics) =
        parse_all(path, false).map_err(|message| ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message,
            event_type: Some("qwen_log".to_string()),
            line: None,
        })?;
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
    let (conversations, _) = parse_all(path, include_deferred_content)?;
    conversations
        .into_iter()
        .find(|conversation| conversation.session.session_id == session_id)
        .ok_or_else(|| "原始文件中的会话 ID 与索引不一致".to_string())
}

pub(super) fn export_session_records(path: &Path, session_id: &str) -> Result<Vec<u8>, String> {
    let selected = if is_jsonl(path) {
        parse_jsonl_conversation_values(path)?
            .into_iter()
            .map(|(_, record)| record)
            .filter(|record| record_session_id(record).as_deref() == Some(session_id))
            .collect::<Vec<_>>()
    } else {
        let file =
            fs::File::open(path).map_err(|error| format!("读取 Qwen logs.json 失败：{error}"))?;
        let value: Value = serde_json::from_reader(BufReader::new(file))
            .map_err(|error| format!("Qwen logs.json 无效：{error}"))?;
        let records = value
            .as_array()
            .ok_or_else(|| "Qwen logs.json 顶层必须是数组".to_string())?;
        records
            .iter()
            .filter(|record| record.get("sessionId").and_then(Value::as_str) == Some(session_id))
            .cloned()
            .collect::<Vec<_>>()
    };
    if selected.is_empty() {
        return Err("原始文件中的会话 ID 与索引不一致".to_string());
    }
    serde_json::to_vec_pretty(&selected).map_err(|error| error.to_string())
}

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = discover_extension(roots, "json")?
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some("logs.json"))
        .collect::<Vec<_>>();
    paths.extend(
        discover_extension(roots, "jsonl")?
            .into_iter()
            .filter(|path| crate::adapters::qwen::is_chat_jsonl(path)),
    );
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn parse_all(
    path: &Path,
    _include_deferred_content: bool,
) -> Result<(Vec<ParsedConversation>, Vec<ConversationIndexIssue>), String> {
    if is_jsonl(path) {
        return parse_chat_jsonl(path);
    }
    // logs.json 是单个 Qwen 会话目录下所有 session 累积写入的一份大数组，流式反序列化
    // 避免同时攥着原始文本和解析后的 Value 树两份内存。
    let file =
        fs::File::open(path).map_err(|error| format!("读取 Qwen logs.json 失败：{error}"))?;
    let value: Value = serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("Qwen logs.json 无效：{error}"))?;
    let records = value
        .as_array()
        .ok_or_else(|| "Qwen logs.json 顶层必须是数组".to_string())?;
    if records.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut grouped = BTreeMap::<String, Vec<(usize, Value)>>::new();
    let mut diagnostics = Vec::new();
    for (line, record) in records.iter().enumerate() {
        let Some(session_id) = record
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|session_id| !session_id.is_empty())
        else {
            diagnostics.push(issue(path, line, "记录缺少 sessionId"));
            continue;
        };
        grouped
            .entry(session_id.to_string())
            .or_default()
            .push((line, record.clone()));
    }
    if grouped.is_empty() {
        return Err("Qwen logs.json 缺少必需的会话 ID".to_string());
    }

    let project = project_from_source_file(&path.to_string_lossy());
    let mut conversations = Vec::new();
    for (session_id, records) in grouped {
        conversations.push(finish_logs_session(
            path,
            session_id,
            project.clone(),
            records,
            &mut diagnostics,
        )?);
    }
    Ok((conversations, diagnostics))
}

fn parse_chat_jsonl(
    path: &Path,
) -> Result<(Vec<ParsedConversation>, Vec<ConversationIndexIssue>), String> {
    let records = parse_jsonl_conversation_values(path)?;
    if records.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let fallback_session = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let mut grouped = BTreeMap::<String, Vec<(usize, Value)>>::new();
    let mut diagnostics = Vec::new();
    for (line, record) in records {
        let session_id = record_session_id(&record)
            .filter(|session_id| !session_id.is_empty())
            .unwrap_or_else(|| fallback_session.clone());
        if session_id.is_empty() {
            diagnostics.push(issue(path, line, "记录缺少 sessionId"));
            continue;
        }
        grouped.entry(session_id).or_default().push((line, record));
    }
    if grouped.is_empty() {
        return Err("Qwen chats jsonl 缺少必需的会话 ID".to_string());
    }
    let project = crate::adapters::project::decode_url_dir(
        path.parent()
            .and_then(Path::parent)
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or_default(),
    );
    let mut conversations = Vec::new();
    for (session_id, records) in grouped {
        conversations.push(finish_chat_session(
            path,
            session_id,
            project.clone(),
            records,
            &mut diagnostics,
        )?);
    }
    Ok((conversations, diagnostics))
}

fn finish_logs_session(
    path: &Path,
    session_id: String,
    project: String,
    records: Vec<(usize, Value)>,
    diagnostics: &mut Vec<ConversationIndexIssue>,
) -> Result<ParsedConversation, String> {
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut sequence = 0usize;
    for (line, record) in records {
        let event_start = events.len();
        let raw_kind = record.get("type").and_then(Value::as_str).unwrap_or("");
        let occurred_at = record
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        update_time_bounds(&occurred_at, &mut started_at, &mut ended_at);
        if raw_kind == "user"
            && !content_text(record.get("message").unwrap_or(&Value::Null)).is_empty()
        {
            push_projected_message(
                sequence,
                &occurred_at,
                "user",
                record.get("message").unwrap_or(&Value::Null),
                structural_details(line, raw_kind, &record),
                &mut messages,
                &mut events,
            );
        } else {
            events.push(unadapted_event(
                sequence,
                &occurred_at,
                raw_kind,
                record.clone(),
            ));
            diagnostics.push(issue(
                path,
                line,
                &format!("事件类型 {} 尚未适配", display_kind(raw_kind)),
            ));
        }
        let native_identity = message_identity(&record).map(|id| format!("{raw_kind}:{id}"));
        tag_source_events(&mut events[event_start..], line, native_identity.as_deref());
        sequence += 1;
    }

    let mut missing = vec!["assistant_message", "model", "provider", "usage"];
    if events.iter().any(|event| event.occurred_at.is_none()) {
        missing.push("timestamp");
    }
    let mut parsed = finish_source_conversation(
        Source::Qwen,
        path,
        session_id,
        String::new(),
        project,
        String::new(),
        started_at,
        ended_at,
        messages,
        events,
        true,
        ConversationFinishPrep {
            native_ids: NativeEventIdPrep::Assign,
            degradation: CapabilityDegradationPrep::Declared {
                sequence,
                missing: &missing,
            },
        },
    )?;
    parsed
        .session
        .capabilities
        .retain(|capability| capability != CAPABILITY_USAGE);
    Ok(parsed)
}

fn finish_chat_session(
    path: &Path,
    session_id: String,
    project: String,
    records: Vec<(usize, Value)>,
    diagnostics: &mut Vec<ConversationIndexIssue>,
) -> Result<ParsedConversation, String> {
    let mut messages = Vec::new();
    let mut events = Vec::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut model = String::new();
    let mut has_usage = false;
    let mut sequence = 0usize;
    for (line, record) in records {
        let event_start = events.len();
        let raw_kind = record.get("type").and_then(Value::as_str).unwrap_or("");
        let occurred_at = record
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        update_time_bounds(&occurred_at, &mut started_at, &mut ended_at);
        let next_model = first_text(&record, &["model"]);
        if !next_model.is_empty() {
            model = next_model;
        }
        if record.get("usageMetadata").is_some_and(Value::is_object) {
            has_usage = true;
        }
        let content = record
            .get("message")
            .or_else(|| record.get("content"))
            .cloned()
            .unwrap_or(Value::Null);
        let role = match raw_kind {
            "user" => Some("user"),
            "assistant" | "gemini" | "model" => Some("assistant"),
            _ => None,
        };
        if let Some(role) = role {
            if content_text(&content).is_empty() {
                events.push(unadapted_event(
                    sequence,
                    &occurred_at,
                    raw_kind,
                    record.clone(),
                ));
            } else {
                push_projected_message(
                    sequence,
                    &occurred_at,
                    role,
                    &content,
                    structural_details(line, raw_kind, &record),
                    &mut messages,
                    &mut events,
                );
            }
        } else {
            events.push(unadapted_event(
                sequence,
                &occurred_at,
                raw_kind,
                record.clone(),
            ));
            diagnostics.push(issue(
                path,
                line,
                &format!("事件类型 {} 尚未适配", display_kind(raw_kind)),
            ));
        }
        let native_identity = message_identity(&record).map(|id| format!("{raw_kind}:{id}"));
        tag_source_events(&mut events[event_start..], line, native_identity.as_deref());
        sequence += 1;
    }

    let mut missing = Vec::new();
    if !messages.iter().any(|message| message.role == "assistant") {
        missing.push("assistant_message");
    }
    if model.is_empty() {
        missing.push("model");
    }
    missing.push("provider");
    if !has_usage {
        missing.push("usage");
    }
    if events.iter().any(|event| event.occurred_at.is_none()) {
        missing.push("timestamp");
    }
    let mut parsed = finish_source_conversation(
        Source::Qwen,
        path,
        session_id,
        String::new(),
        project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        true,
        ConversationFinishPrep {
            native_ids: NativeEventIdPrep::Assign,
            degradation: CapabilityDegradationPrep::Declared {
                sequence,
                missing: &missing,
            },
        },
    )?;
    if !has_usage {
        parsed
            .session
            .capabilities
            .retain(|capability| capability != CAPABILITY_USAGE);
    }
    Ok(parsed)
}

fn is_jsonl(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
}

fn record_session_id(record: &Value) -> Option<String> {
    record
        .get("sessionId")
        .or_else(|| record.get("session_id"))
        .and_then(Value::as_str)
        .filter(|session_id| !session_id.is_empty())
        .map(str::to_string)
}

fn message_identity(record: &Value) -> Option<String> {
    let value = record.get("messageId")?;
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_u64().map(|number| number.to_string()))
}

fn structural_details(line: usize, kind: &str, record: &Value) -> Value {
    json!({
        "line": line + 1,
        "type": kind,
        "message_id": record.get("messageId"),
    })
}

fn issue(path: &Path, line: usize, message: &str) -> ConversationIndexIssue {
    ConversationIndexIssue {
        path: path.to_string_lossy().to_string(),
        message: format!("Qwen log 第 {} 条记录{message}", line + 1),
        event_type: Some("qwen_log_event".to_string()),
        line: Some((line + 1) as u64),
    }
}

fn display_kind(kind: &str) -> &str {
    if kind.is_empty() {
        "<missing>"
    } else {
        kind
    }
}
