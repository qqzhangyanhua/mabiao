use super::price::CostSource;
use serde::{Deserialize, Serialize};

/// 同一 `(source, session_id)` 若已有消耗记录，与记录区间取并集。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkSessionSpan {
    pub source: String,
    pub session_id: String,
    pub project: String,
    pub model: String,
    pub started_at: String,
    pub ended_at: String,
}

/// 工作时间线里的一根横条：一条会话按当天本地日历日裁剪后的区间。
/// `total_tokens` 只统计该会话落在这天的消耗记录，不是会话全量；Cursor 本机会话无记录时为 0。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkSegment {
    pub session_id: String,
    pub source: String,
    pub project: String,
    pub model: String,
    pub start: String,
    pub end: String,
    pub total_tokens: i64,
}

/// 单日工作时间线：与当天区间有交集的会话铺开成 `segments`。独立于本机 token KPI 的既有口径，
/// 不受顶栏范围筛选影响，只看 `day`（本地日历日 `YYYY-MM-DD`）这一天。
/// 消耗记录按 `occurred_at` 聚成横条；Cursor 本机会话按 `first_seen_at` / `last_seen_at` 并入。
/// 账号用量与代码量不进时间线。
///
/// * `turn_count` — 当天落点消耗记录数（按每条记录 `occurred_at` 归当天，一条记录 ≈ 一次模型调用）。
///   Cursor 本机会话不计入此项（轮次在会话维度，不能按日落点拆）。
/// * `ai_exec_minutes` — 各会话裁剪到当天后的区间长度之和（分钟，浮点保留小数）。
/// * `peak_parallel` — 当天任意时刻同时进行的会话数最大值（基于裁剪后区间扫描线）。
/// * `parallel_intensity` — 累计执行时长 ÷ 会话区间并集时长；无重叠为 1.0x；空日为 `None`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkTimelineDto {
    pub day: String,
    pub total_tokens: i64,
    pub segment_count: i64,
    pub turn_count: i64,
    pub ai_exec_minutes: f64,
    pub peak_parallel: i64,
    pub parallel_intensity: Option<f64>,
    pub segments: Vec<WorkSegment>,
}

impl WorkTimelineDto {
    pub fn empty(day: &str) -> Self {
        Self {
            day: day.to_string(),
            total_tokens: 0,
            segment_count: 0,
            turn_count: 0,
            ai_exec_minutes: 0.0,
            peak_parallel: 0,
            parallel_intensity: None,
            segments: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnRow {
    pub occurred_at: String,
    pub model: String,
    pub provider: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
    pub source_file: String,
    pub cost: Option<f64>,
    pub unpriced: bool,
    /// 本轮费用来自哪一层：来源自带 / 用户单价 / LiteLLM 快照 / 未配置。
    pub cost_source: CostSource,
    pub cost_note: Option<String>,
}
