use serde::{Deserialize, Serialize};

use super::overview::Filter;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeriesPoint {
    pub bucket: String,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub reasoning_tokens: i64,
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedAmount {
    pub name: String,
    pub total_tokens: i64,
    pub share: f64,
    pub cost: Option<f64>,
    pub unpriced: bool,
}

/// Provider / 模型等聚合页下的单条消耗记录（一次调用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageCallRow {
    pub occurred_at: String,
    pub source: String,
    pub model: String,
    pub provider: String,
    pub project: String,
    pub session_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
    pub cost: Option<f64>,
    pub unpriced: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageCallPage {
    pub rows: Vec<UsageCallRow>,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EfficiencyMetrics {
    pub total_tokens: i64,
    pub session_count: i64,
    pub cache_hit_rate: Option<f64>,
    pub average_session_tokens: Option<f64>,
    pub reasoning_share: Option<f64>,
}

/// 缓存命中率：`cache_read / (input + cache_read)`。
///
/// 读、写都是 0 时视为没有缓存口径，返回 `None`（界面「无法计算」，不是 0%）。
/// 只在同一来源内比较；Grok / Codex 的 input 含不含 cache 不同，不能跨来源排名。
pub fn cache_hit_rate(cache_read: i64, cache_creation: i64, input: i64) -> Option<f64> {
    if cache_read <= 0 && cache_creation <= 0 {
        return None;
    }
    let denominator = input + cache_read;
    if denominator <= 0 {
        None
    } else {
        Some(cache_read as f64 / denominator as f64)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplicationEfficiency {
    pub source: String,
    pub application: String,
    pub metrics: EfficiencyMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplicationTrendPoint {
    pub bucket: String,
    pub total_tokens: i64,
    pub values: std::collections::BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectApplicationRow {
    pub project: String,
    pub total_tokens: i64,
    pub values: std::collections::BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplicationAnalyticsDto {
    pub summary: EfficiencyMetrics,
    pub by_application: Vec<ApplicationEfficiency>,
    pub trend: Vec<ApplicationTrendPoint>,
    pub projects: Vec<ProjectApplicationRow>,
}

/// 某一来源命中率最低的会话，供来源页下钻进对话记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LowCacheHitSessionRow {
    pub session_id: String,
    pub source: String,
    pub project: String,
    pub model: String,
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
    pub cache_hit_rate: Option<f64>,
    pub started_at: String,
    pub ended_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LowCacheHitSessionsDto {
    pub source: String,
    /// 该来源在当前筛选下能否算出命中率。false 时界面显示「无法计算」，`rows` 为空。
    pub computable: bool,
    pub rows: Vec<LowCacheHitSessionRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRow {
    pub session_id: String,
    pub source: String,
    pub project: String,
    pub model: String,
    pub total_tokens: i64,
    pub started_at: String,
    pub ended_at: String,
    pub source_file: String,
    pub cost: Option<f64>,
    pub unpriced: bool,
}

/// 会话列表分页查询参数：搜索/排序/分页均下沉到 SQL 层执行。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionQuery {
    pub filter: Filter,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub sort_by: Option<String>,
    #[serde(default)]
    pub sort_dir: Option<String>,
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub page_size: Option<u32>,
    /// 列表与导出都可打开；打开后对聚合结果做价目 JOIN。
    #[serde(default)]
    pub include_cost: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionPage {
    pub rows: Vec<SessionRow>,
    pub total: u32,
    pub total_tokens: i64,
    pub last_ended: Option<String>,
}
