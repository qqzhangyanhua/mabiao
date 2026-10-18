use std::path::{Path, PathBuf};

use serde_json::Value;

use super::toolbox::*;
use super::{single_detail, ConversationIndexBatch, ConversationIndexIssue};
use crate::adapters::codebuddy_ide::{conversation_messages, source_revision as usage_revision};
use crate::domain::Source;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::codebuddy_ide::discover(roots)
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

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    usage_revision(path)
}

fn parse(path: &Path, _include_deferred_content: bool) -> Result<ParsedConversation, String> {
    let session_id = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    let messages = conversation_messages(path)?;
    let mut title = String::new();
    let mut project = String::new();
    let mut model = String::new();
    let mut started_at = String::new();
    let mut ended_at = String::new();
    let mut projected = Vec::new();
    let mut events = Vec::new();

    for (index, message) in messages.iter().enumerate() {
        update_time_bounds(&message.created_at, &mut started_at, &mut ended_at);
        if project.is_empty() {
            if let Some(folder) = workspace_from_user(&message.role, &message.text) {
                project = folder;
            }
        }
        if let Some(next_model) = message
            .extra
            .get("modelId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            model = next_model.to_string();
        }
        match message.role.as_str() {
            "user" | "assistant" => {
                if message.text.is_empty() {
                    continue;
                }
                if title.is_empty() && message.role == "user" {
                    title = truncate_title(&strip_prompt_wrappers(&message.text));
                }
                push_projected_message(
                    index,
                    &message.created_at,
                    &message.role,
                    &Value::String(message.text.clone()),
                    message.extra.clone(),
                    &mut projected,
                    &mut events,
                );
            }
            "tool" => {
                if message.text.is_empty() {
                    continue;
                }
                events.push(semantic_event(
                    index,
                    EventKind::ToolResult,
                    &message.created_at,
                    Some(EventActor::Tool),
                    None,
                    Some(message.text.clone()),
                    message.extra.clone(),
                ));
            }
            _ => {}
        }
    }

    finish_source_conversation(
        Source::CodeBuddyIde,
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

fn workspace_from_user(role: &str, text: &str) -> Option<String> {
    if role != "user" {
        return None;
    }
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Workspace Folder:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}
