//! SQL 下推的聚合查询：把原先「load_all 全量载入内存再聚合」改为在 sqlite 里
//! GROUP BY / 过滤，只返回聚合结果。费用通过临时价格表 `price_rows` LEFT JOIN 计算，
//! 与 `cost::derive_cost` 保持同一语义（native_cost 优先，其次 model+provider 匹配，
//! 再次 model 且 provider 为 NULL 的兜底，都没有则标记 unpriced；model/provider 大小写不敏感）。
//!
//! 高频聚合走统一子查询工厂：时间窗按 UTC 天切分，中间整天用 `usage_rollup`，
//! 两端 partial 用明细补差。无时间窗时整段走预聚合；小时粒度无法从日级还原，仍走明细。

pub mod analytics;
pub mod billing;
pub mod meta;
pub mod overview;
pub mod series;
pub mod sessions;
pub mod sql;
pub mod windows;

pub use analytics::*;
pub use billing::*;
pub use meta::*;
pub use overview::*;
pub use series::*;
pub use sessions::*;
pub(crate) use sql::usage_rollups_for_sessions;
pub use windows::*;
