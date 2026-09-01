//! 时间窗相对预聚合表的切分判定。
//!
//! 只做决策，不拼 SQL、不读库。三种形态：纯明细、纯预聚合、切分
//! （中间完整 UTC 天 + 两端 partial 区间，对齐端点时对应 partial 为空）。

use chrono::{DateTime, Duration, NaiveDate, Timelike, Utc};

/// 查询相对预聚合表的走法。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollupPlan {
    /// 整段走消耗记录明细。
    Raw,
    /// 整段走预聚合表（无时间窗）。
    Rollup,
    /// 中间完整 UTC 天走预聚合，两端 partial 走明细。
    Split(RollupSplit),
}

/// 切分结果。`complete_*` 是 UTC 日戳 `YYYY-MM-DD` 的半开区间 `[from, to)`；
/// None 表示该侧开放。partial 的日戳端点与对应 `complete_*` 相同，供明细侧
/// 做 ISO 前缀比较（`occurred_at < day` / `occurred_at >= day`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollupSplit {
    pub complete_from: Option<String>,
    pub complete_to: Option<String>,
    pub head: Option<PartialRange>,
    pub tail: Option<PartialRange>,
}

/// 一端 partial 区间。
///
/// - 头部：`[from, to)`，`from` 是原始窗起点，`to` 是下一个 UTC 日戳（不含）。
/// - 尾部：`[from, to]`，`from` 是该 UTC 日戳（含），`to` 是原始窗终点（含）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialRange {
    pub from: String,
    pub to: String,
}

/// 根据时间窗两端、预聚合就绪状态和最小粒度，判定走哪一种形态。
pub fn rollup_plan(
    from: Option<&str>,
    to: Option<&str>,
    rollup_ready: bool,
    grain: Option<&str>,
) -> RollupPlan {
    if !rollup_ready {
        return RollupPlan::Raw;
    }
    // 小时桶无法从日级预聚合还原，优先于「无时间窗走纯预聚合」。
    if grain == Some("hour") {
        return RollupPlan::Raw;
    }
    if from.is_none() && to.is_none() {
        return RollupPlan::Rollup;
    }

    let from_at = match parse_bound(from) {
        Some(value) => value,
        None => return RollupPlan::Raw,
    };
    let to_at = match parse_bound(to) {
        Some(value) => value,
        None => return RollupPlan::Raw,
    };

    let complete_from = from_at.map(ceil_utc_day);
    let complete_to = to_at.map(floor_utc_day);
    if !has_complete_days(complete_from, complete_to) {
        return RollupPlan::Raw;
    }

    let complete_from = complete_from.map(format_day);
    let complete_to = complete_to.map(format_day);
    let head = match (from, from_at, complete_from.as_deref()) {
        (Some(original), Some(at), Some(day)) if !is_utc_midnight(at) => Some(PartialRange {
            from: original.to_string(),
            to: day.to_string(),
        }),
        _ => None,
    };
    let tail = match (to, to_at, complete_to.as_deref()) {
        (Some(original), Some(at), Some(day)) if !is_utc_midnight(at) => Some(PartialRange {
            from: day.to_string(),
            to: original.to_string(),
        }),
        _ => None,
    };

    RollupPlan::Split(RollupSplit {
        complete_from,
        complete_to,
        head,
        tail,
    })
}

fn parse_bound(value: Option<&str>) -> Option<Option<DateTime<Utc>>> {
    match value {
        None => Some(None),
        Some(raw) => DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|dt| Some(dt.with_timezone(&Utc))),
    }
}

fn has_complete_days(from: Option<NaiveDate>, to: Option<NaiveDate>) -> bool {
    match (from, to) {
        (Some(from), Some(to)) => from < to,
        (None, None) => false,
        _ => true,
    }
}

fn ceil_utc_day(at: DateTime<Utc>) -> NaiveDate {
    let day = at.date_naive();
    if is_utc_midnight(at) {
        day
    } else {
        day + Duration::days(1)
    }
}

fn floor_utc_day(at: DateTime<Utc>) -> NaiveDate {
    at.date_naive()
}

fn is_utc_midnight(at: DateTime<Utc>) -> bool {
    at.num_seconds_from_midnight() == 0 && at.nanosecond() == 0
}

fn format_day(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}
