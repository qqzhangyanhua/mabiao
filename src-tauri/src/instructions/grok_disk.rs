//! Grok 会话「可能生效」的磁盘扫描。
//!
//! # 产品口径
//!
//! 官方 Project Rules（`instructions/grok.rs`，2026-08 查阅
//! https://docs.x.ai/build/features/project-rules）：
//! - `~/.grok/` 候选文件：AGENTS.md / Agents.md / AGENT.md / CLAUDE.md /
//!   Claude.md / CLAUDE.local.md
//! - `~/.grok/rules/*.md`
//!
//! Skills 与 MCP（2026-09 本机实测 + 随客户端分发的
//! `~/.grok/README.md`、`docs/user-guide/07-mcp-servers.md`、
//! `08-skills.md`、`26-config-reference.md`）：
//! - 用户级 `~/.grok/skills/**/SKILL.md`；`[skills] paths` 追加目录或
//!   单份 SKILL.md，`ignore` 排除路径，`disabled` 禁用 skill 名（发现
//!   但不激活，本层仍列出并标 `disabled`）
//! - Claude / Cursor 用户级 skills 跟随 `[compat.<vendor>] skills`，
//!   默认开启；关掉即不扫。不列入 `~/.cursor/skills-cursor`
//! - MCP 四条链合并，同名高优先级胜出：
//!   `config.toml`（用户级 `~/.grok/config.toml`，以及 cwd → git root
//!   各级 `.grok/config.toml`，最深优先）> `~/.claude.json` >
//!   用户级与项目级 `.cursor/mcp.json` > 项目根 `.mcp.json`
//! - `[compat.claude] mcps` / `[compat.cursor] mcps` 默认开启，关掉
//!   即不扫对应兼容源。`.mcp.json` 是独立第四条链
//!
//! **不扫**项目根 `AGENTS.md`、`.cursor/rules`。上下文清单不得假装
//! 「已注入」。缺文件时保持空名单，由 `context_manifest` 写诚实空态。
//!
//! 指令路径表只走 `grok::existing_*`，不另造第三套。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::domain::{ConversationContextItem, ConversationContextKind, ConversationContextLayer};

use super::grok::{existing_home_instruction_files, existing_rule_files, HOME_DIR};
use super::project_walk::{file_stat, is_skill_file, walk_files};

pub fn scan(home: &Path, project: &Path) -> Vec<ConversationContextItem> {
    let user_config = load_toml(&home.join(HOME_DIR).join("config.toml"));
    let user_config = user_config.as_ref();
    let ignores = toml_strings(user_config, &["skills", "ignore"])
        .into_iter()
        .map(|raw| expand_user_path(home, &raw))
        .collect::<Vec<_>>();
    let disabled_skills = toml_strings(user_config, &["skills", "disabled"])
        .into_iter()
        .collect::<BTreeSet<_>>();
    let extra_paths = toml_strings(user_config, &["skills", "paths"]);
    let claude_skills = toml_bool(user_config, &["compat", "claude", "skills"], true);
    let cursor_skills = toml_bool(user_config, &["compat", "cursor", "skills"], true);
    let claude_mcps = toml_bool(user_config, &["compat", "claude", "mcps"], true);
    let cursor_mcps = toml_bool(user_config, &["compat", "cursor", "mcps"], true);

    let mut items = Vec::new();
    for (display, path) in existing_home_instruction_files(home) {
        if let Some(item) = file_item(
            ConversationContextKind::Instruction,
            &display,
            file_label(&display),
            &path,
            &display,
            "user",
        ) {
            items.push(item);
        }
    }
    for (display, path) in existing_rule_files(home) {
        if let Some(item) = file_item(
            ConversationContextKind::Rule,
            &display,
            file_label(&display),
            &path,
            &display,
            "user",
        ) {
            items.push(item);
        }
    }

    let mut seen_skills = BTreeSet::new();
    collect_skill_dir(
        &home.join(HOME_DIR).join("skills"),
        "user",
        home,
        &ignores,
        &disabled_skills,
        &mut seen_skills,
        &mut items,
    );
    for raw in extra_paths {
        collect_skill_path(
            &expand_user_path(home, &raw),
            "config",
            home,
            &ignores,
            &disabled_skills,
            &mut seen_skills,
            &mut items,
        );
    }
    if claude_skills {
        collect_skill_dir(
            &home.join(".claude/skills"),
            "claude",
            home,
            &ignores,
            &disabled_skills,
            &mut seen_skills,
            &mut items,
        );
    }
    if cursor_skills {
        collect_skill_dir(
            &home.join(".cursor/skills"),
            "cursor",
            home,
            &ignores,
            &disabled_skills,
            &mut seen_skills,
            &mut items,
        );
    }

    let chain = dirs_cwd_to_git_root(project);
    let mut mcp = BTreeMap::new();
    extend_toml_mcp(
        &mut mcp,
        &home.join(HOME_DIR).join("config.toml"),
        "grok",
        "~/.grok/config.toml",
    );
    for dir in chain.iter().rev() {
        extend_toml_mcp(
            &mut mcp,
            &dir.join(".grok/config.toml"),
            "grok-project",
            ".grok/config.toml",
        );
    }
    if claude_mcps {
        insert_json_mcp(
            &mut mcp,
            &home.join(".claude.json"),
            "claude",
            "~/.claude.json",
        );
    }
    if cursor_mcps {
        let mut cursor = BTreeMap::new();
        insert_json_mcp_into(
            &mut cursor,
            &home.join(".cursor/mcp.json"),
            "cursor",
            "~/.cursor/mcp.json",
            true,
        );
        for dir in chain.iter().rev() {
            insert_json_mcp_into(
                &mut cursor,
                &dir.join(".cursor/mcp.json"),
                "cursor-project",
                ".cursor/mcp.json",
                true,
            );
        }
        for (name, entry) in cursor {
            mcp.entry(name).or_insert(entry);
        }
    }
    let mut mcp_json = BTreeMap::new();
    for dir in chain.iter().rev() {
        insert_json_mcp_into(
            &mut mcp_json,
            &dir.join(".mcp.json"),
            "mcp_json",
            ".mcp.json",
            true,
        );
    }
    for (name, entry) in mcp_json {
        mcp.entry(name).or_insert(entry);
    }
    for entry in mcp.into_values() {
        if !entry.enabled {
            continue;
        }
        if let Some(item) = mcp_item(&entry) {
            items.push(item);
        }
    }

    items.sort_by(|left, right| {
        left.kind
            .as_str()
            .cmp(right.kind.as_str())
            .then_with(|| left.id.cmp(&right.id))
    });
    items
}

struct McpEntry {
    name: String,
    scope: String,
    path: PathBuf,
    display: String,
    enabled: bool,
}

fn load_toml(path: &Path) -> Option<toml::Value> {
    fs::read_to_string(path).ok()?.parse().ok()
}

fn toml_bool(root: Option<&toml::Value>, keys: &[&str], default: bool) -> bool {
    let mut current = match root {
        Some(value) => value,
        None => return default,
    };
    for key in keys {
        match current.get(*key) {
            Some(next) => current = next,
            None => return default,
        }
    }
    current.as_bool().unwrap_or(default)
}

fn toml_strings(root: Option<&toml::Value>, keys: &[&str]) -> Vec<String> {
    let mut current = match root {
        Some(value) => value,
        None => return Vec::new(),
    };
    for key in keys {
        match current.get(*key) {
            Some(next) => current = next,
            None => return Vec::new(),
        }
    }
    current
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn expand_user_path(home: &Path, raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(trimmed)
}

fn dirs_cwd_to_git_root(start: &Path) -> Vec<PathBuf> {
    if start.as_os_str().is_empty() || !start.is_dir() {
        return Vec::new();
    }
    let mut chain = Vec::new();
    let mut current = start.to_path_buf();
    loop {
        chain.push(current.clone());
        if current.join(".git").exists() {
            return chain;
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => return vec![start.to_path_buf()],
        }
    }
}

fn collect_skill_path(
    path: &Path,
    scope: &str,
    home: &Path,
    ignores: &[PathBuf],
    disabled: &BTreeSet<String>,
    seen: &mut BTreeSet<String>,
    items: &mut Vec<ConversationContextItem>,
) {
    if path.is_file() {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        if is_skill_file(name) {
            push_skill(path, scope, home, ignores, disabled, seen, items);
        }
        return;
    }
    collect_skill_dir(path, scope, home, ignores, disabled, seen, items);
}

fn collect_skill_dir(
    dir: &Path,
    scope: &str,
    home: &Path,
    ignores: &[PathBuf],
    disabled: &BTreeSet<String>,
    seen: &mut BTreeSet<String>,
    items: &mut Vec<ConversationContextItem>,
) {
    if !dir.is_dir() {
        return;
    }
    for path in walk_files(dir, Path::new(""), 8, is_skill_file) {
        push_skill(&path, scope, home, ignores, disabled, seen, items);
    }
}

fn push_skill(
    path: &Path,
    scope: &str,
    home: &Path,
    ignores: &[PathBuf],
    disabled: &BTreeSet<String>,
    seen: &mut BTreeSet<String>,
    items: &mut Vec<ConversationContextItem>,
) {
    if ignores.iter().any(|ignore| path.starts_with(ignore)) {
        return;
    }
    let name = skill_name(path);
    if name.is_empty() || !seen.insert(name.clone()) {
        return;
    }
    let display = display_under_home(home, path);
    let id = format!("{scope}:{name}");
    if let Some(mut item) = file_item(
        ConversationContextKind::Skill,
        &id,
        &name,
        path,
        &display,
        scope,
    ) {
        if disabled.contains(&name) {
            if let Some(Value::Object(meta)) = item.meta.as_mut() {
                meta.insert("disabled".into(), serde_json::json!(true));
            }
        }
        items.push(item);
    }
}

fn skill_name(path: &Path) -> String {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "skills")
        .unwrap_or("SKILL.md")
        .to_string()
}

fn extend_toml_mcp(out: &mut BTreeMap<String, McpEntry>, path: &Path, scope: &str, display: &str) {
    let Some(value) = load_toml(path) else {
        return;
    };
    let Some(servers) = value.get("mcp_servers").and_then(|value| value.as_table()) else {
        return;
    };
    for (name, spec) in servers {
        if name.is_empty() {
            continue;
        }
        let enabled = spec
            .get("enabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        out.insert(
            name.clone(),
            McpEntry {
                name: name.clone(),
                scope: scope.to_string(),
                path: path.to_path_buf(),
                display: display.to_string(),
                enabled,
            },
        );
    }
}

fn insert_json_mcp(out: &mut BTreeMap<String, McpEntry>, path: &Path, scope: &str, display: &str) {
    insert_json_mcp_into(out, path, scope, display, false);
}

fn insert_json_mcp_into(
    out: &mut BTreeMap<String, McpEntry>,
    path: &Path,
    scope: &str,
    display: &str,
    overwrite: bool,
) {
    let Some(servers) = json_mcp_servers(path) else {
        return;
    };
    for (name, enabled) in servers {
        let entry = McpEntry {
            name: name.clone(),
            scope: scope.to_string(),
            path: path.to_path_buf(),
            display: display.to_string(),
            enabled,
        };
        if overwrite {
            out.insert(name, entry);
        } else {
            out.entry(name).or_insert(entry);
        }
    }
}

fn json_mcp_servers(path: &Path) -> Option<Vec<(String, bool)>> {
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let servers = value.get("mcpServers")?.as_object()?;
    let mut names: Vec<(String, bool)> = servers
        .iter()
        .filter(|(name, _)| !name.is_empty())
        .map(|(name, spec)| (name.clone(), json_server_enabled(spec)))
        .collect();
    names.sort_by(|left, right| left.0.cmp(&right.0));
    Some(names)
}

fn json_server_enabled(spec: &Value) -> bool {
    if spec.get("enabled") == Some(&Value::Bool(false)) {
        return false;
    }
    spec.get("disabled") != Some(&Value::Bool(true))
}

fn mcp_item(entry: &McpEntry) -> Option<ConversationContextItem> {
    let (byte_size, modified_at) = file_stat(&entry.path)?;
    Some(ConversationContextItem {
        layer: ConversationContextLayer::OnDiskPossible,
        kind: ConversationContextKind::McpServer,
        id: format!("{}:{}", entry.scope, entry.name),
        label: entry.name.clone(),
        path: Some(entry.path.to_string_lossy().into_owned()),
        load_mode: None,
        injection_status: None,
        char_count: None,
        is_noise: false,
        meta: Some(serde_json::json!({
            "byte_size": byte_size,
            "modified_at": modified_at,
            "config_scope": entry.scope,
            "config_path": entry.display,
        })),
    })
}

fn display_under_home(home: &Path, path: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rel) => format!("~/{}", rel.to_string_lossy().replace('\\', "/")),
        Err(_) => path.to_string_lossy().replace('\\', "/"),
    }
}

fn file_label(display: &str) -> &str {
    display.rsplit('/').next().unwrap_or(display)
}

fn file_item(
    kind: ConversationContextKind,
    id: &str,
    label: &str,
    path: &Path,
    display: &str,
    scope: &str,
) -> Option<ConversationContextItem> {
    let (byte_size, modified_at) = file_stat(path)?;
    let mut meta = serde_json::Map::new();
    meta.insert("byte_size".into(), serde_json::json!(byte_size));
    if let Some(modified_at) = modified_at {
        meta.insert("modified_at".into(), serde_json::json!(modified_at));
    }
    meta.insert("display_path".into(), serde_json::json!(display));
    meta.insert("config_scope".into(), serde_json::json!(scope));
    Some(ConversationContextItem {
        layer: ConversationContextLayer::OnDiskPossible,
        kind,
        id: id.to_string(),
        label: label.to_string(),
        path: Some(path.to_string_lossy().into_owned()),
        load_mode: None,
        injection_status: None,
        char_count: None,
        is_noise: false,
        meta: Some(Value::Object(meta)),
    })
}
