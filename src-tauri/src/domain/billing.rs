use serde::{Deserialize, Serialize};

/// 当前自然月的预算执行情况：仅本地估算，非官方账单，用于阈值提醒与设置页展示。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetStatusDto {
    pub monthly_budget: Option<f64>,
    pub month: String,
    pub days_elapsed: i64,
    pub days_in_month: i64,
    pub month_to_date_cost: f64,
    pub unpriced: bool,
    pub projected_month_cost: Option<f64>,
    pub percent_used: Option<f64>,
    pub percent_projected: Option<f64>,
    pub thresholds: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BurnRateDto {
    pub tokens_per_minute: f64,
    pub cost_per_hour: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionDto {
    pub total_tokens: i64,
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BillingWindowDto {
    pub source: String,
    pub application: String,
    pub start: String,
    pub end: String,
    pub last_activity: String,
    pub is_active: bool,
    pub elapsed_minutes: i64,
    pub remaining_minutes: Option<i64>,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub reasoning_tokens: i64,
    pub session_count: i64,
    pub cost: Option<f64>,
    pub unpriced: bool,
    pub burn: Option<BurnRateDto>,
    pub projection: Option<ProjectionDto>,
}

/// 按来源统计的 7 天滚动窗口：不像 5 小时窗那样按活动间隔切块，而是持续滚动的
/// "最近 N 天用了多少"，贴近 Claude 等工具的周度限额心智模型（仅本地估计，非官方配额）。
/// `source=cursor` 是例外：来自 Cursor 账号用量缓存，不进 `UsageRecord` / 5 小时窗。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeeklyWindowDto {
    pub source: String,
    pub application: String,
    pub window_days: i64,
    pub start: String,
    pub end: String,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub reasoning_tokens: i64,
    pub session_count: i64,
    pub cost: Option<f64>,
    pub unpriced: bool,
    pub daily_average_tokens: f64,
    pub daily_average_cost: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BillingWindowsDto {
    pub now: String,
    pub window_hours: i64,
    pub current: Vec<BillingWindowDto>,
    pub recent: Vec<BillingWindowDto>,
    pub weekly_window_days: i64,
    pub weekly: Vec<WeeklyWindowDto>,
}
