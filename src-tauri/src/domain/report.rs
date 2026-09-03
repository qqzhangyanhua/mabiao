use super::overview::OverviewDto;
use serde::{Deserialize, Serialize};

/// 报告要取的自然周期。`offset = 0` 是最近一个已经结束的完整周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportPeriod {
    pub kind: ReportPeriodKind,
    pub offset: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportPeriodKind {
    Week,
    Month,
}

/// 报告入口 DTO。总量只来自消耗记录；洞察 payload 不含自然语言。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportDto {
    pub period_kind: ReportPeriodKind,
    pub offset: u32,
    /// 周期第一天（本地日历日，含）。
    pub start_date: String,
    /// 周期最后一天（本地日历日，含）。
    pub end_date: String,
    pub has_data: bool,
    pub totals: OverviewDto,
    /// 周期内每个本地日历日一根柱，零也是 0，不是缺省。
    pub days: Vec<ReportDayPoint>,
    /// 来源 token 占比。只含 token > 0 的来源；`pct` 已是整数，前端不重算。
    pub sources: Vec<ReportShareSlice>,
    /// 按 token 降序最多三条模型名；不足三条有几条列几条。
    pub models: Vec<String>,
    pub insights: Vec<ReportInsight>,
}

/// 报告占比条上的一段。`name` 是来源标识；`pct` 是整数百分比。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportShareSlice {
    pub name: String,
    pub pct: i64,
}

/// 报告按天序列上的一天。`date` 是本地日历日 `YYYY-MM-DD`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportDayPoint {
    pub date: String,
    pub total_tokens: i64,
}

/// 报告洞察。`kind` + 数值/标识符，不含文案。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportInsight {
    NightShare {
        night_tokens: i64,
        total_tokens: i64,
        pct: i64,
    },
    PeakHours {
        start_hour: u8,
        end_hour: u8,
    },
    /// `weekday`：0 = 周一 … 6 = 周日。
    BusiestDay {
        weekday: u8,
    },
    TopSession {
        by: ReportTopSessionBy,
        source: String,
        session_id: String,
        project: Option<String>,
        cost: Option<f64>,
        total_tokens: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportTopSessionBy {
    Cost,
    Tokens,
}
