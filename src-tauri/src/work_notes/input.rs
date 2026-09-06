use std::collections::BTreeMap;
use std::path::Path;

use crate::domain::{
    ConversationEvent, ConversationEventActor, ConversationEventContentStatus,
    ConversationEventKind, ConversationSessionRow,
};

const FIRST_USER_CHARS: usize = 1000;
const MIDDLE_USER_CHARS: usize = 200;
const MIDDLE_USER_MAX: usize = 10;
const LAST_ASSISTANT_CHARS: usize = 1000;
const SESSION_CHAR_CAP: usize = 6000;

pub fn project_dir_name(project: &str) -> String {
    let trimmed = project.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return String::new();
    }
    Path::new(trimmed)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| trimmed.to_string())
}

pub fn is_sparse(event_count: usize, total_tokens: i64) -> bool {
    event_count < 3 || total_tokens < 1000
}

pub fn compress(session: &ConversationSessionRow, events: &[ConversationEvent]) -> String {
    let project = project_dir_name(&session.project);
    let users: Vec<&str> = events
        .iter()
        .filter(|event| {
            event.kind == ConversationEventKind::Message
                && event.actor == Some(ConversationEventActor::User)
        })
        .filter_map(visible_text)
        .collect();
    let last_assistant = events
        .iter()
        .rev()
        .filter(|event| {
            event.kind == ConversationEventKind::Message
                && event.actor == Some(ConversationEventActor::Assistant)
        })
        .find_map(visible_text);
    let tools = tool_counts(events);

    let mut body = String::new();
    body.push_str("标题：");
    body.push_str(&session.title);
    body.push('\n');
    body.push_str("项目：");
    body.push_str(&project);
    body.push('\n');
    if let Some(first) = users.first() {
        body.push_str("首条用户消息：\n");
        body.push_str(&take_chars(first, FIRST_USER_CHARS));
        body.push('\n');
    }
    let middles = users.iter().skip(1).take(MIDDLE_USER_MAX);
    let mut any_middle = false;
    for text in middles {
        if !any_middle {
            body.push_str("中间用户消息：\n");
            any_middle = true;
        }
        body.push_str("- ");
        body.push_str(&take_chars(text, MIDDLE_USER_CHARS));
        body.push('\n');
    }
    if let Some(assistant) = last_assistant {
        body.push_str("末条助手消息：\n");
        body.push_str(&take_chars(assistant, LAST_ASSISTANT_CHARS));
        body.push('\n');
    }
    if !tools.is_empty() {
        body.push_str("工具调用：");
        body.push_str(&tools);
        body.push('\n');
    }
    if body.chars().count() > SESSION_CHAR_CAP {
        take_chars(&body, SESSION_CHAR_CAP)
    } else {
        body
    }
}

fn visible_text(event: &ConversationEvent) -> Option<&str> {
    if event.content_status == ConversationEventContentStatus::Deferred {
        return None;
    }
    event
        .text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn tool_counts(events: &[ConversationEvent]) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut order: Vec<&str> = Vec::new();
    for event in events {
        if event.kind != ConversationEventKind::ToolCall {
            continue;
        }
        let Some(name) = event
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        if !counts.contains_key(name) {
            order.push(name);
        }
        *counts.entry(name).or_insert(0) += 1;
    }
    order
        .into_iter()
        .map(|name| format!("{name}×{}", counts.get(name).copied().unwrap_or(0)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn take_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}
