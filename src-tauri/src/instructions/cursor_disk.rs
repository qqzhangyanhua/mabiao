//! Cursor 项目上「可能生效」的磁盘扫描。
//!
//! # 本机探测（2026-09，Cloud / 本仓库）
//!
//! `agent-transcripts` jsonl **没有**发送时完整注入清单：现有 Adapter 与
//! `docs/probe/cursor-agent.md` 只见到 `user` / `assistant`（`tool_use` /
//! `tool_result`）/ `turn_ended`。因此本层只能列磁盘上存在的候选，不得写成
//! 「已注入」。
//!
//! MCP：官方用户/项目文件是 `{home|project}/.cursor/mcp.json`，对象键为
//! `mcpServers`（本机插件缓存里的 `mcp.json` 也是这个键）。**只扫用户级与
//! 项目级这两处**，不扫 `~/.cursor/plugins/cache/**/mcp.json`——那是插件打包
//! 物，不是用户声明的本轮挂载。
//!
//! Skills：项目级 `.cursor/skills/**/SKILL.md` 按 #251 点名列入。用户自建
//! `~/.cursor/skills/` 官方有约定、本机未创建，存在才列。
//! `~/.cursor/skills-cursor/`（Cursor 内置 skill，frontmatter 含 `name`）
//! 单独成一档 `editor_builtin`：计入体积，不标成用户可删的噪音。
//!
//! `.cursor/rules` 下的 `.mdc` / `.md` 按 frontmatter 分成 `always` /
//! `on_match` / `on_demand` / `manual` 四档。分档用来算「白装了」差集，
//! 不是推测本轮会不会加载。
//!
//! 指令/rules 相对路径与 `conflict` / `project_walk` 共用，不另造第三套。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::domain::{
    ConversationContextItem, ConversationContextKind, ConversationContextLayer,
    ConversationContextLoadMode,
};

use super::project_walk::{
    existing_file, file_stat, is_cursor_rule_file, is_skill_file, walk_files,
    CURSOR_BUILTIN_SKILLS_DIR, CURSOR_MCP_REL, CURSOR_PROJECT_INSTRUCTION_NAMES, CURSOR_RULES_DIR,
    CURSOR_SKILLS_DIR, CURSOR_USER_MCP_REL, CURSOR_USER_SKILLS_DIR,
};

pub fn scan(home: &Path, project: &Path) -> Vec<ConversationContextItem> {
    let mut items = Vec::new();
    if !project.as_os_str().is_empty() && project.is_dir() {
        for name in CURSOR_PROJECT_INSTRUCTION_NAMES {
            if let Some(path) = existing_file(project, Path::new(name)) {
                if let Some(item) = file_item(
                    ConversationContextKind::Instruction,
                    name,
                    name,
                    &path,
                    Some(name.to_string()),
                    None,
                    None,
                ) {
                    items.push(item);
                }
            }
        }
        for path in walk_files(project, Path::new(CURSOR_RULES_DIR), 3, is_cursor_rule_file) {
            let rel = rel_posix(project, &path);
            if let Some(item) = file_item(
                ConversationContextKind::Rule,
                &rel,
                &rel,
                &path,
                Some(rel.clone()),
                None,
                None,
            ) {
                items.push(item);
            }
        }
        push_skills(
            &mut items,
            &project.join(CURSOR_SKILLS_DIR),
            "project",
            CURSOR_SKILLS_DIR,
            None,
        );
        push_mcp(
            &mut items,
            &project.join(CURSOR_MCP_REL),
            "project",
            CURSOR_MCP_REL,
        );
    }

    push_user_and_builtin(&mut items, home);
    items.sort_by(|left, right| {
        left.kind
            .as_str()
            .cmp(right.kind.as_str())
            .then_with(|| left.id.cmp(&right.id))
    });
    items
}

fn push_user_and_builtin(items: &mut Vec<ConversationContextItem>, home: &Path) {
    push_mcp(
        items,
        &home.join(CURSOR_USER_MCP_REL),
        "user",
        "~/.cursor/mcp.json",
    );
    push_skills(
        items,
        &home.join(CURSOR_USER_SKILLS_DIR),
        "user",
        "~/.cursor/skills",
        None,
    );
    push_skills(
        items,
        &home.join(CURSOR_BUILTIN_SKILLS_DIR),
        "editor_builtin",
        "~/.cursor/skills-cursor",
        Some(ConversationContextLoadMode::Always),
    );
}

fn push_skills(
    items: &mut Vec<ConversationContextItem>,
    dir: &Path,
    scope: &str,
    prefix: &str,
    load_mode: Option<ConversationContextLoadMode>,
) {
    if !dir.is_dir() {
        return;
    }
    for path in walk_files(dir, Path::new(""), 4, is_skill_file) {
        let skill_id = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty() && *name != "skills")
            .unwrap_or("SKILL.md");
        let id = format!("{scope}:{skill_id}");
        let nested = rel_posix(dir, &path);
        let rel = if nested.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}/{nested}")
        };
        if let Some(item) = file_item(
            ConversationContextKind::Skill,
            &id,
            skill_id,
            &path,
            Some(rel),
            Some(scope),
            load_mode,
        ) {
            items.push(item);
        }
    }
}

fn push_mcp(items: &mut Vec<ConversationContextItem>, path: &Path, scope: &str, display: &str) {
    let Some((byte_size, modified_at)) = file_stat(path) else {
        return;
    };
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let Some(servers) = value.get("mcpServers").and_then(Value::as_object) else {
        return;
    };
    let mut names: Vec<String> = servers.keys().cloned().collect();
    names.sort();
    for name in names {
        if name.is_empty() {
            continue;
        }
        items.push(ConversationContextItem {
            layer: ConversationContextLayer::OnDiskPossible,
            kind: ConversationContextKind::McpServer,
            id: format!("{scope}:{name}"),
            label: name,
            path: Some(path.to_string_lossy().into_owned()),
            load_mode: None,
            injection_status: None,
            char_count: None,
            is_noise: false,
            is_unused_install: false,
            meta: Some(serde_json::json!({
                "byte_size": byte_size,
                "modified_at": modified_at,
                "config_scope": scope,
                "config_path": display,
            })),
        });
    }
}

fn file_item(
    kind: ConversationContextKind,
    id: &str,
    label: &str,
    path: &Path,
    display: Option<String>,
    scope: Option<&str>,
    load_mode: Option<ConversationContextLoadMode>,
) -> Option<ConversationContextItem> {
    let (byte_size, modified_at) = file_stat(path)?;
    let text = fs::read_to_string(path).ok();
    let char_count = text.as_ref().map(|text| text.chars().count() as u64);
    let load_mode = if kind == ConversationContextKind::Rule {
        Some(mdc_load_mode(text.as_deref().unwrap_or("")))
    } else {
        load_mode
    };
    let mut meta = serde_json::Map::new();
    meta.insert("byte_size".into(), serde_json::json!(byte_size));
    if let Some(modified_at) = modified_at {
        meta.insert("modified_at".into(), serde_json::json!(modified_at));
    }
    if let Some(display) = display {
        meta.insert("display_path".into(), serde_json::json!(display));
    }
    if let Some(scope) = scope {
        meta.insert("config_scope".into(), serde_json::json!(scope));
    }
    Some(ConversationContextItem {
        layer: ConversationContextLayer::OnDiskPossible,
        kind,
        id: id.to_string(),
        label: label.to_string(),
        path: Some(path.to_string_lossy().into_owned()),
        load_mode,
        injection_status: None,
        char_count,
        is_noise: false,
        is_unused_install: false,
        meta: Some(Value::Object(meta)),
    })
}

fn rel_posix(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn mdc_load_mode(text: &str) -> ConversationContextLoadMode {
    let Some(map) = parse_frontmatter(text) else {
        return ConversationContextLoadMode::Manual;
    };
    if yaml_true(map.get("alwaysApply")) {
        return ConversationContextLoadMode::Always;
    }
    if yaml_present(map.get("globs")) {
        return ConversationContextLoadMode::OnMatch;
    }
    if yaml_present(map.get("description")) {
        return ConversationContextLoadMode::OnDemand;
    }
    ConversationContextLoadMode::Manual
}

fn parse_frontmatter(text: &str) -> Option<BTreeMap<String, String>> {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = body.strip_prefix("---")?;
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))?;
    let end = rest.find("\n---")?;
    Some(parse_simple_yaml_map(&rest[..end]))
}

fn parse_simple_yaml_map(front: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut current_key: Option<String> = None;
    let mut list_vals = Vec::new();
    for line in front.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            if current_key.is_some() {
                let item = unquote(item.trim());
                if !item.is_empty() {
                    list_vals.push(item);
                }
            }
            continue;
        }
        flush_yaml_key(&mut map, &mut current_key, &mut list_vals);
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = unquote(value.trim());
        if value.is_empty() {
            current_key = Some(key.to_string());
        } else {
            map.insert(key.to_string(), value);
        }
    }
    flush_yaml_key(&mut map, &mut current_key, &mut list_vals);
    map
}

fn flush_yaml_key(
    map: &mut BTreeMap<String, String>,
    current_key: &mut Option<String>,
    list_vals: &mut Vec<String>,
) {
    if let Some(key) = current_key.take() {
        if !list_vals.is_empty() {
            map.insert(key, list_vals.join(","));
        }
        list_vals.clear();
    }
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

fn yaml_true(value: Option<&String>) -> bool {
    matches!(
        value.map(String::as_str),
        Some("true" | "True" | "TRUE" | "yes" | "Yes")
    )
}

fn yaml_present(value: Option<&String>) -> bool {
    value.is_some_and(|value| {
        let trimmed = value.trim();
        !trimmed.is_empty() && trimmed != "~" && !trimmed.eq_ignore_ascii_case("null")
    })
}
