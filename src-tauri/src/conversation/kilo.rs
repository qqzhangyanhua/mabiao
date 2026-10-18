use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::toolbox::*;
use super::{single_detail, ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::extension_tasks::{
    content_blob, extract_tags, project_from_sidecars, API_HISTORY, HISTORY_ITEM,
};
use crate::domain::Source;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::discover_suffix(roots, API_HISTORY)
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
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

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, parse)
}

pub(super) fn parse(
    path: &Path,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let root: Value = serde_json::from_str(
        fs::read_to_string(path)
            .map_err(|error| format!("读取原始文件失败：{error}"))?
            .trim_start_matches('\u{feff}'),
    )
    .map_err(|error| format!("JSON 无效：{error}"))?;
    let messages = root
        .as_array()
        .ok_or_else(|| "缺少 Kilo Code api_conversation_history 数组".to_string())?;
    let history_item = path
        .parent()
        .and_then(|dir| fs::read_to_string(dir.join(HISTORY_ITEM)).ok())
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok());
    let session_id = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    let mut title = history_item
        .as_ref()
        .map(|value| first_text(value, &["task"]))
        .unwrap_or_default();
    let project = project_from_sidecars(history_item.as_ref(), Some(&root));
    let mut model = String::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut projected = Vec::new();
    let mut events = Vec::new();

    for (index, message) in messages.iter().enumerate() {
        let ts = message.get("ts").and_then(Value::as_i64).unwrap_or(0);
        let timestamp = millis_timestamp(ts);
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        let role = first_text(message, &["role"]);
        let content = message.get("content").cloned().unwrap_or(Value::Null);
        let mut visible = strip_environment_details(&content_blob(&content));
        if visible.trim().is_empty() {
            visible = String::new();
        }
        if role == "user" {
            if let Some(next) = extract_tags(&content_blob(&content), "model")
                .into_iter()
                .last()
            {
                if !next.is_empty() && next != model {
                    model = next.clone();
                    events.push(semantic_event(
                        index,
                        EventKind::ModelChange,
                        &timestamp,
                        None,
                        Some(next),
                        None,
                        message.clone(),
                    ));
                }
            }
        }
        if matches!(role.as_str(), "user" | "assistant") && !visible.is_empty() {
            push_projected_message(
                index,
                &timestamp,
                &role,
                &Value::String(visible),
                message.clone(),
                &mut projected,
                &mut events,
            );
        }
        if let Some(items) = content.as_array() {
            for item in items {
                match item.get("type").and_then(Value::as_str).unwrap_or("") {
                    "tool_use" | "tool_call" => events.push(semantic_event(
                        index,
                        EventKind::ToolCall,
                        &timestamp,
                        Some(EventActor::Assistant),
                        optional_text(item, &["name"]),
                        item.get("input").map(Value::to_string),
                        normalize_tool_call_details(item),
                    )),
                    "tool_result" => events.push(tool_result_event(
                        index,
                        &timestamp,
                        &normalize_tool_result_details(item),
                        include_deferred_content,
                    )),
                    _ => {}
                }
            }
        }
    }
    if title.is_empty() {
        title = projected
            .iter()
            .find(|message| message.role == "user")
            .map(|message| message.text.clone())
            .unwrap_or_default();
    }
    finish_source_conversation(
        Source::Kilo,
        path,
        session_id,
        title,
        project,
        model,
        started_at,
        ended_at,
        projected,
        events,
        true,
        ConversationFinishPrep {
            native_ids: NativeEventIdPrep::Assign,
            degradation: CapabilityDegradationPrep::Inferred {
                sequence: messages.len(),
            },
        },
    )
}

fn millis_timestamp(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    let millis = if ts > 1_000_000_000_000 {
        ts
    } else {
        ts * 1000
    };
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn strip_environment_details(text: &str) -> String {
    let open = "<environment_details>";
    let close = "</environment_details>";
    let mut remaining = text;
    let mut stripped = String::new();
    while let Some(start) = remaining.find(open) {
        stripped.push_str(&remaining[..start]);
        let after = &remaining[start + open.len()..];
        match after.find(close) {
            Some(end) => remaining = &after[end + close.len()..],
            None => {
                remaining = "";
                break;
            }
        }
    }
    stripped.push_str(remaining);
    stripped.trim().to_string()
}
