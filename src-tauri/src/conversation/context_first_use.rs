//! 消息流「本轮引入 X」：skill / MCP 工具 / 子代理的首次调用。
//!
//! 触发信号只有「首次出现」一条。明确不做模型切换（`ModelChange`）与上下文
//! 压缩（`auto_compact_*` / `compaction_checkpoint`）：Cursor 会话文件只有
//! user / assistant / turn_ended 三种行，拿不到那些信号。Grok 虽有压缩事件，
//! 也不另开触发源，避免两套故事。
//!
//! 标记挂在详情 DTO 的 `context_manifest.first_uses` 上，不写
//! `conversation_events`，不改适配器版本。

use std::collections::BTreeSet;

use rusqlite::{params, Connection};

use crate::domain::{
    ConversationContextFirstUse, ConversationContextItem, ConversationContextKind,
    ConversationContextLayer, ConversationEvent, ConversationEventKind as EventKind, Source,
};

use super::{context_manifest, event_index};

const SUBAGENT_TOOLS: &[&str] = &["task", "agent", "spawn_agent"];
const COMPACTION_STATUS: &[&str] = &[
    "auto_compact_started",
    "auto_compact_completed",
    "compaction_checkpoint",
];
const SUBAGENT_SPAWNED: &str = "subagent_spawned";
const SUBAGENT_TYPES_ID: &str = "available_subagent_types";

#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub event_id: String,
    pub sequence: u32,
    pub kind: EventKind,
    pub name: Option<String>,
    pub text: Option<String>,
}

impl Candidate {
    pub(crate) fn from_event(event: &ConversationEvent) -> Self {
        let mut text = event.text.clone();
        if text.as_deref().map(str::trim).unwrap_or("").is_empty() {
            if let Some(name) = event.name.as_deref() {
                if let Some(skill) =
                    context_manifest::skill_id_from_call(name, None, Some(&event.details))
                {
                    text = Some(skill);
                }
            }
        }
        Self {
            event_id: event.event_id.clone(),
            sequence: event.sequence,
            kind: event.kind,
            name: event.name.clone(),
            text,
        }
    }
}

pub(crate) fn candidates_from_events(events: &[ConversationEvent]) -> Vec<Candidate> {
    events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::ToolCall | EventKind::SystemStatus))
        .map(Candidate::from_event)
        .collect()
}

pub(crate) fn candidates_from_index(
    conn: &Connection,
    source: Source,
    session_id: &str,
) -> Result<Vec<Candidate>, String> {
    let Some(generation) = event_index::session_generation(conn, source.as_str(), session_id)?
    else {
        return Ok(Vec::new());
    };
    let mut statement = conn
        .prepare(
            r#"
            SELECT event_id, sequence, kind, name, text
            FROM conversation_events
            WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
              AND kind IN ('tool_call', 'system_status')
            ORDER BY sequence
            "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![source.as_str(), session_id, generation], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (event_id, sequence, kind, name, text) = row.map_err(|error| error.to_string())?;
        let kind = match kind.as_str() {
            "tool_call" => EventKind::ToolCall,
            "system_status" => EventKind::SystemStatus,
            _ => continue,
        };
        out.push(Candidate {
            event_id,
            sequence,
            kind,
            name,
            text,
        });
    }
    Ok(out)
}

pub(crate) fn collect(
    candidates: &[Candidate],
    items: &[ConversationContextItem],
) -> Vec<ConversationContextFirstUse> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for candidate in candidates {
        let Some(marker) = classify(candidate, items) else {
            continue;
        };
        let key = dedup_key(&marker);
        if !seen.insert(key) {
            continue;
        }
        out.push(marker);
    }
    out
}

fn classify(
    candidate: &Candidate,
    items: &[ConversationContextItem],
) -> Option<ConversationContextFirstUse> {
    match candidate.kind {
        EventKind::ToolCall => classify_tool(candidate, items),
        EventKind::SystemStatus => classify_status(candidate, items),
        EventKind::Message
        | EventKind::Plan
        | EventKind::ToolResult
        | EventKind::Error
        | EventKind::Unadapted
        | EventKind::ModelChange => None,
    }
}

fn classify_tool(
    candidate: &Candidate,
    items: &[ConversationContextItem],
) -> Option<ConversationContextFirstUse> {
    let name = candidate
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())?;
    if let Some(skill_id) =
        context_manifest::skill_id_from_call(name, candidate.text.as_deref(), None)
    {
        let target = skill_target(&skill_id, items)?;
        return Some(marker(candidate, target, skill_label(target, &skill_id)));
    }
    if is_subagent_tool(name) {
        let label = candidate
            .text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .unwrap_or(name);
        let target = subagent_target(items)?;
        return Some(marker(candidate, target, label.to_string()));
    }
    let server = mcp_server_for_tool(name, items)?;
    Some(marker(candidate, server, name.to_string()))
}

fn classify_status(
    candidate: &Candidate,
    items: &[ConversationContextItem],
) -> Option<ConversationContextFirstUse> {
    let name = candidate
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())?;
    if COMPACTION_STATUS.contains(&name) {
        return None;
    }
    if name != SUBAGENT_SPAWNED {
        return None;
    }
    let label = candidate
        .text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or("子代理");
    let target = subagent_target(items)?;
    Some(marker(candidate, target, label.to_string()))
}

fn marker(
    candidate: &Candidate,
    item: &ConversationContextItem,
    label: String,
) -> ConversationContextFirstUse {
    ConversationContextFirstUse {
        event_id: candidate.event_id.clone(),
        sequence: candidate.sequence,
        item_id: item.id.clone(),
        item_kind: item.kind,
        item_layer: item.layer,
        label,
    }
}

fn dedup_key(marker: &ConversationContextFirstUse) -> String {
    format!(
        "{}:{}:{}",
        marker.item_kind.as_str(),
        marker.item_id,
        marker.label
    )
}

fn skill_label(item: &ConversationContextItem, skill_id: &str) -> String {
    if !item.label.is_empty() {
        item.label.clone()
    } else {
        skill_id.to_string()
    }
}

fn skill_target<'a>(
    skill_id: &str,
    items: &'a [ConversationContextItem],
) -> Option<&'a ConversationContextItem> {
    let matches = |item: &&ConversationContextItem| {
        item.kind == ConversationContextKind::Skill && skill_item_matches(item, skill_id)
    };
    items
        .iter()
        .find(|item| item.layer == ConversationContextLayer::Injected && matches(item))
        .or_else(|| {
            items
                .iter()
                .find(|item| item.layer == ConversationContextLayer::Observed && matches(item))
        })
}

fn skill_item_matches(item: &ConversationContextItem, skill_id: &str) -> bool {
    item.id == skill_id
        || item.label == skill_id
        || item
            .path
            .as_deref()
            .is_some_and(|path| path_matches_skill(path, skill_id))
        || path_matches_skill(&item.id, skill_id)
}

fn path_matches_skill(path: &str, skill_id: &str) -> bool {
    let path = path.replace('\\', "/");
    let needle = skill_id.replace('\\', "/");
    path == needle
        || path.ends_with(&format!("/{needle}"))
        || path.ends_with(&format!("/{needle}/SKILL.md"))
        || path.split('/').any(|part| part == needle)
}

fn mcp_server_for_tool<'a>(
    tool: &str,
    items: &'a [ConversationContextItem],
) -> Option<&'a ConversationContextItem> {
    let mut fallback = None;
    for item in items {
        if item.kind != ConversationContextKind::McpServer || !server_owns_tool(item, tool) {
            continue;
        }
        if item.layer == ConversationContextLayer::Injected {
            return Some(item);
        }
        if fallback.is_none() {
            fallback = Some(item);
        }
    }
    fallback
}

fn server_owns_tool(server: &ConversationContextItem, tool: &str) -> bool {
    if server.id == tool || server.label == tool {
        return true;
    }
    if tool.starts_with(&format!("{}__", server.id))
        || tool.starts_with(&format!("{}__", server.label))
    {
        return true;
    }
    mcp_tools(server).iter().any(|name| name == tool)
}

fn mcp_tools(item: &ConversationContextItem) -> Vec<String> {
    item.meta
        .as_ref()
        .and_then(|meta| meta.get("tools"))
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(str::to_string)
        .collect()
}

fn subagent_target(items: &[ConversationContextItem]) -> Option<&ConversationContextItem> {
    items
        .iter()
        .find(|item| {
            item.layer == ConversationContextLayer::Injected && item.id == SUBAGENT_TYPES_ID
        })
        .or_else(|| {
            items.iter().find(|item| {
                item.layer == ConversationContextLayer::Observed
                    && item.kind == ConversationContextKind::SystemStatus
                    && item.id == SUBAGENT_SPAWNED
            })
        })
        .or_else(|| {
            items.iter().find(|item| {
                item.layer == ConversationContextLayer::Observed
                    && item.kind == ConversationContextKind::Tool
                    && (is_subagent_tool(&item.id) || is_subagent_tool(&item.label))
            })
        })
}

fn is_subagent_tool(name: &str) -> bool {
    SUBAGENT_TOOLS
        .iter()
        .any(|tool| name.eq_ignore_ascii_case(tool))
}
