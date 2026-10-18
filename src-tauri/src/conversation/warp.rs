//! Warp 对话记录：本机 `warp.sqlite` 不存助手回复正文，目录保持空。

use std::path::{Path, PathBuf};

use super::toolbox::ParsedConversation;
use super::{ConversationIndexBatch, ConversationIndexIssue};

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::warp::discover(roots)
}

pub(super) fn index(_path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    Ok(ConversationIndexBatch {
        conversations: Vec::new(),
        diagnostics: Vec::new(),
    })
}

pub(super) fn detail(
    _path: &Path,
    _session_id: &str,
    _include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    Err("Warp 本机库不含助手回复正文".to_string())
}

pub(super) fn source_revision(path: &Path) -> Result<String, String> {
    super::discover::wal_source_revision(path, "Warp")
}
