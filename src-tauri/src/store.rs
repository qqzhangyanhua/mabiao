pub const ADAPTER_VERSION: i64 = 9;

/// `user_version` 记账：1 = usage_records.model 已归一化成小写。
pub(crate) const LOWERCASE_MODEL_VERSION: i64 = 1;

mod connect;
pub mod cursor_account;
pub mod cursor_session;
pub mod official_quota;
pub mod records;
pub mod rollup;
mod schema;

pub use connect::{open_db, open_memory, open_readonly, shrink_memory};
pub use cursor_account::*;
pub use cursor_session::*;
pub use official_quota::*;
pub use records::*;
pub use rollup::*;
