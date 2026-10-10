use std::fs;
use std::path::Path;

use serde_json::Value;

use super::toolbox::*;
use super::{single_detail, ConversationIndexBatch, ConversationIndexIssue};

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
    let values = parse_jsonl_conversation_values(path)?;
    let path_is_subagent = path
        .components()
        .any(|part| part.as_os_str() == "subagents");
    let file_stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    let meta_project = read_meta_cwd(path);
    let mut session_id = String::new();
    let mut title = String::new();
    let mut project = meta_project;
    let mut model = String::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut messages = Vec::new();
    let mut events = Vec::new();

    for (index, value) in values {
        let timestamp =
            millis_timestamp(value.get("timestamp").and_then(Value::as_i64).unwrap_or(0));
        update_time_bounds(&timestamp, &mut started_at, &mut ended_at);
        if session_id.is_empty() {
            session_id = first_text(&value, &["sessionId"]);
        }
        if project.is_empty() {
            project = first_text(&value, &["cwd"]);
        }
        let provider = value.get("providerData").cloned().unwrap_or(Value::Null);
        let next_model = first_text(&provider, &["model", "requestModelId"]);
        if !next_model.is_empty() {
            model = next_model;
        }
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "message" | "" => {
                let role = first_text(&value, &["role"]);
                if matches!(role.as_str(), "user" | "assistant") {
                    push_projected_message(
                        index,
                        &timestamp,
                        &role,
                        value.get("content").unwrap_or(&Value::Null),
                        value.clone(),
                        &mut messages,
                        &mut events,
                    );
                }
            }
            "function_call" => events.push(semantic_event(
                index,
                EventKind::ToolCall,
                &timestamp,
                Some(EventActor::Assistant),
                optional_text(&value, &["name"]),
                optional_text(&value, &["arguments", "input"]),
                normalize_tool_call_details(&value),
            )),
            "function_call_result" | "function_call_output" => events.push(tool_result_event(
                index,
                &timestamp,
                &normalize_tool_result_details(&value),
                include_deferred_content,
            )),
            "reasoning" => events.push(semantic_event(
                index,
                EventKind::Plan,
                &timestamp,
                Some(EventActor::Assistant),
                None,
                optional_text(&value, &["text", "content"]),
                value.clone(),
            )),
            "ai-title" | "custom-title" => {
                if let Some(next_title) =
                    optional_text(&value, &["aiTitle", "customTitle", "title"])
                {
                    title = next_title.clone();
                    events.push(semantic_event(
                        index,
                        EventKind::SystemStatus,
                        &timestamp,
                        None,
                        Some(kind.to_string()),
                        Some(next_title),
                        value.clone(),
                    ));
                }
            }
            _ => events.push(event_msg_semantic_event(index, &timestamp, kind, &value)),
        }
    }
    if session_id.is_empty() {
        session_id = file_stem;
    }
    let is_top_level = !path_is_subagent;
    if !is_top_level {
        let parent = path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("");
        events.push(semantic_event(
            0,
            EventKind::SystemStatus,
            &started_at,
            None,
            Some("session_started".to_string()),
            None,
            serde_json::json!({ "parent_id": parent }),
        ));
    }
    finish_source_conversation(
        Source::WorkBuddy,
        path,
        session_id,
        title,
        project,
        model,
        started_at,
        ended_at,
        messages,
        events,
        is_top_level,
        ConversationFinishPrep::NONE,
    )
}

fn read_meta_cwd(path: &Path) -> String {
    let Some(stem) = path.file_stem().and_then(|name| name.to_str()) else {
        return String::new();
    };
    let meta = path.with_file_name(format!("{stem}.meta.json"));
    fs::read_to_string(meta)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .map(|value| first_text(&value, &["cwd"]))
        .unwrap_or_default()
}

fn millis_timestamp(ts: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ts)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}
