//! 对话记录事件存储（Conversation Record Event Store）的统一内部边界。
//!
//! 集中管理：
//! - 会话事件索引就绪态（适配器版本、已发布代次、待补建脏标与磁盘修订号一致）
//! - 权威事件读取（整场、分页、单条事件、计数与导出流式读取，按 `ReadOptions` 控制延迟正文展开与回退策略）
//! - ADR-0025 外置正文回水与推导 `event_id` 压缩/还原
//! - 全量刷新、未变子集重解析、后缀增量追加、目录待补建刷新与后台补建的统一原子提交

mod incremental;
mod query;
mod read;
mod readiness;
mod storage;
mod tables;
mod write;

pub(crate) use read::{
    catalog_event_count, finish_prepared_events, load_session_events, prepare_events_read,
    PreparedEventsRead,
};
pub(in crate::conversation) use read::{
    indexed_event_count, read_event_by_id, read_indexed_event_if_ready, read_session_events,
    stream_events_for_export, ExportRecord, ReadOptions,
};
pub use read::{indexed_events, load_events};

pub(crate) use readiness::event_index_ready;
pub(in crate::conversation) use readiness::{
    event_index_ready_sql, has_live_generation, session_generation,
};

pub use storage::adopt_text_references;
pub(in crate::conversation) use storage::{
    referenced_text, restore_event_id, ReferencedLine, TextRef,
};

pub(in crate::conversation) use incremental::{
    commit_incremental_append, prepare_incremental, IncrementalPrepare, PendingIncremental,
};
pub(in crate::conversation) use write::{
    clear_session_events, commit_session, with_savepoint, SessionCommit, SessionEventStager,
};
