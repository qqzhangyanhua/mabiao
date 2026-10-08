pub const ADAPTER_VERSION: i64 = 9;

/// `user_version` 记账：1 = usage_records.model 已归一化成小写。
pub(crate) const LOWERCASE_MODEL_VERSION: i64 = 1;
/// `user_version` 记账：2 = 正文倒排的维护已改为整份重灌，老库的维护基准已清空过一次。
pub(crate) const FTS_REBUILD_VERSION: i64 = 2;

mod connect;
pub(crate) mod conversation_fts;
pub mod cursor_account;
pub mod cursor_session;
pub mod official_quota;
pub mod records;
pub mod rollup;
mod schema;

pub use connect::{open_db, open_memory, open_readonly, shrink_memory, vacuum};
pub(crate) use conversation_fts::{
    conversation_fts_needs_migration, conversation_fts_needs_optimize, database_vacuum_is_due,
    measure_conversation_index_bytes, merge_conversation_fts_step, migrate_conversation_events_fts,
    rebuild_conversation_fts, store_conversation_index_bytes, stored_conversation_index_bytes,
};
pub use cursor_account::*;
pub use cursor_session::*;
pub use official_quota::*;
pub use records::*;
pub use rollup::*;
pub(crate) use schema::{
    conversation_events_needs_layout_migration, conversation_session_tools_sql,
    migrate_conversation_events_layout, CONVERSATION_EVENT_COLUMN_LIST,
};
