//! 码表的计价规则（ADR 0026「代码布局」）。
//!
//! 优先级：来源自带 `native_cost` > 价目 model+provider 精确匹配 > model 兜底
//! （provider 为空的条目，含 LiteLLM 快照）> 未定价。桌面端 `cost.rs` 与远程服务的团队价目重算
//! 共用这一份，避免第三处实现漂移。只含规则与匹配，不依赖 sqlite / Tauri。
//!
//! `aggregate.rs` 与 `query.rs` 各有一份 SQL / 内存聚合，语义必须与这里一致，
//! 由桌面端 `cargo test parity` 守住。

mod apply;
mod lookup;
mod types;

pub use apply::{
    apply_entry, apply_entry_attribution, derive_priced, price_usage, CostAttribution, PricedCost,
    PricedUsage, PricingBasis,
};
pub use lookup::{find_price, find_price_by_signature, resolve_entry, PriceCache};
pub use types::{CostSource, DerivedCost, PriceEntry, PriceOrigin, PriceTable};
