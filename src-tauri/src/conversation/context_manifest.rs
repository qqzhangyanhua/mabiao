//! 对话详情「上下文清单」：已注入 + 会话内已观测 + 可能生效 / 磁盘存在。
//!
//! Grok 注入层读会话目录 `prompt_context.json` 的 `agents_md_files[]`，以及
//! `events.jsonl` 的 MCP 配置解析 / 连接成功 / 连接失败 / 初始化完成。Cursor
//! 注入层读 `~/.cursor/chats/<hash>/<session>/store.db` 下标 1 的 user 消息。
//! 与 `updates.jsonl` 解析隔离，正文和 MCP 错误全文不进缓存。合成的
//! `transcript_missing` 不算已观测。Skill 认事件 `name`/`text` 字面引用，
//! 以及 Cursor `Skill` 工具的 `input.skill`（解析路径读 `details`；索引路径靠工具 `text`）。
//!
//! 噪音差集（injected 减 observed）对 skill 与 MCP 通用；指令与规则不参与。
//! 未连上的 MCP 不占 token、不吃红标。编辑器内置 skill 计入体积，不吃红标。
//! 磁盘有、注入快照没有的条目标 `is_unused_install`（白装了）；`on_match`
//! 未命中的规则本来就不该加载，不进这个差集。
//!
//! Grok 复用同一套 `ConversationContextManifest` / `ConversationContextItem`，
//! 磁盘层扫用户级 `~/.grok` 指令 / skills 与四条 MCP 加载链，不要另造第三套 DTO。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use chrono::DateTime;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

use crate::domain::{
    ConversationContextInjectionStatus, ConversationContextItem, ConversationContextKind,
    ConversationContextLayer, ConversationContextLoadMode, ConversationContextManifest,
    ConversationEvent, ConversationEventKind as EventKind, ConversationSessionRow, Source,
};

use super::context_cache::CachedContextMetrics;
use super::{cursor_inject, event_index, grok_inject};

const INJECTED_EMPTY: &str = "未发现本轮注入条目。";
const INJECTED_PRESENT: &str = "下列条目已注入本会话首轮上下文。";
const INJECTED_DEGRADED: &str = "注入快照已过期（Cursor 只保留约 40 天），以下为按当前磁盘状态重建";
const INJECTED_FROM_CACHE: &str = "注入快照已清理，下列度量结果来自缓存。";

const OBSERVED_EMPTY: &str =
    "源文件未留下可观测的工具、系统状态或 skill 引用，无法确认本轮注入了什么。";
const ON_DISK_POSSIBLE: &str =
    "下列文件与 MCP server 名来自磁盘扫描，可能生效 / 磁盘存在，不是本轮一定进了上下文。";
const NO_PROJECT: &str =
    "会话没有项目路径，且用户级 MCP/skills 未发现；源文件未落盘注入清单，无法确认。";
const MISSING_PROJECT: &str =
    "项目路径在本机不存在，且未发现用户级 MCP/skills；源文件未落盘注入清单，无法确认。";
const PROJECT_EMPTY: &str = "项目下未发现指令文件、rules、skills 或 MCP 配置。";
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
    cached: Option<CachedContextMetrics>,
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
    let live = match source {
        Source::Grok => {
            let snapshot = grok_inject::from_session(session);
            snapshot.found.then_some((
                snapshot.items,
                snapshot.mcp_init_summary,
                snapshot.called_mcp,
                snapshot.found,
                None,
            ))
        }
        Source::CursorAgent => {
            let snapshot = cursor_inject::from_session(home, session);
            snapshot.found.then_some((
                snapshot.items,
                None,
                BTreeSet::new(),
                snapshot.found,
                snapshot.incomplete_keys,
            ))
        }
        _ => None,
    };
    let from_cache = live.is_none() && cached.is_some();
    let (injected, mcp_init_summary, extra_observed, snapshot_found, incomplete_keys) =
        if let Some(live) = live {
            live
        } else if let Some(cached) = cached {
            (
                cached.items,
                cached.mcp_init_summary,
                BTreeSet::new(),
                true,
                cached.incomplete_keys,
            )
        } else if source == Source::Grok {
            (Vec::new(), None, BTreeSet::new(), true, None)
        } else {
            (Vec::new(), None, BTreeSet::new(), false, None)
        };
    let mut manifest = assemble(
        source,
        injected,
        observed,
        on_disk,
        session.project.as_str(),
        mcp_init_summary,
        &extra_observed,
    );
    apply_honesty(
        &mut manifest,
        source,
        session,
        snapshot_found,
        from_cache,
        incomplete_keys,
    );
    Some(manifest)
}

pub(crate) fn assemble(
    source: Source,
    mut injected: Vec<ConversationContextItem>,
    observed: Vec<ConversationContextItem>,
    mut on_disk: Vec<ConversationContextItem>,
    project: &str,
    mcp_init_summary: Option<String>,
    extra_observed: &BTreeSet<String>,
) -> ConversationContextManifest {
    debug_assert!(injected
        .iter()
        .all(|item| item.layer == ConversationContextLayer::Injected));
    debug_assert!(observed
        .iter()
        .all(|item| item.layer == ConversationContextLayer::Observed));
    debug_assert!(on_disk
        .iter()
        .all(|item| item.layer == ConversationContextLayer::OnDiskPossible));
    mark_injected_noise(&mut injected, &observed, extra_observed);
    mark_unused_installs(&mut on_disk, &injected);
    let injected_note = Some(if injected.is_empty() {
        INJECTED_EMPTY.to_string()
    } else {
        INJECTED_PRESENT.to_string()
    });
    let observed_note = observed.is_empty().then(|| OBSERVED_EMPTY.to_string());
    let on_disk_note = Some(on_disk_note(source, project, on_disk.is_empty()));
    let mut items = injected;
    items.extend(observed);
    items.extend(on_disk);
    ConversationContextManifest {
        items,
        injected_note,
        observed_note,
        on_disk_note,
        mcp_init_summary,
        has_injected_snapshot: true,
        metrics_from_cache: false,
        volume_is_estimate: false,
        completeness_note: None,
        first_uses: Vec::new(),
    }
}

fn apply_honesty(
    manifest: &mut ConversationContextManifest,
    source: Source,
    session: &ConversationSessionRow,
    snapshot_found: bool,
    from_cache: bool,
    incomplete_keys: Option<Vec<String>>,
) {
    if from_cache {
        manifest.metrics_from_cache = true;
        manifest.has_injected_snapshot = true;
        manifest.injected_note = Some(INJECTED_FROM_CACHE.to_string());
    }
    if source == Source::CursorAgent {
        manifest.volume_is_estimate = true;
        if !snapshot_found && !from_cache {
            manifest.has_injected_snapshot = false;
            manifest.injected_note = Some(INJECTED_DEGRADED.to_string());
        }
        if let Some(keys) = incomplete_keys {
            if !keys.is_empty() {
                manifest.completeness_note = Some(format!("上下文采集不完整：{}", keys.join("、")));
            }
        }
    }
    mark_changed_after_session(&mut manifest.items, &session.ended_at);
}

fn mark_changed_after_session(items: &mut [ConversationContextItem], session_ended_at: &str) {
    if session_ended_at.is_empty() {
        return;
    }
    let Ok(ended) = DateTime::parse_from_rfc3339(session_ended_at) else {
        return;
    };
    for item in items {
        if item.layer != ConversationContextLayer::OnDiskPossible {
            continue;
        }
        let Some(meta) = item.meta.as_mut().and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(mtime) = meta
            .get("modified_at")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let Ok(modified) = DateTime::parse_from_rfc3339(&mtime) else {
            continue;
        };
        if modified > ended {
            meta.insert("changed_after_session".into(), json!(true));
        }
    }
}

fn mark_injected_noise(
    injected: &mut [ConversationContextItem],
    observed: &[ConversationContextItem],
    extra_observed: &BTreeSet<String>,
) {
    for item in injected.iter_mut() {
        if !matches!(
            item.kind,
            ConversationContextKind::Skill | ConversationContextKind::McpServer
        ) {
            item.is_noise = false;
            continue;
        }
        if is_editor_builtin(item) {
            item.is_noise = false;
            continue;
        }
        if item.kind == ConversationContextKind::McpServer
            && item.injection_status != Some(ConversationContextInjectionStatus::Connected)
        {
            item.is_noise = false;
            continue;
        }
        item.is_noise = !item_was_observed(item, observed, extra_observed);
    }
}

fn mark_unused_installs(
    on_disk: &mut [ConversationContextItem],
    injected: &[ConversationContextItem],
) {
    for item in on_disk.iter_mut() {
        item.is_unused_install = unused_install(item, injected);
    }
}

fn unused_install(item: &ConversationContextItem, injected: &[ConversationContextItem]) -> bool {
    if is_editor_builtin(item) {
        return false;
    }
    if item.load_mode == Some(ConversationContextLoadMode::OnMatch) {
        return false;
    }
    !injected.iter().any(|other| same_context_item(item, other))
}

fn is_editor_builtin(item: &ConversationContextItem) -> bool {
    if item.id.starts_with("editor_builtin:") {
        return true;
    }
    if item
        .meta
        .as_ref()
        .and_then(|meta| meta.get("config_scope"))
        .and_then(|value| value.as_str())
        == Some("editor_builtin")
    {
        return true;
    }
    path_has_segment(item.path.as_deref(), "skills-cursor")
        || path_has_segment(Some(item.id.as_str()), "skills-cursor")
}

fn path_has_segment(path: Option<&str>, segment: &str) -> bool {
    path.is_some_and(|path| path.split(['/', '\\']).any(|part| part == segment))
}

fn same_context_item(disk: &ConversationContextItem, injected: &ConversationContextItem) -> bool {
    if !kinds_compatible(disk.kind, injected.kind) {
        return false;
    }
    if matches!(
        disk.kind,
        ConversationContextKind::Skill | ConversationContextKind::McpServer
    ) && !disk.label.is_empty()
        && disk.label == injected.label
    {
        return true;
    }
    if disk.kind == ConversationContextKind::McpServer
        && (disk.id == injected.id
            || disk.id == format!("user:{}", injected.id)
            || disk.id == format!("project:{}", injected.id))
    {
        return true;
    }
    path_eq_or_suffix(disk.path.as_deref(), injected.path.as_deref())
        || path_eq_or_suffix(disk.path.as_deref(), Some(injected.id.as_str()))
        || path_eq_or_suffix(Some(disk.id.as_str()), injected.path.as_deref())
        || path_eq_or_suffix(Some(disk.id.as_str()), Some(injected.id.as_str()))
}

fn kinds_compatible(disk: ConversationContextKind, injected: ConversationContextKind) -> bool {
    disk == injected
        || matches!(
            (disk, injected),
            (
                ConversationContextKind::Instruction,
                ConversationContextKind::Rule
            ) | (
                ConversationContextKind::Rule,
                ConversationContextKind::Instruction
            )
        )
}

fn path_eq_or_suffix(left: Option<&str>, right: Option<&str>) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return false;
    };
    let left = left.replace('\\', "/");
    let right = right.replace('\\', "/");
    if left.is_empty() || right.is_empty() {
        return false;
    }
    left == right || left.ends_with(&right) || right.ends_with(&left)
}

fn item_was_observed(
    item: &ConversationContextItem,
    observed: &[ConversationContextItem],
    extra_observed: &BTreeSet<String>,
) -> bool {
    match item.kind {
        ConversationContextKind::Skill => {
            extra_observed.contains(&item.id)
                || extra_observed.contains(&item.label)
                || observed.iter().any(|other| {
                    other.kind == ConversationContextKind::Skill
                        && (other.id == item.id || other.label == item.label)
                })
        }
        ConversationContextKind::McpServer => mcp_was_observed(item, observed, extra_observed),
        _ => false,
    }
}

fn mcp_was_observed(
    item: &ConversationContextItem,
    observed: &[ConversationContextItem],
    extra_observed: &BTreeSet<String>,
) -> bool {
    if extra_observed.contains(&item.id) || extra_observed.contains(&item.label) {
        return true;
    }
    let prefix = format!("{}__", item.label);
    let id_prefix = format!("{}__", item.id);
    if observed.iter().any(|other| {
        other.kind == ConversationContextKind::McpServer
            && (other.id == item.id || other.label == item.label)
            || other.kind == ConversationContextKind::Tool
                && (other.id.starts_with(&prefix)
                    || other.label.starts_with(&prefix)
                    || other.id.starts_with(&id_prefix))
    }) {
        return true;
    }
    let tools = item
        .meta
        .as_ref()
        .and_then(|meta| meta.get("tools"))
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str());
    for tool in tools {
        if extra_observed.contains(tool)
            || extra_observed.contains(&format!("{}__{tool}", item.label))
            || extra_observed.contains(&format!("{}__{tool}", item.id))
        {
            return true;
        }
        if observed.iter().any(|other| {
            other.kind == ConversationContextKind::Tool
                && (other.id == tool
                    || other.label == tool
                    || other.id == format!("{}__{tool}", item.label)
                    || other.id == format!("{}__{tool}", item.id))
        }) {
            return true;
        }
    }
    false
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
        load_mode: Some(ConversationContextLoadMode::Observed),
        injection_status: None,
        char_count: None,
        is_noise: false,
        is_unused_install: false,
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
        load_mode: Some(ConversationContextLoadMode::Observed),
        injection_status: None,
        char_count: None,
        is_noise: false,
        is_unused_install: false,
        meta: if meta.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(meta))
        },
    })
}

/// 只认事件 `name`/`text` 与工具 `details.input.skill`；不扫用户/助手正文。
pub(crate) fn skill_id_from_call(
    name: &str,
    text: Option<&str>,
    details: Option<&serde_json::Value>,
) -> Option<String> {
    skill_id_from_details(details)
        .or_else(|| skill_id_from_skill_tool(name, text))
        .or_else(|| literal_skill_id(name))
        .or_else(|| text.and_then(literal_skill_id))
}

fn observed_skill_item(
    name: &str,
    text: Option<&str>,
    details: Option<&serde_json::Value>,
) -> Option<ConversationContextItem> {
    let id = skill_id_from_call(name, text, details)?;
    Some(ConversationContextItem {
        layer: ConversationContextLayer::Observed,
        kind: ConversationContextKind::Skill,
        id: id.clone(),
        label: id,
        path: None,
        load_mode: Some(ConversationContextLoadMode::Observed),
        injection_status: None,
        char_count: None,
        is_noise: false,
        is_unused_install: false,
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
