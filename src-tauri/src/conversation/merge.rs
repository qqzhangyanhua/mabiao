//! 同一会话跨文件的合并语义，以及目录索引用的 agent 摘要类型。
//!
//! 目录行、解析结果、索引文件三组口径都在这里；不碰 sqlite、路径校验或附件。

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::domain::{
    ConversationEvent, ConversationEventKind as EventKind, ConversationSessionRow,
};

use super::toolbox::{
    compare_event_timestamps, compare_optional_timestamps, compare_timestamps, ParsedConversation,
    CAPABILITY_EVENTS, CAPABILITY_MESSAGES, CAPABILITY_USAGE,
};

/// 一个源文件在目录索引里的全部有效信息。见 `IndexedAgentEvent` 的说明。
pub(crate) struct IndexedFile {
    pub(crate) session: ConversationSessionRow,
    is_top_level: bool,
    agent_events: Vec<IndexedAgentEvent>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct IndexedAgentMetadata {
    pub(crate) parent_session_ids: Vec<String>,
    pub(crate) spawn_attempts: Vec<IndexedSpawnAttempt>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct IndexedSpawnAttempt {
    pub(crate) launch_event_id: String,
    pub(crate) child_session_id: Option<String>,
}

/// 把同一会话散落在多个文件里的会话行合成一行。`rows` 必须已按 `source_file` 排序。
pub(crate) fn merge_session_rows(rows: &[ConversationSessionRow]) -> ConversationSessionRow {
    let mut session = rows[0].clone();
    session.started_at = rows
        .iter()
        .map(|row| row.started_at.as_str())
        .filter(|value| !value.is_empty())
        .min_by(|left, right| compare_timestamps(left, right))
        .unwrap_or("")
        .to_string();
    session.ended_at = rows
        .iter()
        .map(|row| row.ended_at.as_str())
        .filter(|value| !value.is_empty())
        .max_by(|left, right| compare_timestamps(left, right))
        .unwrap_or("")
        .to_string();
    if let Some(latest_model) = rows
        .iter()
        .filter(|row| !row.model.is_empty())
        .max_by(|left, right| compare_timestamps(&left.ended_at, &right.ended_at))
    {
        session.model = latest_model.model.clone();
    }
    session.source_files = rows.iter().map(|row| row.source_file.clone()).collect();
    let capability_set = rows
        .iter()
        .flat_map(|row| row.capabilities.iter().cloned())
        .collect::<BTreeSet<_>>();
    let mut capabilities = [CAPABILITY_MESSAGES, CAPABILITY_EVENTS, CAPABILITY_USAGE]
        .into_iter()
        .filter(|capability| capability_set.contains(*capability))
        .map(str::to_string)
        .collect::<Vec<_>>();
    capabilities.extend(capability_set.into_iter().filter(|capability| {
        !matches!(
            capability.as_str(),
            CAPABILITY_MESSAGES | CAPABILITY_EVENTS | CAPABILITY_USAGE
        )
    }));
    session.capabilities = capabilities;
    session
}

pub(crate) fn merge_parsed_conversations(
    mut parsed_files: Vec<ParsedConversation>,
) -> ParsedConversation {
    parsed_files.sort_by(|left, right| left.session.source_file.cmp(&right.session.source_file));
    let session = merge_session_rows(
        &parsed_files
            .iter()
            .map(|parsed| parsed.session.clone())
            .collect::<Vec<_>>(),
    );

    let mut messages = parsed_files
        .iter()
        .flat_map(|parsed| parsed.messages.iter().cloned())
        .collect::<Vec<_>>();
    messages.sort_by(|left, right| {
        compare_timestamps(&left.occurred_at, &right.occurred_at)
            .then_with(|| left.role.cmp(&right.role))
            .then_with(|| left.text.cmp(&right.text))
    });
    let mut seen_messages = BTreeSet::new();
    messages.retain(|message| {
        seen_messages.insert((
            message.occurred_at.clone(),
            message.role.clone(),
            message.text.clone(),
        ))
    });

    let is_top_level = parsed_files.iter().all(|parsed| parsed.is_top_level);
    let mut sourced_events = Vec::new();
    for parsed in parsed_files {
        let source_file = parsed.session.source_file;
        let mut occurrences = BTreeMap::<String, u32>::new();
        for event in parsed.events {
            let identity = event_identity(&event);
            let occurrence = occurrences.entry(identity.clone()).or_default();
            let dedupe_key = format!("{identity}\u{1f}{}", *occurrence);
            *occurrence += 1;
            sourced_events.push((source_file.clone(), dedupe_key, event));
        }
    }
    let mut seen_events = BTreeSet::new();
    sourced_events.retain(|(_, dedupe_key, _)| seen_events.insert(dedupe_key.clone()));
    sourced_events.sort_by(|(left_path, _, left), (right_path, _, right)| {
        compare_event_timestamps(left, right)
            .then_with(|| left_path.cmp(right_path))
            .then_with(|| left.source_sequence.cmp(&right.source_sequence))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    let events = sourced_events
        .into_iter()
        .enumerate()
        .map(|(sequence, (_, _, mut event))| {
            event.sequence = sequence as u32;
            event
        })
        .collect();

    ParsedConversation {
        session,
        messages,
        events,
        is_top_level,
        index_cursor: None,
    }
}

pub(crate) fn extract_agent_metadata(events: &[ConversationEvent]) -> IndexedAgentMetadata {
    let indexed = events
        .iter()
        .filter_map(index_agent_event)
        .collect::<Vec<_>>();
    fold_agent_metadata(indexed.iter())
}

pub(crate) fn summarize_for_index(parsed: ParsedConversation) -> IndexedFile {
    IndexedFile {
        is_top_level: parsed.is_top_level,
        agent_events: parsed.events.iter().filter_map(index_agent_event).collect(),
        session: parsed.session,
    }
}

/// 与 `merge_parsed_conversations` 的事件合并保持同一套语义：先按文件内出现次数生成去重键，
/// 跨文件保留首次出现，再按 (时间, 文件名, 文件内序号, event_id) 排序。
pub(crate) fn merge_indexed_files(
    mut files: Vec<IndexedFile>,
) -> (ConversationSessionRow, bool, IndexedAgentMetadata) {
    files.sort_by(|left, right| left.session.source_file.cmp(&right.session.source_file));
    let session = merge_session_rows(
        &files
            .iter()
            .map(|file| file.session.clone())
            .collect::<Vec<_>>(),
    );
    let is_top_level = files.iter().all(|file| file.is_top_level);

    let mut sourced = Vec::new();
    for file in files {
        let source_file = file.session.source_file;
        let mut occurrences = BTreeMap::<u64, u32>::new();
        for event in file.agent_events {
            let occurrence = occurrences.entry(event.identity).or_default();
            let dedupe_key = (event.identity, *occurrence);
            *occurrence += 1;
            sourced.push((source_file.clone(), dedupe_key, event));
        }
    }
    let mut seen = BTreeSet::new();
    sourced.retain(|(_, dedupe_key, _)| seen.insert(*dedupe_key));
    sourced.sort_by(|(left_path, _, left), (right_path, _, right)| {
        compare_optional_timestamps(&left.occurred_at, &right.occurred_at)
            .then_with(|| left_path.cmp(right_path))
            .then_with(|| left.source_sequence.cmp(&right.source_sequence))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });

    let agent = fold_agent_metadata(sourced.iter().map(|(_, _, event)| event));
    (session, is_top_level, agent)
}

/// 目录索引真正要写库的只有会话行和父子关系；事件正文另写入 `conversation_events`（ADR 0011）。
///
/// 但父子关系必须等一个会话的全部文件合并、去重、排序之后才算得对，所以每解析完一个文件，
/// 就把相关事件压成这个形状：保留合并所需的去重键与排序键，正文只留 `fold_agent_metadata`
/// 真正读的那几个字段。整份 `events`/`messages` 随即释放——否则扫描期间整个来源的全部
/// 事件会同时活着，一次「重建全部」就是几个 GB。
pub(crate) struct IndexedAgentEvent {
    /// 只用于去重比较，所以存哈希而不是 `event_identity` 的完整字符串——
    /// 一条 tool_result 的身份串可以有几十 KB，而它是要一直留到合并阶段的。
    identity: u64,
    event_id: String,
    source_sequence: u32,
    occurred_at: Option<String>,
    role: IndexedAgentRole,
}

pub(crate) enum IndexedAgentRole {
    SessionStarted {
        parent_session_ids: Vec<String>,
    },
    SpawnCall {
        call_id: String,
    },
    ToolResult {
        call_id: String,
        child_session_id: Option<String>,
    },
}

/// 事件是否与父子关系有关，完全由 `kind`/`name`/`details` 决定，而这三者都参与
/// `event_identity`——所以「先过滤再去重」和「先去重再过滤」结果一致。
pub(crate) fn index_agent_event(event: &ConversationEvent) -> Option<IndexedAgentEvent> {
    let role = if event.kind == EventKind::SystemStatus
        && event.name.as_deref() == Some("session_started")
    {
        IndexedAgentRole::SessionStarted {
            parent_session_ids: [
                event.details.get("parent_id"),
                event.details.get("parent_session_id"),
                event.details.pointer("/source/subagent/parent_id"),
                event.details.pointer("/source/subagent/parent_session_id"),
            ]
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        }
    } else {
        let call_id = event.details.get("call_id").and_then(Value::as_str)?;
        match event.kind {
            EventKind::ToolCall
                if matches!(
                    event.name.as_deref(),
                    Some("spawn_agent" | "Agent" | "Task")
                ) =>
            {
                IndexedAgentRole::SpawnCall {
                    call_id: call_id.to_string(),
                }
            }
            EventKind::ToolResult => IndexedAgentRole::ToolResult {
                call_id: call_id.to_string(),
                child_session_id: structured_agent_id(&event.details),
            },
            _ => return None,
        }
    };
    Some(IndexedAgentEvent {
        identity: {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&event_identity(event), &mut hasher);
            std::hash::Hasher::finish(&hasher)
        },
        event_id: event.event_id.clone(),
        source_sequence: event.source_sequence,
        occurred_at: event.occurred_at.clone(),
        role,
    })
}

pub(crate) fn fold_agent_metadata<'a>(
    events: impl Iterator<Item = &'a IndexedAgentEvent>,
) -> IndexedAgentMetadata {
    let mut parent_session_ids = BTreeSet::new();
    let mut spawn_calls = BTreeMap::new();
    let mut spawn_results = BTreeMap::new();
    for event in events {
        match &event.role {
            IndexedAgentRole::SessionStarted {
                parent_session_ids: ids,
            } => {
                parent_session_ids.extend(ids.iter().cloned());
            }
            IndexedAgentRole::SpawnCall { call_id } => {
                spawn_calls.insert(call_id.clone(), event.event_id.clone());
            }
            IndexedAgentRole::ToolResult {
                call_id,
                child_session_id,
            } => {
                spawn_results.insert(call_id.clone(), child_session_id.clone());
            }
        }
    }
    let spawn_attempts = spawn_calls
        .into_iter()
        .map(|(call_id, launch_event_id)| IndexedSpawnAttempt {
            launch_event_id,
            child_session_id: spawn_results.get(&call_id).cloned().flatten(),
        })
        .collect();
    IndexedAgentMetadata {
        parent_session_ids: parent_session_ids.into_iter().collect(),
        spawn_attempts,
    }
}

pub(crate) fn structured_agent_id(value: &Value) -> Option<String> {
    if let Some(agent_id) = value
        .as_object()
        .and_then(|object| object.get("agent_id"))
        .and_then(Value::as_str)
        .filter(|agent_id| !agent_id.is_empty())
    {
        return Some(agent_id.to_string());
    }
    for key in ["output", "result"] {
        let Some(candidate) = value.get(key) else {
            continue;
        };
        if let Some(agent_id) = structured_agent_id(candidate) {
            return Some(agent_id);
        }
        if let Some(text) = candidate.as_str() {
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                if let Some(agent_id) = structured_agent_id(&parsed) {
                    return Some(agent_id);
                }
            }
        }
    }
    None
}

pub(crate) fn event_identity(event: &ConversationEvent) -> String {
    let mut normalized = event.clone();
    normalized.event_id.clear();
    normalized.sequence = 0;
    normalized.source_file.clear();
    normalized.source_sequence = 0;
    for (index, attachment) in normalized.attachments.iter_mut().enumerate() {
        attachment.id = index.to_string();
    }
    serde_json::to_string(&normalized).unwrap_or_default()
}
