//! 活跃会话的增量索引判定：文件只变长时解析后缀，否则整会话重索引。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConversationFileIndexPlan {
    Skip,
    Incremental,
    Full,
}

#[derive(Debug, Clone)]
pub(crate) struct ConversationFileFingerprint {
    pub mtime_ns: i64,
    pub size: i64,
    pub revision: String,
    pub indexed_byte_offset: i64,
    pub has_live_generation: bool,
}

pub(crate) fn plan_conversation_file_index(
    cached: Option<&ConversationFileFingerprint>,
    current_mtime_ns: i64,
    current_size: i64,
    current_revision: &str,
    supports_incremental: bool,
) -> ConversationFileIndexPlan {
    let Some(cached) = cached else {
        return ConversationFileIndexPlan::Full;
    };
    if cached.mtime_ns == current_mtime_ns
        && cached.size == current_size
        && cached.revision == current_revision
    {
        return ConversationFileIndexPlan::Skip;
    }
    if supports_incremental
        && cached.has_live_generation
        && cached.indexed_byte_offset > 0
        && current_size > cached.size
        && current_mtime_ns >= cached.mtime_ns
    {
        return ConversationFileIndexPlan::Incremental;
    }
    ConversationFileIndexPlan::Full
}

pub(crate) fn new_events_precede_existing(
    existing_max_occurred_at: Option<&str>,
    new_occurred_at: impl IntoIterator<Item = Option<String>>,
) -> bool {
    let Some(existing_max) = existing_max_occurred_at else {
        return false;
    };
    new_occurred_at
        .into_iter()
        .flatten()
        .any(|occurred_at| super::toolbox::compare_timestamps(&occurred_at, existing_max).is_lt())
}
