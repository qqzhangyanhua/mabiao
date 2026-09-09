//! 对话详情「上下文清单」：会话内已观测 + 可能生效 / 磁盘存在。
//!
//! Cursor transcript **不落**发送时完整注入清单（见 `instructions/cursor_disk.rs`
//! 探测注释）。本模块只聚合已有事件索引 / 解析结果，再叠加实时磁盘扫描。
//! 合成的 `transcript_missing` 不算已观测。Skill 认事件 `name`/`text`
//! 字面引用，以及 Cursor `Skill` 工具的 `input.skill`（解析路径读
//! `details`；索引路径靠工具 `text`）。
//!
//! Grok 复用同一套 `ConversationContextManifest` / `ConversationContextItem`，
//! 磁盘层扫用户级 `~/.grok` 指令 / skills 与四条 MCP 加载链，不要另造第三套 DTO。

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection};
use serde_json::json;

use crate::domain::{
    ConversationContextItem, ConversationContextKind, ConversationContextLayer,
    ConversationContextManifest, ConversationEvent, ConversationEventKind as EventKind,
    ConversationSessionRow, Source,
};

use super::event_index;

const OBSERVED_EMPTY: &str =
    "源文件未留下可观测的工具、系统状态或 skill 引用，无法确认本轮注入了什么。";
const ON_DISK_POSSIBLE: &str =
    "下列文件与 MCP server 名来自磁盘扫描，可能生效 / 磁盘存在，不是本轮一定进了上下文。未列入 ~/.cursor/skills-cursor：Cursor 内置 skill，产品未给口径。";
const NO_PROJECT: &str =
    "会话没有项目路径，且用户级 MCP/skills 未发现；源文件未落盘注入清单，无法确认。未列入 ~/.cursor/skills-cursor：Cursor 内置 skill，产品未给口径。";
const MISSING_PROJECT: &str =
    "项目路径在本机不存在，且未发现用户级 MCP/skills；源文件未落盘注入清单，无法确认。未列入 ~/.cursor/skills-cursor：Cursor 内置 skill，产品未给口径。";
const PROJECT_EMPTY: &str =
    "项目下未发现指令文件、rules、skills 或 MCP 配置。未列入 ~/.cursor/skills-cursor：Cursor 内置 skill，产品未给口径。";
const SYNTHETIC_STATUS: &[&str] = &["transcript_missing"];
const GROK_ON_DISK_POSSIBLE: &str =
    "下列文件、skills 与 MCP server 来自 Grok 磁盘扫描，可能生效 / 磁盘存在，不是本轮一定进了上下文。不扫描项目根 AGENTS.md 或 .cursor/rules。";
const GROK_ON_DISK_EMPTY: &str =
    "未发现 Grok 会加载的指令、skills 或 MCP 配置。不扫描项目根 AGENTS.md 或 .cursor/rules。";

pub(crate) fn supported_source(source: Source) -> bool {
    matches!(source, Source::CursorAgent | Source::Grok)
}

pub(crate) fn for_session(
    home: &Path,
    source: Source,
    session: &ConversationSessionRow,
    observed: Vec<ConversationContextItem>,
) -> Option<ConversationContextManifest> {
    let on_disk = match source {
        Source::CursorAgent => {
            crate::instructions::cursor_disk::scan(home, Path::new(session.project.as_str()))
        }
        Source::Grok => {
            crate::instructions::grok_disk::scan(home, Path::new(session.project.as_str()))
        }
        _ => return None,
    };
    Some(assemble(
        source,
        observed,
        on_disk,
        session.project.as_str(),
    ))
}

pub(crate) fn assemble(
    source: Source,
    observed: Vec<ConversationContextItem>,
    on_disk: Vec<ConversationContextItem>,
    project: &str,
) -> ConversationContextManifest {
    debug_assert!(observed
        .iter()
        .all(|item| item.layer == ConversationContextLayer::Observed));
    debug_assert!(on_disk
        .iter()
        .all(|item| item.layer == ConversationContextLayer::OnDiskPossible));
    let observed_note = observed.is_empty().then(|| OBSERVED_EMPTY.to_string());
    let on_disk_note = Some(on_disk_note(source, project, on_disk.is_empty()));
    let mut items = observed;
    items.extend(on_disk);
    ConversationContextManifest {
        items,
        observed_note,
        on_disk_note,
    }
}

pub(crate) fn observed_from_events(events: &[ConversationEvent]) -> Vec<ConversationContextItem> {
    let mut tools = BTreeMap::<String, u64>::new();
    let mut statuses = BTreeMap::<String, ConversationContextItem>::new();
    let mut skills = BTreeMap::<String, ConversationContextItem>::new();
    for event in events {
        match event.kind {
            EventKind::ToolCall => {
                let Some(name) = event.name.as_deref().filter(|name| !name.is_empty()) else {
                    continue;
                };
                *tools.entry(name.to_string()).or_default() += 1;
                if let Some(item) =
                    observed_skill_item(name, event.text.as_deref(), Some(&event.details))
                {
                    skills.entry(item.id.clone()).or_insert(item);
                }
            }
            EventKind::SystemStatus => {
                if let Some(item) = status_item(ConversationContextKind::SystemStatus, event) {
                    statuses.entry(item.id.clone()).or_insert(item);
                }
            }
            EventKind::Error => {
                if let Some(item) = status_item(ConversationContextKind::Error, event) {
                    statuses.entry(item.id.clone()).or_insert(item);
                }
            }
            EventKind::Message
            | EventKind::Plan
            | EventKind::ToolResult
            | EventKind::ModelChange
            | EventKind::Unadapted => {}
        }
    }
    let mut items: Vec<ConversationContextItem> = tools
        .into_iter()
        .map(|(name, call_count)| tool_item(name, call_count))
        .collect();
    items.extend(statuses.into_values());
    items.extend(skills.into_values());
    items
}

pub(crate) fn observed_from_index(
    conn: &Connection,
    source: Source,
    session_id: &str,
) -> Result<Vec<ConversationContextItem>, String> {
    let Some(generation) = event_index::session_generation(conn, source.as_str(), session_id)?
    else {
        return Ok(Vec::new());
    };
    let mut tools = BTreeMap::<String, u64>::new();
    {
        let mut statement = conn
            .prepare(
                r#"
                SELECT name, COUNT(*)
                FROM conversation_events
                WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
                  AND kind = 'tool_call' AND COALESCE(name, '') != ''
                GROUP BY name
                "#,
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![source.as_str(), session_id, generation], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (name, count) = row.map_err(|error| error.to_string())?;
            tools.insert(name, count);
        }
    }

    let mut extras = Vec::new();
    {
        let mut statement = conn
            .prepare(
                r#"
                SELECT kind, name, text
                FROM conversation_events
                WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3
                  AND kind IN ('tool_call', 'system_status', 'error')
                ORDER BY sequence
                "#,
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![source.as_str(), session_id, generation], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        let mut statuses = BTreeMap::<String, ConversationContextItem>::new();
        let mut skills = BTreeMap::<String, ConversationContextItem>::new();
        for row in rows {
            let (kind, name, text) = row.map_err(|error| error.to_string())?;
            match kind.as_str() {
                "tool_call" => {
                    if let Some(name) = name.as_deref().filter(|name| !name.is_empty()) {
                        if let Some(item) = observed_skill_item(name, text.as_deref(), None) {
                            skills.entry(item.id.clone()).or_insert(item);
                        }
                    }
                }
                "system_status" | "error" => {
                    let kind = if kind == "error" {
                        ConversationContextKind::Error
                    } else {
                        ConversationContextKind::SystemStatus
                    };
                    if let Some(item) = status_from_parts(kind, name.as_deref(), text.as_deref()) {
                        statuses.entry(item.id.clone()).or_insert(item);
                    }
                }
                _ => {}
            }
        }
        extras.extend(statuses.into_values());
        extras.extend(skills.into_values());
    }

    let mut items: Vec<ConversationContextItem> = tools
        .into_iter()
        .map(|(name, call_count)| tool_item(name, call_count))
        .collect();
    items.extend(extras);
    Ok(items)
}

fn on_disk_note(source: Source, project: &str, empty: bool) -> String {
    if source == Source::Grok {
        return if empty {
            GROK_ON_DISK_EMPTY.to_string()
        } else {
            GROK_ON_DISK_POSSIBLE.to_string()
        };
    }
    if !empty {
        return ON_DISK_POSSIBLE.to_string();
    }
    if project.trim().is_empty() {
        return NO_PROJECT.to_string();
    }
    if !Path::new(project).is_dir() {
        return MISSING_PROJECT.to_string();
    }
    PROJECT_EMPTY.to_string()
}

fn tool_item(name: String, call_count: u64) -> ConversationContextItem {
    ConversationContextItem {
        layer: ConversationContextLayer::Observed,
        kind: ConversationContextKind::Tool,
        id: name.clone(),
        label: name,
        path: None,
        meta: Some(json!({ "call_count": call_count })),
    }
}

fn status_item(
    kind: ConversationContextKind,
    event: &ConversationEvent,
) -> Option<ConversationContextItem> {
    status_from_parts(kind, event.name.as_deref(), event.text.as_deref())
}

fn status_from_parts(
    kind: ConversationContextKind,
    name: Option<&str>,
    text: Option<&str>,
) -> Option<ConversationContextItem> {
    let name = name.map(str::trim).filter(|name| !name.is_empty());
    if name.is_some_and(|name| SYNTHETIC_STATUS.contains(&name)) {
        return None;
    }
    let text = text.map(str::trim).filter(|text| !text.is_empty());
    let id = name.or(text).filter(|value| !value.is_empty())?.to_string();
    let label = name.unwrap_or(id.as_str()).to_string();
    let mut meta = serde_json::Map::new();
    if let Some(text) = text {
        meta.insert("text".into(), json!(text));
    }
    Some(ConversationContextItem {
        layer: ConversationContextLayer::Observed,
        kind,
        id,
        label,
        path: None,
        meta: if meta.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(meta))
        },
    })
}

/// 只认事件 `name`/`text` 与工具 `details.input.skill`；不扫用户/助手正文。
fn observed_skill_item(
    name: &str,
    text: Option<&str>,
    details: Option<&serde_json::Value>,
) -> Option<ConversationContextItem> {
    let id = skill_id_from_details(details)
        .or_else(|| skill_id_from_skill_tool(name, text))
        .or_else(|| literal_skill_id(name))
        .or_else(|| text.and_then(literal_skill_id))?;
    Some(ConversationContextItem {
        layer: ConversationContextLayer::Observed,
        kind: ConversationContextKind::Skill,
        id: id.clone(),
        label: id,
        path: None,
        meta: Some(json!({ "source": "event" })),
    })
}

fn skill_id_from_details(details: Option<&serde_json::Value>) -> Option<String> {
    let details = details?;
    details
        .get("input")
        .and_then(|input| json_nonempty_str(input, "skill"))
        .or_else(|| json_nonempty_str(details, "skill"))
}

fn skill_id_from_skill_tool(name: &str, text: Option<&str>) -> Option<String> {
    if !name.eq_ignore_ascii_case("skill") {
        return None;
    }
    text.map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("skill"))
        .map(ToString::to_string)
}

fn json_nonempty_str(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn literal_skill_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    for prefix in ["Skill:", "skill:", "SKILL:"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let id = rest.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    if let Some(index) = trimmed.find("SKILL.md") {
        let start = trimmed[..index]
            .rfind(|ch: char| ch.is_whitespace() || ch == '"' || ch == '\'')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        let end = index + "SKILL.md".len();
        let token = trimmed[start..end].trim_matches(|ch: char| matches!(ch, '"' | '\'' | ','));
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::observed_from_events;
    use crate::domain::{
        ConversationContextKind, ConversationEvent, ConversationEventCapabilityStatus,
        ConversationEventContentStatus, ConversationEventKind,
    };
    use serde_json::json;

    fn event(
        kind: ConversationEventKind,
        name: Option<&str>,
        text: Option<&str>,
        details: serde_json::Value,
    ) -> ConversationEvent {
        ConversationEvent {
            event_id: String::new(),
            sequence: 0,
            source_file: String::new(),
            source_sequence: 0,
            kind,
            occurred_at: Some("2026-09-08T00:00:00Z".to_string()),
            actor: None,
            name: name.map(ToString::to_string),
            text: text.map(ToString::to_string),
            details,
            attachments: Vec::new(),
            capability_status: ConversationEventCapabilityStatus::Complete,
            content_status: ConversationEventContentStatus::Complete,
        }
    }

    #[test]
    fn observed_from_events_reads_skill_from_tool_details() {
        let items = observed_from_events(&[event(
            ConversationEventKind::ToolCall,
            Some("Skill"),
            None,
            json!({"input":{"skill":"deploy"}}),
        )]);
        assert!(items
            .iter()
            .any(|item| { item.kind == ConversationContextKind::Skill && item.id == "deploy" }));
    }

    #[test]
    fn observed_from_events_skips_synthetic_transcript_missing() {
        let items = observed_from_events(&[event(
            ConversationEventKind::SystemStatus,
            Some("transcript_missing"),
            Some("Cursor transcript 不可读取；仅展示确定性关联的用量与状态"),
            json!({}),
        )]);
        assert!(items.is_empty());
    }
}
