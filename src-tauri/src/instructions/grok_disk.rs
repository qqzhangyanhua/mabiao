//! Grok 会话「可能生效」的磁盘扫描。
//!
//! # 口径（对照 `instructions/grok.rs` + 官方 Project Rules）
//!
//! 用户级（产品已确认会加载）：`~/.grok/` 下 `HOME_INSTRUCTION_NAMES`
//! 候选，以及 `~/.grok/rules/*.md`。存在才列，标「可能生效 / 磁盘存在」，
//! **不是**「已注入」。
//!
//! 项目级：官方文档（https://docs.x.ai/build/features/project-rules ，
//! 2026-09 查阅）写明 Grok 会从仓库根走到 cwd，加载 `AGENTS.md` /
//! `Agents.md` / `AGENT.md` / `CLAUDE.md` / `Claude.md` /
//! `CLAUDE.local.md`，以及每层 `.grok/rules/*.md`，并兼容
//! `.claude/rules/` / `.cursor/rules/`。但本产品 `instructions/grok.rs`
//! **没有**项目 cwd 扫描口径，本清单不发明这些路径（对标 agy「未扫描」）。
//! 不把项目 `AGENTS.md` 或 Cursor `.cursor/rules` 假装列进来。
//!
//! MCP / skills：产品未接受 Grok 的磁盘布局，不扫；也不把 Cursor
//! `mcp.json` / `.cursor/skills` 套到 Grok。

use std::path::Path;

use crate::domain::{ConversationContextItem, ConversationContextKind, ConversationContextLayer};

use super::file;
use super::grok::HOME_INSTRUCTION_NAMES;
use super::project_walk::{existing_file, on_disk_file_item};

pub fn scan(home: &Path) -> Vec<ConversationContextItem> {
    let mut items = Vec::new();
    let dir = home.join(".grok");

    for name in HOME_INSTRUCTION_NAMES {
        if let Some(path) = existing_file(&dir, Path::new(name)) {
            if let Some(item) = on_disk_file_item(
                ConversationContextKind::Instruction,
                &format!("user:{name}"),
                name,
                &path,
                Some(format!("~/.grok/{name}")),
            ) {
                items.push(item);
            }
        }
    }

    let rules_dir = dir.join("rules");
    for (name, path) in file::list_files(&rules_dir) {
        if !name.ends_with(".md") {
            continue;
        }
        if let Some(item) = on_disk_file_item(
            ConversationContextKind::Rule,
            &format!("user:rules/{name}"),
            &name,
            &path,
            Some(format!("~/.grok/rules/{name}")),
        ) {
            items.push(item);
        }
    }

    items.sort_by(|left, right| {
        left.kind
            .as_str()
            .cmp(right.kind.as_str())
            .then_with(|| left.id.cmp(&right.id))
    });
    debug_assert!(items
        .iter()
        .all(|item| item.layer == ConversationContextLayer::OnDiskPossible));
    items
}
