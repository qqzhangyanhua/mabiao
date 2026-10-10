use std::path::Path;

use super::toolbox::*;
use super::{claude, single_detail, ConversationIndexBatch, ConversationIndexIssue};

fn wrap_index(
    path: &Path,
    parse: fn(&Path, bool) -> Result<ParsedConversation, String>,
) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    parse(path, false)
        .map(|conversation| ConversationIndexBatch {
            conversations: vec![conversation],
            diagnostics: Vec::new(),
        })
        .map_err(|message| ConversationIndexIssue {
            path: path.to_string_lossy().to_string(),
            message,
            event_type: None,
            line: None,
        })
}

pub(super) fn index(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    wrap_index(path, parse)
}

pub(super) fn detail(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, parse)
}

pub(super) fn parse(
    path: &Path,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    claude::parse_as(path, include_deferred_content, Source::Qoder)
}

pub(super) fn index_suffix(
    path: &Path,
    byte_offset: u64,
    start_line: u32,
    session_id: &str,
) -> Result<ParsedConversation, ConversationIndexIssue> {
    claude::index_suffix_as(path, byte_offset, start_line, session_id, Source::Qoder)
}

pub(super) fn index_cn(path: &Path) -> Result<ConversationIndexBatch, ConversationIndexIssue> {
    wrap_index(path, parse_cn)
}

pub(super) fn detail_cn(
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    single_detail(path, session_id, include_deferred_content, parse_cn)
}

pub(super) fn parse_cn(
    path: &Path,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    claude::parse_as(path, include_deferred_content, Source::QoderCn)
}

pub(super) fn index_suffix_cn(
    path: &Path,
    byte_offset: u64,
    start_line: u32,
    session_id: &str,
) -> Result<ParsedConversation, ConversationIndexIssue> {
    claude::index_suffix_as(path, byte_offset, start_line, session_id, Source::QoderCn)
}
