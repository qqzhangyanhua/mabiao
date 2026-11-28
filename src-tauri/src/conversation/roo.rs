use std::path::{Path, PathBuf};

use super::extension_tasks;
use super::toolbox::ParsedConversation;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::domain::Source;

pub(super) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    extension_tasks::discover(roots)
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    extension_tasks::index(path, Source::Roo)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    extension_tasks::detail(path, session_id, include_deferred_content, Source::Roo)
}
