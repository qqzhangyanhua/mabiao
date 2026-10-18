use std::path::Path;

use super::codex;
use super::toolbox::ParsedConversation;
use super::{single_detail, ConversationIndexBatch, ConversationIndexIssue};
use crate::domain::Source;

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    codex::index_as(Source::Trae, path)
}

pub(super) fn index_suffix(
    path: &Path,
    byte_offset: u64,
    start_line: u32,
    session_id: &str,
) -> Result<ParsedConversation, ConversationIndexIssue> {
    codex::index_suffix_as(Source::Trae, path, byte_offset, start_line, session_id)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, parse_file)
}

fn parse_file(path: &Path, include_deferred_content: bool) -> Result<ParsedConversation, String> {
    codex::parse_file_as(Source::Trae, path, include_deferred_content)
}
