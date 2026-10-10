use serde::{Deserialize, Serialize};

pub use pricing::{CostSource, DerivedCost, PriceEntry, PriceOrigin, PriceTable};

/// 未定价诊断的原因分档。判定在聚合层完成：空模型名无法按价目表补单价。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnpricedReason {
    /// 模型名非空，但精确查价（model+provider，再 model 且 provider 为空）未命中。
    Pricable,
    /// 模型名为空。价目表以模型名为键，补单价也算不出费用。
    StructurallyUnbillable,
}

/// 全库未定价诊断的一组 `(模型, provider)`。
/// 已自带费用或已精确命中价目的部分不计入。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnpricedGroupDto {
    pub model: String,
    pub provider: String,
    pub sources: Vec<String>,
    pub total_tokens: i64,
    pub record_count: i64,
    pub reason: UnpricedReason,
    /// 可补组上的签名兼容候选；形状与价目条目一致，来源多为快照。精确已命中或完全对不上时为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<PriceEntry>,
}

/// 内置/可刷新的价目快照（当前来自 LiteLLM 社区维护的 `model_prices_and_context_window.json`）。
/// 作为「用户单价 + 来源自带费用」之外的兜底：只在某模型既无 native_cost、用户也未配置单价时启用，
/// 让费用从「用户手填才能算」变成「开箱大体准」。快照里的 `provider` 一律为空，充当按模型的兜底单价。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PriceSnapshot {
    pub as_of: String,
    pub source: String,
    pub entries: Vec<PriceEntry>,
}

/// 给界面展示的快照元信息（不含逐条单价）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceSnapshotMeta {
    pub as_of: String,
    pub source: String,
    pub count: usize,
    /// 是否为内置默认快照（`true`）还是用户联网刷新后的本地缓存（`false`）。
    pub bundled: bool,
}
