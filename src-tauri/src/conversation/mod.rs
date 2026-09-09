use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::domain::{
    ConversationAgentRelations, ConversationSessionRow, CursorSessionRecord, Source, UsageRecord,
};

mod agent_graph;
pub(crate) mod attachments;
mod catalog;
mod catalog_search;
mod claude;
mod codex;
mod context_cache;
mod context_first_use;
mod context_manifest;
mod copilot;
mod cursor;
mod cursor_bridge;
mod cursor_inject;
mod discover;
mod droid;
mod dsh;
mod event_index;
mod event_page;
mod event_tables;
mod export;
mod gemini;
mod grok;
mod grok_inject;
mod hydrate;
mod incremental;
mod kimi;
mod line_direct;
mod merge;
mod omp;
mod opencode;
mod persist;
mod pi;
mod qwen;
mod read;
mod refresh;
mod session_store;
mod toolbox;
pub(crate) mod trusted_path;

use merge::merge_parsed_conversations;
use toolbox::{FileIndexCursor, ParsedConversation};

pub use catalog::{
    catalog_tool_names, indexed_events, sessions_page, sessions_page_with_prices,
    usage_records_page,
};
#[cfg(test)]
pub(crate) use context_manifest::assemble;
pub(crate) use discover::{
    detail_claude, detail_gemini, detail_omp, detail_pi, diagnostic_detail, diagnostic_index,
    discover_droid, discover_dsh, discover_extension, discover_gemini, discover_jsonl,
    discover_opencode, index_claude, index_gemini, index_omp, index_pi, regular_source_revision,
    single_detail,
};
pub(crate) use event_index::indexed_event_count;
pub(crate) use persist::{persist_session_file_cursors, write_session_file_events};
#[cfg(test)]
pub(crate) use read::read_consistent_snapshot;
pub use read::{
    backfill_event_index, backfill_event_index_step, detail_state, event_index_progress,
    load_attachment, load_attachment_thumbnail, load_detail, load_event_content,
    load_parsed_detail, parse_session_events, rebuild_events_from_line,
};
pub(crate) use read::{
    backfill_event_index_step_skipping, catalog_roots, event_index_ready, finish_prepared_detail,
    load_prepared_parsed, prepare_detail, prepare_detail_read,
};
pub use session_store::load_session;

pub use export::build_export;
#[cfg(test)]
pub(crate) use export::parsed_export;
pub(crate) use export::{export_default_name, write_conversation_export};

pub(super) const DEFAULT_PAGE_SIZE: u32 = 20;
pub(super) const MAX_PAGE_SIZE: u32 = 200;
pub(crate) const CONVERSATION_SOURCES: &[Source] = &[
    Source::Codex,
    Source::Claude,
    Source::CursorAgent,
    Source::Dsh,
    Source::Factory,
    Source::Kimi,
    Source::Grok,
    Source::Pi,
    Source::Omp,
    Source::Gemini,
    Source::Opencode,
    Source::Qwen,
    Source::Copilot,
];
pub(crate) const DETAIL_READ_ATTEMPTS: usize = 3;
pub(crate) const CONVERSATION_ADAPTER_VERSION: i64 = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationIndexIssue {
    pub path: String,
    pub message: String,
    pub event_type: Option<String>,
    pub line: Option<u64>,
}

pub(crate) struct CachedConversationFingerprint {
    session_id: String,
    source_file_mtime_ns: i64,
    source_file_size: i64,
    adapter_version: i64,
    source_revision: String,
    indexed_byte_offset: i64,
    indexed_line: i64,
    has_live_generation: bool,
}

pub(crate) struct SessionFileCursorWrite<'a> {
    path: &'a Path,
    cursor: FileIndexCursor,
    max_sequence: Option<u32>,
    mtime_ns: i64,
    size: i64,
    source_revision: &'a str,
}

pub(crate) struct ConversationIndexBatch {
    conversations: Vec<ParsedConversation>,
    diagnostics: Vec<ConversationIndexIssue>,
}

pub(crate) type ConversationDiscoverFn = fn(&[PathBuf]) -> Result<Vec<PathBuf>, String>;
pub(crate) type ConversationIndexFn =
    fn(&Path) -> Result<ConversationIndexBatch, ConversationIndexIssue>;
pub(crate) type ConversationIndexSuffixFn =
    fn(&Path, u64, u32, &str) -> Result<ParsedConversation, ConversationIndexIssue>;
pub(crate) type ConversationDetailFn = fn(&Path, &str, bool) -> Result<ParsedConversation, String>;
pub(crate) type ConversationRevisionFn = fn(&Path) -> Result<String, String>;

pub(crate) struct ConversationAdapter {
    source: Source,
    discover: ConversationDiscoverFn,
    index: ConversationIndexFn,
    /// 只解析后缀、不写库。未填则刷新时一律全量 index。
    index_suffix: Option<ConversationIndexSuffixFn>,
    detail: ConversationDetailFn,
    revision: ConversationRevisionFn,
    raw_extension: Option<&'static str>,
    reuse_unchanged_index: bool,
}

pub(crate) const CONVERSATION_ADAPTERS: &[ConversationAdapter] = &[
    ConversationAdapter {
        source: Source::Codex,
        discover: discover_jsonl,
        index: codex::index,
        index_suffix: Some(codex::index_suffix),
        detail: codex::detail,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Claude,
        discover: discover_jsonl,
        index: index_claude,
        index_suffix: None,
        detail: detail_claude,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::CursorAgent,
        discover: cursor::discover,
        index: cursor::index,
        index_suffix: None,
        detail: cursor::detail,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Dsh,
        discover: discover_dsh,
        index: dsh::index,
        index_suffix: None,
        detail: dsh::detail,
        revision: regular_source_revision,
        raw_extension: None,
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Factory,
        discover: discover_droid,
        index: droid::index,
        index_suffix: None,
        detail: droid::detail,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Kimi,
        discover: kimi::discover,
        index: kimi::index,
        index_suffix: None,
        detail: kimi::detail,
        revision: kimi::source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Grok,
        discover: grok::discover,
        index: grok::index,
        index_suffix: None,
        detail: grok::detail,
        revision: grok::source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Pi,
        discover: discover_jsonl,
        index: index_pi,
        index_suffix: None,
        detail: detail_pi,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Omp,
        discover: discover_jsonl,
        index: index_omp,
        index_suffix: None,
        detail: detail_omp,
        revision: omp::source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Gemini,
        discover: discover_gemini,
        index: index_gemini,
        index_suffix: None,
        detail: detail_gemini,
        revision: regular_source_revision,
        raw_extension: Some("json"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Opencode,
        discover: discover_opencode,
        index: opencode::index,
        index_suffix: None,
        detail: opencode::detail,
        revision: opencode::source_revision,
        raw_extension: None,
        reuse_unchanged_index: false,
    },
    ConversationAdapter {
        source: Source::Qwen,
        discover: qwen::discover,
        index: qwen::index,
        index_suffix: None,
        detail: qwen::detail,
        revision: regular_source_revision,
        raw_extension: Some("json"),
        reuse_unchanged_index: true,
    },
    ConversationAdapter {
        source: Source::Copilot,
        discover: copilot::discover,
        index: copilot::index,
        index_suffix: None,
        detail: copilot::detail,
        revision: regular_source_revision,
        raw_extension: Some("jsonl"),
        reuse_unchanged_index: true,
    },
];

pub(crate) fn conversation_adapter(source: Source) -> Result<&'static ConversationAdapter, String> {
    CONVERSATION_ADAPTERS
        .iter()
        .find(|adapter| adapter.source == source)
        .ok_or_else(|| "该来源尚未支持对话详情".to_string())
}

pub(super) fn raw_export_extension(source: Source) -> Result<Option<&'static str>, String> {
    Ok(conversation_adapter(source)?.raw_extension)
}

pub(crate) struct PreparedConversationDetail {
    source: Source,
    session: ConversationSessionRow,
    usage_records: Vec<UsageRecord>,
    agent_relations: ConversationAgentRelations,
    cursor_session_stats: Option<CursorSessionRecord>,
}

pub(crate) enum PreparedDetailRead {
    Indexed {
        prepared: PreparedConversationDetail,
        event_count: u32,
        observed_context: Vec<crate::domain::ConversationContextItem>,
        context_metrics: Option<context_cache::CachedContextMetrics>,
        first_use_events: Vec<context_first_use::Candidate>,
    },
    Parsed {
        prepared: PreparedConversationDetail,
        context_metrics: Option<context_cache::CachedContextMetrics>,
    },
}

pub fn load_events(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    anchor: crate::domain::ConversationEventAnchor,
    limit: u32,
) -> Result<crate::domain::ConversationEventPage, String> {
    event_page::load_events(conn, home, source, session_id, anchor, limit)
}

pub(crate) fn prepare_events_read(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
    anchor: &crate::domain::ConversationEventAnchor,
    limit: u32,
) -> Result<event_page::PreparedEventsRead, String> {
    event_page::prepare_events_read(conn, home, source, session_id, anchor, limit)
}

pub(crate) fn finish_prepared_events(
    home: &Path,
    read: event_page::PreparedEventsRead,
    anchor: &crate::domain::ConversationEventAnchor,
    limit: u32,
) -> Result<crate::domain::ConversationEventPage, String> {
    event_page::finish_prepared_events(home, read, anchor, limit)
}

pub(crate) use incremental::{
    plan_conversation_file_index, ConversationFileFingerprint, ConversationFileIndexPlan,
};
pub(crate) use refresh::refresh;
pub use refresh::refresh_codex;

/// 本机 bench 用：只跑 Codex 对话整文件 index，返回事件数。
pub fn codex_index_for_bench(path: &Path) -> Result<usize, String> {
    match codex::index(path) {
        Ok(batch) => Ok(batch
            .conversations
            .iter()
            .map(|conversation| conversation.events.len())
            .sum()),
        Err(issue) => Err(issue.message),
    }
}

/// 本机 bench 用：只跑 Codex 对话后缀 index，返回新事件数。
pub fn codex_index_suffix_for_bench(
    path: &Path,
    byte_offset: u64,
    start_line: u32,
    expected_session_id: &str,
) -> Result<usize, String> {
    match codex::index_suffix(path, byte_offset, start_line, expected_session_id) {
        Ok(parsed) => Ok(parsed.events.len()),
        Err(issue) => Err(issue.message),
    }
}

pub(crate) fn parse_conversation_file(
    source: Source,
    path: &Path,
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    (conversation_adapter(source)?.detail)(path, session_id, include_deferred_content)
}

pub(super) fn parse_conversation_files(
    source: Source,
    paths: &[PathBuf],
    session_id: &str,
    include_deferred_content: bool,
) -> Result<ParsedConversation, String> {
    let parsed = paths
        .iter()
        .map(|path| parse_conversation_file(source, path, session_id, include_deferred_content))
        .collect::<Result<Vec<_>, _>>()?;
    if parsed.is_empty() {
        return Err("对话没有可读取的原始文件".to_string());
    }
    Ok(merge_parsed_conversations(parsed))
}
