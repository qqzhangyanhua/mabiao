//! agent 父子图读写：父子链、断链判定、子 agent 目录可达性。
//!
//! 改这部分关系怎么算出来时，只看这里。按会话 id 反查会话行走兄弟模块
//! `session_store`，不经模块根。

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection};

use crate::domain::{
    ConversationAgentCapabilityStatus as AgentCapabilityStatus, ConversationAgentLink,
    ConversationAgentLinkStatus as AgentLinkStatus, ConversationAgentRelations, ConversationEvent,
    ConversationSessionRow, Source,
};

use super::merge::{extract_agent_metadata, IndexedAgentMetadata};
use super::session_store::load_session;

pub(crate) fn load_agent_relations(
    conn: &Connection,
    source: Source,
    current_session_id: &str,
    current_events: &[ConversationEvent],
) -> Result<ConversationAgentRelations, String> {
    let mut catalog = load_agent_catalog(conn, source)?;
    if !current_events.is_empty() {
        catalog
            .entry(current_session_id.to_string())
            .and_modify(|(_, metadata)| *metadata = extract_agent_metadata(current_events));
    }

    let mut parent_claims = BTreeMap::<String, BTreeSet<String>>::new();
    for (session_id, (_, metadata)) in &catalog {
        for parent_id in &metadata.parent_session_ids {
            parent_claims
                .entry(session_id.clone())
                .or_default()
                .insert(parent_id.clone());
        }
        for attempt in &metadata.spawn_attempts {
            if let Some(child_id) = &attempt.child_session_id {
                parent_claims
                    .entry(child_id.clone())
                    .or_default()
                    .insert(session_id.clone());
            }
        }
    }

    let current_metadata = &catalog
        .get(current_session_id)
        .ok_or_else(|| "未找到该对话记录".to_string())?
        .1;
    let mut child_launches = BTreeMap::<String, Option<String>>::new();
    for attempt in &current_metadata.spawn_attempts {
        if let Some(child_id) = &attempt.child_session_id {
            child_launches
                .entry(child_id.clone())
                .or_insert_with(|| Some(attempt.launch_event_id.clone()));
        }
    }
    for (child_id, parents) in &parent_claims {
        if parents.contains(current_session_id) {
            child_launches.entry(child_id.clone()).or_insert(None);
        }
    }

    let mut children = child_launches
        .into_iter()
        .map(|(child_id, launch_event_id)| {
            let status = agent_link_status(current_session_id, &child_id, &catalog, &parent_claims);
            let session = (status == AgentLinkStatus::Linked)
                .then(|| catalog.get(&child_id).map(|(session, _)| session.clone()))
                .flatten();
            ConversationAgentLink {
                relationship_id: launch_event_id
                    .clone()
                    .unwrap_or_else(|| format!("metadata:{current_session_id}:{child_id}")),
                session_id: Some(child_id),
                launch_event_id,
                status,
                session,
            }
        })
        .collect::<Vec<_>>();
    children.extend(
        current_metadata
            .spawn_attempts
            .iter()
            .filter(|attempt| attempt.child_session_id.is_none())
            .map(|attempt| ConversationAgentLink {
                relationship_id: attempt.launch_event_id.clone(),
                session_id: None,
                launch_event_id: Some(attempt.launch_event_id.clone()),
                status: AgentLinkStatus::Unresolved,
                session: None,
            }),
    );
    children.sort_by(|left, right| left.relationship_id.cmp(&right.relationship_id));

    let parent = build_parent_link(current_session_id, &catalog, &parent_claims);
    let statuses = parent
        .iter()
        .map(|link| link.status)
        .chain(children.iter().map(|link| link.status))
        .collect::<Vec<_>>();
    let has_linked = statuses.contains(&AgentLinkStatus::Linked);
    let has_unavailable = statuses
        .iter()
        .any(|status| *status != AgentLinkStatus::Linked);
    let capability_status = match (has_linked, has_unavailable) {
        (true, true) => AgentCapabilityStatus::Partial,
        (false, true) => AgentCapabilityStatus::Unavailable,
        _ => AgentCapabilityStatus::Complete,
    };

    Ok(ConversationAgentRelations {
        capability_status,
        parent,
        children,
    })
}

pub(crate) fn load_agent_catalog(
    conn: &Connection,
    source: Source,
) -> Result<BTreeMap<String, (ConversationSessionRow, IndexedAgentMetadata)>, String> {
    let indexed = {
        let mut statement = conn
            .prepare(
                "SELECT session_id, agent_metadata_json FROM conversation_sessions WHERE source = ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![source.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        rows
    };
    let mut catalog = BTreeMap::new();
    for (session_id, metadata_json) in indexed {
        let Some(session) = load_session(conn, source.as_str(), &session_id)? else {
            continue;
        };
        let metadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        catalog.insert(session_id, (session, metadata));
    }
    Ok(catalog)
}

pub(crate) fn build_parent_link(
    child_id: &str,
    catalog: &BTreeMap<String, (ConversationSessionRow, IndexedAgentMetadata)>,
    parent_claims: &BTreeMap<String, BTreeSet<String>>,
) -> Option<ConversationAgentLink> {
    let claims = parent_claims.get(child_id)?;
    if claims.len() != 1 {
        return Some(ConversationAgentLink {
            relationship_id: format!("conflict:{child_id}"),
            session_id: None,
            launch_event_id: None,
            status: AgentLinkStatus::Conflict,
            session: None,
        });
    }
    let parent_id = claims.iter().next()?.clone();
    let launch_event_id = catalog.get(&parent_id).and_then(|(_, metadata)| {
        metadata
            .spawn_attempts
            .iter()
            .find(|attempt| attempt.child_session_id.as_deref() == Some(child_id))
            .map(|attempt| attempt.launch_event_id.clone())
    });
    let status = agent_link_status(&parent_id, child_id, catalog, parent_claims);
    let session = (status == AgentLinkStatus::Linked)
        .then(|| catalog.get(&parent_id).map(|(session, _)| session.clone()))
        .flatten();
    Some(ConversationAgentLink {
        relationship_id: launch_event_id
            .clone()
            .unwrap_or_else(|| format!("metadata:{parent_id}:{child_id}")),
        session_id: Some(parent_id),
        launch_event_id,
        status,
        session,
    })
}

pub(crate) fn agent_link_status(
    parent_id: &str,
    child_id: &str,
    catalog: &BTreeMap<String, (ConversationSessionRow, IndexedAgentMetadata)>,
    parent_claims: &BTreeMap<String, BTreeSet<String>>,
) -> AgentLinkStatus {
    if !catalog.contains_key(parent_id) || !catalog.contains_key(child_id) {
        return AgentLinkStatus::MissingSource;
    }
    if parent_claims
        .get(child_id)
        .is_some_and(|claims| claims.len() > 1)
    {
        return AgentLinkStatus::Conflict;
    }
    if parent_id == child_id || agent_path_exists(child_id, parent_id, parent_claims) {
        return AgentLinkStatus::Cycle;
    }
    AgentLinkStatus::Linked
}

pub(crate) fn agent_path_exists(
    from: &str,
    target: &str,
    parent_claims: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    let mut pending = vec![from.to_string()];
    let mut visited = BTreeSet::new();
    while let Some(parent) = pending.pop() {
        if !visited.insert(parent.clone()) {
            continue;
        }
        for (child, parents) in parent_claims {
            if parents.contains(&parent) {
                if child == target {
                    return true;
                }
                pending.push(child.clone());
            }
        }
    }
    false
}
