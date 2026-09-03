//! 报告：已结束自然周期内、仅基于消耗记录的可分享汇总。
//! 洞察规则在本模块；前端只做措辞与排版（ADR 0015）。

use chrono::{DateTime, Datelike, Duration, Local, Months, NaiveDate};
use rusqlite::{params_from_iter, Connection};

use crate::domain::{Filter, PriceTable, ReportDto, ReportPeriod, ReportPeriodKind};
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
    Ok(ReportDto {
        period_kind: period.kind,
        offset: period.offset,
        start_date: start.format("%Y-%m-%d").to_string(),
        end_date: end_inclusive.format("%Y-%m-%d").to_string(),
        has_data: record_count > 0,
        totals,
        insights: Vec::new(),
    })
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
