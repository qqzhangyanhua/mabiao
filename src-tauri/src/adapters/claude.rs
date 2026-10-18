use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapters::project::decode_dashed_dir;
use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::PathOverrides;

/// Claude Code 在部分安装方式下把会话写到 XDG 目录（`~/.config/claude`）而不是
/// `~/.claude`；默认两个都扫，显式设置 `CLAUDE_CONFIG_DIR` 后只扫用户指定的 Code 根。
///
/// Claude Desktop / Cowork 另把会话写进桌面数据目录下的嵌套
/// `local-agent-mode-sessions/*/*/local_*/.claude/projects`。这些目录只在磁盘上
/// 真实存在时追加，不替换 Code 根；`CLAUDE_CONFIG_DIR` 也不关掉 Desktop 发现。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    let roots = overrides
        .get("CLAUDE_CONFIG_DIR")
        .cloned()
        .unwrap_or_else(|| vec![home.join(".claude"), home.join(".config/claude")]);
    let mut dirs: Vec<PathBuf> = roots
        .into_iter()
        .map(|root| root.join("projects"))
        .collect();
    for extra in desktop_project_dirs(home) {
        if !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    dirs
}

/// 各平台 Claude Desktop 数据根下、已经存在的 Cowork / Desktop 会话 `projects`。
pub(crate) fn desktop_project_dirs(home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for root in desktop_data_roots(home) {
        collect_nested_claude_projects(&root, &mut found, &mut seen);
    }
    found
}

fn desktop_data_roots(home: &Path) -> Vec<PathBuf> {
    let mut roots = vec![
        home.join("Library/Application Support/Claude"),
        home.join("Library/Application Support/Claude-3p"),
        home.join(".config/Claude"),
        home.join(".config/Claude-3p"),
        home.join("AppData/Roaming/Claude"),
        home.join("AppData/Local/Claude"),
        home.join("AppData/Local/Claude-3p"),
    ];
    let packages = home.join("AppData/Local/Packages");
    if is_real_dir(&packages) {
        if let Ok(entries) = fs::read_dir(&packages) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                if !name.starts_with("Claude_") {
                    continue;
                }
                let cache = entry.path().join("LocalCache");
                roots.push(cache.join("Roaming/Claude"));
                roots.push(cache.join("Local/Claude"));
                roots.push(cache.join("Local/Claude-3p"));
            }
        }
    }
    roots
}

fn collect_nested_claude_projects(
    root: &Path,
    found: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    let sessions = root.join("local-agent-mode-sessions");
    if !is_real_dir(&sessions) {
        return;
    }
    for account in real_child_dirs(&sessions) {
        if account.file_name().and_then(|name| name.to_str()) == Some("skills-plugin") {
            continue;
        }
        for org in real_child_dirs(&account) {
            for local in real_child_dirs(&org) {
                let Some(name) = local.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !name.starts_with("local_") {
                    continue;
                }
                let projects = local.join(".claude/projects");
                if is_real_dir(&projects) && seen.insert(projects.clone()) {
                    found.push(projects);
                }
            }
        }
    }
}

fn real_child_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_real_dir(path))
        .collect()
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_dir())
        .unwrap_or(false)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    crate::ingest::validate_jsonl_file(path)?;
    let loc = path.to_string_lossy();
    let parent_ids = parent_assistant_message_ids(path);
    let factory: &LineFactory<'_> = &|| crate::ingest::open_jsonl_lines(path);
    Ok(parse_claude_shaped_jsonl_with(
        factory,
        loc.as_ref(),
        Source::Claude,
        &parent_ids,
    ))
}

struct ClaudeTurn {
    record: UsageRecord,
    stop_reason: Option<String>,
    is_sidechain: bool,
}

/// 下面两轮扫描都通过 `lines()` 重新拿一份新的行迭代器，配合磁盘流式读取场景，
/// 不需要先把整份文件内容读进内存再扫两遍。
pub fn parse_claude_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    parse_claude_shaped_jsonl(lines, source_file, Source::Claude)
}

/// Claude Code 形态的 jsonl：`type=assistant` + `message.usage`，按 `message.id` 去重。
/// Qoder / Qoder CN 复用同一套归一化，只改 `source`。
pub fn parse_claude_shaped_jsonl(
    lines: &LineFactory<'_>,
    source_file: &str,
    source: Source,
) -> Vec<UsageRecord> {
    parse_claude_shaped_jsonl_with(lines, source_file, source, &HashSet::new())
}

fn parse_claude_shaped_jsonl_with(
    lines: &LineFactory<'_>,
    source_file: &str,
    source: Source,
    parent_message_ids: &HashSet<String>,
) -> Vec<UsageRecord> {
    let mut project = String::new();
    let mut session_id = String::new();
    for value in parse_jsonl_value_lines(lines()) {
        if project.is_empty() {
            project = text_field(&value, &["cwd"]);
        }
        if session_id.is_empty() {
            session_id = text_field(&value, &["sessionId", "session_id"]);
        }
        if !project.is_empty() && !session_id.is_empty() {
            break;
        }
    }
    if project.is_empty() {
        project = project_from_path(source_file);
    }
    if session_id.is_empty() {
        session_id = crate::adapters::project::session_id_from_source_file(source_file);
    }

    let mut by_id: HashMap<String, ClaudeTurn> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut anonymous = Vec::new();

    for value in parse_jsonl_value_lines(lines()) {
        if value.get("type").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let message = value.get("message").cloned().unwrap_or_default();
        let usage = message.get("usage").cloned().unwrap_or_default();
        if usage.is_null() {
            continue;
        }
        let is_sidechain = value.get("isSidechain").and_then(|v| v.as_bool()) == Some(true);
        let cache_creation = cache_creation_from_usage(&usage);
        let agent_id = text_field(&value, &["agentId", "agent_id"]);
        let record_session_id = if agent_id.is_empty() {
            session_id.clone()
        } else {
            agent_id
        };
        let message_id = text_field(&message, &["id"]);
        // subagents/ 里会把父会话同一 message.id 再写一遍（含父级 cache-read）。
        // 对齐 ccusage：有 isSidechain 且父文件已有该 id 时丢掉回放，子代理自己的 id 仍计入。
        if is_sidechain && !message_id.is_empty() && parent_message_ids.contains(&message_id) {
            continue;
        }
        anonymous.extend(advisor_records_from_usage(
            &usage,
            &value,
            source,
            &project,
            &record_session_id,
            source_file,
        ));
        let record = finish(UsageRecord {
            occurred_at: text_field(&value, &["timestamp"]),
            source,
            model: text_field(&message, &["model"]),
            provider: String::new(),
            project: project.clone(),
            session_id: record_session_id,
            source_file: source_file.to_string(),
            input_tokens: i64_field(&usage, &["input_tokens"]),
            output_tokens: i64_field(&usage, &["output_tokens"]),
            cache_read_tokens: i64_field(&usage, &["cache_read_input_tokens"]),
            cache_creation_tokens: cache_creation.0,
            cache_creation_1h_tokens: cache_creation.1,
            reasoning_tokens: 0,
            total_tokens: 0,
            native_cost: native_cost_from_event(&value),
        });
        if !has_billable_tokens(&record) {
            continue;
        }
        let stop_reason = message
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if message_id.is_empty() {
            anonymous.push(record);
            continue;
        }
        let turn = ClaudeTurn {
            record,
            stop_reason,
            is_sidechain,
        };
        let should_replace = match by_id.get(&message_id) {
            None => true,
            Some(existing) => should_replace_claude(existing, &turn),
        };
        if should_replace {
            if !by_id.contains_key(&message_id) {
                order.push(message_id.clone());
            }
            by_id.insert(message_id, turn);
        }
    }

    let mut records: Vec<UsageRecord> = order
        .into_iter()
        .filter_map(|id| by_id.remove(&id).map(|turn| turn.record))
        .collect();
    records.extend(anonymous);
    records
}

/// 同一 `message.id`：有 sidechain 回放时保留非 sidechain 父条目；
/// 否则与 cc-switch 一致，优先有 stop_reason 的，再取 output 更大的。
fn should_replace_claude(existing: &ClaudeTurn, next: &ClaudeTurn) -> bool {
    match (existing.is_sidechain, next.is_sidechain) {
        (true, false) => true,
        (false, true) => false,
        _ => match (existing.stop_reason.is_some(), next.stop_reason.is_some()) {
            (false, true) => true,
            (true, false) => false,
            _ => next.record.output_tokens > existing.record.output_tokens,
        },
    }
}

/// `…/<session>/subagents/*.jsonl` 的父会话助理 message.id，用来丢掉跨文件回放。
fn parent_assistant_message_ids(path: &Path) -> HashSet<String> {
    let Some(dir) = path.parent() else {
        return HashSet::new();
    };
    if dir.file_name().and_then(|name| name.to_str()) != Some("subagents") {
        return HashSet::new();
    }
    let Some(session_dir) = dir.parent() else {
        return HashSet::new();
    };
    let Ok(entries) = fs::read_dir(session_dir) else {
        return HashSet::new();
    };
    let mut ids = HashSet::new();
    for entry in entries.flatten() {
        let candidate = entry.path();
        if candidate.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        collect_assistant_message_ids(&candidate, &mut ids);
    }
    ids
}

fn collect_assistant_message_ids(path: &Path, ids: &mut HashSet<String>) {
    for value in parse_jsonl_value_lines(crate::ingest::open_jsonl_lines(path)) {
        if value.get("type").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let id = value
            .get("message")
            .map(|message| text_field(message, &["id"]))
            .unwrap_or_default();
        if !id.is_empty() {
            ids.insert(id);
        }
    }
}

/// `message.usage.iterations` 里只有 `type=advisor_message` 单独入账；
/// `type=message` 与顶层 usage 重复，不再加。advisor 没有 `costUSD`。
fn advisor_records_from_usage(
    usage: &serde_json::Value,
    event: &serde_json::Value,
    source: Source,
    project: &str,
    session_id: &str,
    source_file: &str,
) -> Vec<UsageRecord> {
    let Some(iterations) = usage.get("iterations").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    iterations
        .iter()
        .filter(|iteration| {
            iteration.get("type").and_then(|value| value.as_str()) == Some("advisor_message")
        })
        .filter_map(|iteration| {
            let model = text_field(iteration, &["model"]);
            if model.is_empty() {
                return None;
            }
            let cache_creation = cache_creation_from_usage(iteration);
            let record = finish(UsageRecord {
                occurred_at: text_field(event, &["timestamp"]),
                source,
                model,
                provider: String::new(),
                project: project.to_string(),
                session_id: session_id.to_string(),
                source_file: source_file.to_string(),
                input_tokens: i64_field(iteration, &["input_tokens"]),
                output_tokens: i64_field(iteration, &["output_tokens"]),
                cache_read_tokens: i64_field(iteration, &["cache_read_input_tokens"]),
                cache_creation_tokens: cache_creation.0,
                cache_creation_1h_tokens: cache_creation.1,
                reasoning_tokens: 0,
                total_tokens: 0,
                native_cost: None,
            });
            has_billable_tokens(&record).then_some(record)
        })
        .collect()
}

/// KPI 的 `cache_creation_tokens` 仍是 5m+1h 合计（优先 `cache_creation_input_tokens`，
/// 缺失时用嵌套 5m+1h）；1h 部分另记，计价时乘 1h 单价。
fn cache_creation_from_usage(usage: &serde_json::Value) -> (i64, i64) {
    let listed = i64_field(usage, &["cache_creation_input_tokens"]).max(0);
    let nested = usage.get("cache_creation");
    let one_h = nested
        .map(|value| i64_field(value, &["ephemeral_1h_input_tokens"]))
        .unwrap_or(0)
        .max(0);
    let five_m = nested
        .map(|value| i64_field(value, &["ephemeral_5m_input_tokens"]))
        .unwrap_or(0)
        .max(0);
    let total = if listed > 0 {
        listed
    } else {
        five_m.saturating_add(one_h)
    };
    (total, one_h.min(total))
}

fn native_cost_from_event(value: &serde_json::Value) -> Option<f64> {
    for key in ["costUSD", "costUsd", "cost_usd"] {
        if let Some(amount) = value.get(key).and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_i64().map(|n| n as f64))
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        }) {
            if amount > 0.0 {
                return Some(amount);
            }
        }
    }
    None
}

fn project_from_path(source_file: &str) -> String {
    std::path::Path::new(source_file)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(decode_dashed_dir)
        .unwrap_or_default()
}
