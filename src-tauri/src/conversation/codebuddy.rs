use std::path::Path;

use super::buddy_jsonl;
use super::normalize::ParsedConversation;
use super::{ConversationIndexBatch, ConversationIndexIssue};
use crate::domain::Source;

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    buddy_jsonl::index(path, Source::CodeBuddy, true)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    buddy_jsonl::detail(
        path,
        session_id,
        include_deferred_content,
        Source::CodeBuddy,
        true,
    )
}
