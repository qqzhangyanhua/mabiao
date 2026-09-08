use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};

use crate::domain::{Filter, WorkNotesRange, WorkNotesRangeKind};
use crate::work_timeline;

const CUSTOM_MAX_DAYS: i64 = 31;

pub struct ResolvedRange {
    pub kind: WorkNotesRangeKind,
    pub start_date: String,
    pub end_date: String,
    pub from: String,
    pub to: String,
}

pub fn resolve(range: &WorkNotesRange, now: DateTime<Local>) -> Result<ResolvedRange, String> {
    match range.kind {
        WorkNotesRangeKind::Today => {
            Ok(until_now(WorkNotesRangeKind::Today, now.date_naive(), now))
        }
        WorkNotesRangeKind::ThisWeek => {
            let today = now.date_naive();
            let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
            Ok(until_now(WorkNotesRangeKind::ThisWeek, monday, now))
        }
        WorkNotesRangeKind::ThisMonth => {
            let today = now.date_naive();
            let start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
                .ok_or_else(|| "无法解析本月起始日".to_string())?;
            Ok(until_now(WorkNotesRangeKind::ThisMonth, start, now))
        }
        WorkNotesRangeKind::Custom => resolve_custom(range, now),
    }
}

pub fn usage_filter(range: &ResolvedRange) -> Filter {
    Filter {
        from: Some(range.from.clone()),
        to: Some(range.to.clone()),
        sources: Vec::new(),
        models: Vec::new(),
        projects: Vec::new(),
        providers: Vec::new(),
    }
}

fn until_now(kind: WorkNotesRangeKind, start: NaiveDate, now: DateTime<Local>) -> ResolvedRange {
    let today = now.date_naive();
    ResolvedRange {
        kind,
        start_date: start.format("%Y-%m-%d").to_string(),
        end_date: today.format("%Y-%m-%d").to_string(),
        from: work_timeline::rfc3339_millis(work_timeline::local_midnight_utc(start)),
        to: work_timeline::rfc3339_millis(now.with_timezone(&Utc)),
    }
}

fn resolve_custom(range: &WorkNotesRange, now: DateTime<Local>) -> Result<ResolvedRange, String> {
    let today = now.date_naive();
    let start = parse_iso_date(
        range
            .from
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or("自定义区间缺少起始日")?,
    )?;
    let end_inclusive = parse_iso_date(
        range
            .to
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or("自定义区间缺少结束日")?,
    )?;
    if start > today {
        return Err("起始日不能晚于今天".to_string());
    }
    if end_inclusive > today {
        return Err("结束日不能晚于今天".to_string());
    }
    if start > end_inclusive {
        return Err("起始日不能晚于结束日".to_string());
    }
    let days = (end_inclusive - start).num_days() + 1;
    if days > CUSTOM_MAX_DAYS {
        return Err(format!("自定义区间最多 {CUSTOM_MAX_DAYS} 天，请收窄后再试"));
    }
    let to = if end_inclusive == today {
        work_timeline::rfc3339_millis(now.with_timezone(&Utc))
    } else {
        let end_exclusive = end_inclusive
            .checked_add_signed(Duration::days(1))
            .ok_or_else(|| "区间超出范围".to_string())?;
        work_timeline::rfc3339_millis(
            work_timeline::local_midnight_utc(end_exclusive) - Duration::milliseconds(1),
        )
    };
    Ok(ResolvedRange {
        kind: WorkNotesRangeKind::Custom,
        start_date: start.format("%Y-%m-%d").to_string(),
        end_date: end_inclusive.format("%Y-%m-%d").to_string(),
        from: work_timeline::rfc3339_millis(work_timeline::local_midnight_utc(start)),
        to,
    })
}

fn parse_iso_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| format!("无法解析日期：{value}"))
}
