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
//! `~/.cursor/skills/` 官方有约定、本机未创建，存在才列。`instructions::*`
//! **没有**用户级 Cursor skills 口径。本机另有 `~/.cursor/skills-cursor/`
//! （Cursor 内置 skill，frontmatter 含 `name`），产品未给口径，**不列入**，
//! 避免把编辑器自带 skill 标成用户指令。
//!
//! 指令/rules 相对路径与 `conflict` / `project_walk` 共用，不另造第三套。

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::domain::{ConversationContextItem, ConversationContextKind, ConversationContextLayer};

use super::project_walk::{
    existing_file, file_stat, is_cursor_rule_file, is_skill_file, walk_files, CURSOR_MCP_REL,
    CURSOR_PROJECT_INSTRUCTION_NAMES, CURSOR_RULES_DIR, CURSOR_SKILLS_DIR, CURSOR_USER_MCP_REL,
    CURSOR_USER_SKILLS_DIR,
};

pub fn scan(home: &Path, project: &Path) -> Vec<ConversationContextItem> {
    let mut items = Vec::new();
    if project.as_os_str().is_empty() {
        push_mcp(
            &mut items,
            &home.join(CURSOR_USER_MCP_REL),
            "user",
            "~/.cursor/mcp.json",
        );
        push_skills(
            &mut items,
            &home.join(CURSOR_USER_SKILLS_DIR),
            "user",
            "~/.cursor/skills",
        );
        return items;
    }

    if project.is_dir() {
        for name in CURSOR_PROJECT_INSTRUCTION_NAMES {
            if let Some(path) = existing_file(project, Path::new(name)) {
                if let Some(item) = file_item(
                    ConversationContextKind::Instruction,
                    name,
                    name,
                    &path,
                    Some(name.to_string()),
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
            ) {
                items.push(item);
            }
        }
        push_skills(
            &mut items,
            &project.join(CURSOR_SKILLS_DIR),
            "project",
            CURSOR_SKILLS_DIR,
        );
        push_mcp(
            &mut items,
            &project.join(CURSOR_MCP_REL),
            "project",
            CURSOR_MCP_REL,
        );
    }

    push_mcp(
        &mut items,
        &home.join(CURSOR_USER_MCP_REL),
        "user",
        "~/.cursor/mcp.json",
    );
    push_skills(
        &mut items,
        &home.join(CURSOR_USER_SKILLS_DIR),
        "user",
        "~/.cursor/skills",
    );
    items.sort_by(|left, right| {
        left.kind
            .as_str()
            .cmp(right.kind.as_str())
            .then_with(|| left.id.cmp(&right.id))
    });
    items
}

fn push_skills(items: &mut Vec<ConversationContextItem>, dir: &Path, scope: &str, prefix: &str) {
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
) -> Option<ConversationContextItem> {
    let (byte_size, modified_at) = file_stat(path)?;
    let mut meta = serde_json::Map::new();
    meta.insert("byte_size".into(), serde_json::json!(byte_size));
    if let Some(modified_at) = modified_at {
        meta.insert("modified_at".into(), serde_json::json!(modified_at));
    }
    if let Some(display) = display {
        meta.insert("display_path".into(), serde_json::json!(display));
    }
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

fn rel_posix(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
