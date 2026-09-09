//! Grok 会话首轮注入快照。
//!
//! 读会话目录里的 `prompt_context.json`（`agents_md_files[]`，每项带
//! `file_path` 与全文）。与对话记录适配器隔离：不解析 `updates.jsonl`、
//! 不写 `conversation_events`、不把注入正文送进任何缓存。体积只保留字符数。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::domain::{
    ConversationContextItem, ConversationContextKind, ConversationContextLayer,
    ConversationContextLoadMode, ConversationSessionRow,
};

const PROMPT_CONTEXT: &str = "prompt_context.json";

#[derive(Deserialize)]
struct PromptContext {
    #[serde(default)]
    agents_md_files: Vec<AgentsMdFile>,
}

#[derive(Deserialize)]
struct AgentsMdFile {
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    file_path: String,
    #[serde(default)]
    content: String,
}

pub(crate) fn from_session(session: &ConversationSessionRow) -> Vec<ConversationContextItem> {
    let Some(path) = prompt_context_path(&session.source_file) else {
        return Vec::new();
    };
    from_path(&path)
}

fn prompt_context_path(source_file: &str) -> Option<PathBuf> {
    if source_file.is_empty() {
        return None;
    }
    Some(Path::new(source_file).parent()?.join(PROMPT_CONTEXT))
}

fn from_path(path: &Path) -> Vec<ConversationContextItem> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<PromptContext>(&text) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    for file in parsed.agents_md_files {
        let file_path = file.file_path.trim();
        if file_path.is_empty() || !seen.insert(file_path.to_string()) {
            continue;
        }
        let label = file_label(&file.file_name, file_path);
        items.push(ConversationContextItem {
            layer: ConversationContextLayer::Injected,
            kind: ConversationContextKind::Instruction,
            id: file_path.to_string(),
            label,
            path: Some(file_path.to_string()),
            load_mode: Some(ConversationContextLoadMode::Always),
            char_count: Some(file.content.chars().count() as u64),
            meta: None,
        });
    }
    items
}

fn file_label(file_name: &str, file_path: &str) -> String {
    let trimmed = file_name.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    Path::new(file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(file_path)
        .to_string()
}
