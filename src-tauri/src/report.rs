//! 报告：已结束自然周期内、仅基于消耗记录的可分享汇总。
//! 洞察规则在本模块；前端只做措辞与排版（ADR 0015）。

use chrono::{DateTime, Datelike, Duration, Local, Months, NaiveDate};
use rusqlite::{params_from_iter, Connection};

use crate::domain::{
    Filter, NamedAmount, PriceTable, ReportDayPoint, ReportDto, ReportInsight, ReportPeriod,
    ReportPeriodKind, ReportShareSlice,
};
use crate::query;
use crate::rollup_source::rollup_source;
use crate::rollup_split::rollup_plan;
use crate::store;
use crate::work_timeline;

/// 报告模块的单一入口。`now` 由调用方注入，便于用例固定本地时刻。
pub fn build(
    conn: &Connection,
    prices: &PriceTable,
    period: ReportPeriod,
    now: DateTime<Local>,
) -> Result<ReportDto, String> {
    let (start, end_exclusive) = resolve_period(period, now.date_naive())?;
    let end_inclusive = end_exclusive - Duration::days(1);
    let filter = period_filter(start, end_exclusive);
    let totals = query::overview(conn, &filter, prices)?;
    let record_count = usage_record_count(conn, &filter)?;
    let has_data = record_count > 0;
    let days = complete_days(
        start,
        end_inclusive,
        &query::tokens_by_local_day(conn, &filter)?,
    );
    let insights = if has_data {
        period_insights(&query::hour_of_day(conn, &filter)?, start, &days)
    } else {
        Vec::new()
    };
    let sources = share_slices(&query::breakdown(conn, &filter, prices, "source")?);
    let models = top_models(&query::breakdown(conn, &filter, prices, "model")?);
    Ok(ReportDto {
        period_kind: period.kind,
        offset: period.offset,
        start_date: start.format("%Y-%m-%d").to_string(),
        end_date: end_inclusive.format("%Y-%m-%d").to_string(),
        has_data,
        totals,
        days,
        sources,
        models,
        insights,
    })
}

fn share_slices(rows: &[NamedAmount]) -> Vec<ReportShareSlice> {
    let rows: Vec<&NamedAmount> = rows.iter().filter(|row| row.total_tokens > 0).collect();
    let tokens: Vec<i64> = rows.iter().map(|row| row.total_tokens).collect();
    integer_pcts(&tokens)
        .into_iter()
        .zip(rows)
        .map(|(pct, row)| ReportShareSlice {
            name: row.name.clone(),
            pct,
        })
        .collect()
}

fn top_models(rows: &[NamedAmount]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.total_tokens > 0)
        .take(3)
        .map(|row| row.name.clone())
        .collect()
}

fn integer_pcts(tokens: &[i64]) -> Vec<i64> {
    let n = tokens.len();
    if n == 0 {
        return Vec::new();
    }
    let total: i64 = tokens.iter().sum();
    if total <= 0 {
        return vec![0; n];
    }
    if n == 1 {
        return vec![100];
    }
    let mut floors = Vec::with_capacity(n);
    let mut remainders = Vec::with_capacity(n);
    let mut used = 0i64;
    for (index, &token) in tokens.iter().enumerate() {
        let exact = token as f64 * 100.0 / total as f64;
        let floor = exact.floor() as i64;
        used += floor;
        floors.push(floor);
        remainders.push((exact - floor as f64, index));
    }
    remainders.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let mut leftover = 100 - used;
    for (_, index) in remainders {
        if leftover <= 0 {
            break;
        }
        floors[index] += 1;
        leftover -= 1;
    }
    floors
}

fn period_insights(
    hours: &[i64; 24],
    start: NaiveDate,
    days: &[ReportDayPoint],
) -> Vec<ReportInsight> {
    let mut insights = schedule_insights(hours);
    insights.push(ReportInsight::BusiestDay {
        weekday: busiest_weekday(start, days),
    });
    insights
}

fn complete_days(
    start: NaiveDate,
    end_inclusive: NaiveDate,
    sparse: &[(String, i64)],
) -> Vec<ReportDayPoint> {
    let lookup: std::collections::BTreeMap<&str, i64> = sparse
        .iter()
        .map(|(date, tokens)| (date.as_str(), *tokens))
        .collect();
    let mut days = Vec::new();
    let mut day = start;
    while day <= end_inclusive {
        let date = day.format("%Y-%m-%d").to_string();
        days.push(ReportDayPoint {
            total_tokens: lookup.get(date.as_str()).copied().unwrap_or(0),
            date,
        });
        day += Duration::days(1);
    }
    days
}

fn busiest_weekday(start: NaiveDate, days: &[ReportDayPoint]) -> u8 {
    assert!(!days.is_empty(), "有数据时按天序列必须覆盖周期内每一天");
    let mut best_index = 0usize;
    let mut best_tokens = i64::MIN;
    for (index, point) in days.iter().enumerate() {
        if point.total_tokens > best_tokens {
            best_tokens = point.total_tokens;
            best_index = index;
        }
    }
    let weekday = i64::from(start.weekday().num_days_from_monday()) + best_index as i64;
    (weekday.rem_euclid(7)) as u8
}

fn schedule_insights(hours: &[i64; 24]) -> Vec<ReportInsight> {
    let total_tokens: i64 = hours.iter().sum();
    let night_tokens: i64 = hours[..6].iter().sum();
    let start_hour = peak_start_hour(hours);
    vec![
        ReportInsight::NightShare {
            night_tokens,
            total_tokens,
            pct: night_pct(night_tokens, total_tokens),
        },
        ReportInsight::PeakHours {
            start_hour,
            end_hour: (start_hour + 4) % 24,
        },
    ]
}

fn night_pct(night_tokens: i64, total_tokens: i64) -> i64 {
    if night_tokens <= 0 || total_tokens <= 0 {
        return 0;
    }
    if night_tokens >= total_tokens {
        return 100;
    }
    let rounded = ((night_tokens as f64) * 100.0 / (total_tokens as f64)).round() as i64;
    rounded.clamp(1, 99)
}

fn peak_start_hour(hours: &[i64; 24]) -> u8 {
    let window = |start: u8| -> i64 {
        (0..4)
            .map(|offset| hours[usize::from((start + offset) % 24)])
            .sum()
    };
    let mut best_start = 0u8;
    let mut best_sum = window(0);
    for start in 1..24u8 {
        let sum = window(start);
        if sum > best_sum {
            best_sum = sum;
            best_start = start;
        }
    }
    best_start
}

fn resolve_period(
    period: ReportPeriod,
    today: NaiveDate,
) -> Result<(NaiveDate, NaiveDate), String> {
    match period.kind {
        ReportPeriodKind::Week => {
            let weekday = i64::from(today.weekday().num_days_from_monday());
            let current_week_start = today - Duration::days(weekday);
            let start = current_week_start
                .checked_sub_signed(Duration::days(7 * (i64::from(period.offset) + 1)))
                .ok_or_else(|| "周周期超出范围".to_string())?;
            let end = start
                .checked_add_signed(Duration::days(7))
                .ok_or_else(|| "周周期超出范围".to_string())?;
            Ok((start, end))
        }
        ReportPeriodKind::Month => {
            let current_month_start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
                .ok_or_else(|| "无法解析当前月".to_string())?;
            let start = current_month_start
                .checked_sub_months(Months::new(period.offset + 1))
                .ok_or_else(|| "月周期超出范围".to_string())?;
            let end = current_month_start
                .checked_sub_months(Months::new(period.offset))
                .ok_or_else(|| "月周期超出范围".to_string())?;
            Ok((start, end))
        }
    }
}

fn usage_record_count(conn: &Connection, filter: &Filter) -> Result<i64, String> {
    let inner = rollup_source(
        &rollup_plan(
            filter.from.as_deref(),
            filter.to.as_deref(),
            store::rollup_is_ready(conn),
            None,
        ),
        filter,
    );
    let sql = format!(
        "SELECT COALESCE(SUM(d.record_count), 0) FROM ({}) d",
        inner.sql
    );
    conn.query_row(&sql, params_from_iter(inner.params.iter()), |row| {
        row.get(0)
    })
    .map_err(|e| e.to_string())
}

/// 本地半开区间 `[start, end)`，转换成 Filter 的闭区间（`to` 含最后一毫秒）。
fn period_filter(start: NaiveDate, end_exclusive: NaiveDate) -> Filter {
    let from = work_timeline::local_midnight_utc(start);
    let to = work_timeline::local_midnight_utc(end_exclusive) - Duration::milliseconds(1);
    Filter {
        from: Some(work_timeline::rfc3339_millis(from)),
        to: Some(work_timeline::rfc3339_millis(to)),
        sources: Vec::new(),
        models: Vec::new(),
        projects: Vec::new(),
        providers: Vec::new(),
    }
}
