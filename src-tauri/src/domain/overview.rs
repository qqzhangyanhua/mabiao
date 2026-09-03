use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub from: Option<String>,
    pub to: Option<String>,
    pub sources: Vec<String>,
    pub models: Vec<String>,
    pub projects: Vec<String>,
    #[serde(default)]
    pub providers: Vec<String>,
}

/// 用户单价 / LiteLLM 快照按 token 口径拆出的费用。来源自带整笔不进这四档。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct OverviewCostBreakdown {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_creation: Option<f64>,
}

/// 概览费用按来源分层：金额给已计价三档，未配置只计记录条数。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct OverviewCostSources {
    pub native: Option<f64>,
    pub user: Option<f64>,
    pub snapshot: Option<f64>,
    pub unpriced_records: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverviewDto {
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub reasoning_tokens: i64,
    pub session_count: i64,
    pub cost: Option<f64>,
    pub unpriced: bool,
    pub cost_breakdown: OverviewCostBreakdown,
    pub cost_sources: OverviewCostSources,
}
