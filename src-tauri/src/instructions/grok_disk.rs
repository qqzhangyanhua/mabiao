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
//! **不扫**项目根 `AGENTS.md`、`.cursor/rules`、MCP、skills：仓库与官方文档
//! 没有「Grok 会加载这些」的已验证证据。上下文清单不得假装有项目指令或
//! 「已注入」。缺文件时保持空名单，由 `context_manifest` 写诚实空态。
//!
//! 路径表只走 `grok::existing_*`，不另造第三套。

use std::path::Path;

use serde_json::Value;

use crate::domain::{ConversationContextItem, ConversationContextKind, ConversationContextLayer};

use super::grok::{existing_home_instruction_files, existing_rule_files};
use super::project_walk::file_stat;

pub fn scan(home: &Path) -> Vec<ConversationContextItem> {
    let mut items = Vec::new();
    for (display, path) in existing_home_instruction_files(home) {
        if let Some(item) = file_item(
            ConversationContextKind::Instruction,
            &display,
            file_label(&display),
            &path,
            &display,
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
    items
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
) -> Option<ConversationContextItem> {
    let (byte_size, modified_at) = file_stat(path)?;
    let mut meta = serde_json::Map::new();
    meta.insert("byte_size".into(), serde_json::json!(byte_size));
    if let Some(modified_at) = modified_at {
        meta.insert("modified_at".into(), serde_json::json!(modified_at));
    }
    meta.insert("display_path".into(), serde_json::json!(display));
    meta.insert("config_scope".into(), serde_json::json!("user"));
    Some(ConversationContextItem {
        layer: ConversationContextLayer::OnDiskPossible,
        kind,
        id: id.to_string(),
        label: label.to_string(),
        path: Some(path.to_string_lossy().into_owned()),
        meta: Some(Value::Object(meta)),
    })
}
