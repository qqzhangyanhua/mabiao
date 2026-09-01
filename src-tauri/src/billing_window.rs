//! 按来源把消耗记录切成 5 小时计费窗，并计算燃烧速率与窗末预测。
//! 只使用本地 `occurred_at`，不读官方配额、不访问网络。

use chrono::{DateTime, Duration, NaiveDateTime, SecondsFormat, Timelike, Utc};
use std::collections::{BTreeMap, BTreeSet};

use crate::cost::{derive_cost, sum_cursor_event_costs};
use crate::domain::{
    BillingWindowDto, BillingWindowsDto, BurnRateDto, CursorUsageEvent, PriceTable, ProjectionDto,
    Source, UsageRecord, WeeklyWindowDto,
};

pub const WINDOW_HOURS: i64 = 5;
pub const LOOKBACK_DAYS: i64 = 14;
pub const RECENT_LIMIT: usize = 6;
/// 7 天滚动窗口：与官方「周配额」概念对齐，用来提前预警周度限额。
pub const WEEKLY_WINDOW_DAYS: i64 = 7;
/// 概览 7 天滚动里挂的 Cursor 账号用量行；不是 `Source`，也不进 5 小时窗。
pub const CURSOR_WEEKLY_SOURCE: &str = "cursor";
pub const CURSOR_WEEKLY_APPLICATION: &str = "Cursor";

/// SQL 下推后的计费事件：费用已在查询层算好，不再带回整行消耗记录。
pub struct BillingEvent {
    pub occurred_at: String,
    pub source: Source,
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

struct Timed<'a> {
    at: DateTime<Utc>,
    event: &'a BillingEvent,
}

/// `occurred_at < iso_day_end(day)` 覆盖该 UTC 日全部 ISO 时间戳，且能走索引。
/// 上界用 `~`（ASCII 126）因为当天时间戳第 11 位只会是 `T`（84）。
pub fn iso_day_end(day: &str) -> String {
    format!("{day}~")
}

pub fn lookback_date(now: DateTime<Utc>) -> String {
    (now - Duration::days(LOOKBACK_DAYS))
        .format("%Y-%m-%d")
        .to_string()
}

pub fn summarize<'a, I>(records: I, prices: &PriceTable, now: DateTime<Utc>) -> BillingWindowsDto
where
    I: IntoIterator<Item = &'a UsageRecord>,
{
    let events: Vec<BillingEvent> = records
        .into_iter()
        .map(|record| event_from_record(record, prices))
        .collect();
    summarize_events(&events, now)
}

fn event_from_record(record: &UsageRecord, prices: &PriceTable) -> BillingEvent {
    let derived = derive_cost(record, prices);
    BillingEvent {
        occurred_at: record.occurred_at.clone(),
        source: record.source,
        session_id: record.session_id.clone(),
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: record.cache_creation_tokens,
        reasoning_tokens: record.reasoning_tokens,
        total_tokens: record.total_tokens,
        cost: derived.amount,
        unpriced: derived.unpriced,
    }
}
pub fn summarize_events(events: &[BillingEvent], now: DateTime<Utc>) -> BillingWindowsDto {
    let lookback = now - Duration::days(LOOKBACK_DAYS);
    let mut by_source: BTreeMap<String, Vec<Timed<'_>>> = BTreeMap::new();
    for event in events {
        let Some(at) = parse_occurred_at(&event.occurred_at) else {
            continue;
        };
        if at < lookback {
            continue;
        }
        by_source
            .entry(event.source.as_str().to_string())
            .or_default()
            .push(Timed { at, event });
    }

    let weekly_start = now - Duration::days(WEEKLY_WINDOW_DAYS);
    let mut weekly: Vec<WeeklyWindowDto> = by_source
        .values()
        .filter_map(|entries| {
            let items: Vec<&Timed<'_>> = entries
                .iter()
                .filter(|entry| entry.at >= weekly_start)
                .collect();
            if items.is_empty() {
                None
            } else {
                Some(build_weekly_window(&items, weekly_start, now))
            }
        })
        .collect();
    weekly.sort_by_key(|window| std::cmp::Reverse(window.total_tokens));

    let window_len = Duration::hours(WINDOW_HOURS);
    let mut current = Vec::new();
    let mut recent = Vec::new();
    for (_source, mut entries) in by_source {
        entries.sort_by_key(|entry| entry.at);
        for window in split_windows(&entries, now, window_len) {
            if window.is_active {
                current.push(window);
            } else {
                recent.push(window);
            }
        }
    }

    current.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
    recent.sort_by(|a, b| b.start.cmp(&a.start));
    recent.truncate(RECENT_LIMIT);

    BillingWindowsDto {
        now: iso(now),
        window_hours: WINDOW_HOURS,
        current,
        recent,
        weekly_window_days: WEEKLY_WINDOW_DAYS,
        weekly,
    }
}

/// 把 Cursor 账号用量挂进 7 天滚动（不进 5 小时窗，也不改本机消耗记录）。
/// 费用：用户价目优先，否则 LiteLLM 快照按模型签名兜底（允许词序/后缀差异）。
pub fn attach_cursor_weekly(
    mut dto: BillingWindowsDto,
    events: &[CursorUsageEvent],
    prices: &PriceTable,
    now: DateTime<Utc>,
) -> BillingWindowsDto {
    dto.weekly
        .retain(|window| window.source != CURSOR_WEEKLY_SOURCE);
    if let Some(window) = weekly_from_cursor_events(events, prices, now) {
        dto.weekly.push(window);
        dto.weekly
            .sort_by_key(|window| std::cmp::Reverse(window.total_tokens));
    }
    dto
}

fn weekly_from_cursor_events(
    events: &[CursorUsageEvent],
    prices: &PriceTable,
    now: DateTime<Utc>,
) -> Option<WeeklyWindowDto> {
    let start = now - Duration::days(WEEKLY_WINDOW_DAYS);
    let items: Vec<&CursorUsageEvent> = events
        .iter()
        .filter(|event| parse_occurred_at(&event.occurred_at).is_some_and(|at| at >= start))
        .collect();
    if items.is_empty() {
        return None;
    }

    let mut total_tokens = 0;
    let mut input_tokens = 0;
    let mut output_tokens = 0;
    let mut cache_read_tokens = 0;
    let mut cache_creation_tokens = 0;
    for event in &items {
        total_tokens += event.total_tokens();
        input_tokens += event.input_tokens;
        output_tokens += event.output_tokens;
        cache_read_tokens += event.cache_read_tokens;
        cache_creation_tokens += event.cache_creation_tokens;
    }
    let (cost, unpriced) = sum_cursor_event_costs(&items, prices);
    let days = WEEKLY_WINDOW_DAYS as f64;

    Some(WeeklyWindowDto {
        source: CURSOR_WEEKLY_SOURCE.to_string(),
        application: CURSOR_WEEKLY_APPLICATION.to_string(),
        window_days: WEEKLY_WINDOW_DAYS,
        start: iso(start),
        end: iso(now),
        total_tokens,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        reasoning_tokens: 0,
        session_count: items.len() as i64,
        cost,
        unpriced,
        daily_average_tokens: total_tokens as f64 / days,
        daily_average_cost: cost.map(|amount| amount / days),
    })
}

fn fold_event_totals(items: &[&Timed<'_>]) -> (Source, TokenTotals, Option<f64>, bool, i64) {
    let source = items[0].event.source;
    let mut totals = TokenTotals::default();
    let mut cost_total = 0.0;
    let mut any_cost = false;
    let mut unpriced = false;
    let mut sessions = BTreeSet::new();
    for item in items {
        let event = item.event;
        totals.total_tokens += event.total_tokens;
        totals.input_tokens += event.input_tokens;
        totals.output_tokens += event.output_tokens;
        totals.cache_read_tokens += event.cache_read_tokens;
        totals.cache_creation_tokens += event.cache_creation_tokens;
        totals.reasoning_tokens += event.reasoning_tokens;
        sessions.insert((event.source.as_str(), event.session_id.as_str()));
        if let Some(amount) = event.cost {
            cost_total += amount;
            any_cost = true;
        }
        if event.unpriced {
            unpriced = true;
        }
    }
    (
        source,
        totals,
        if any_cost { Some(cost_total) } else { None },
        unpriced,
        sessions.len() as i64,
    )
}

#[derive(Default)]
struct TokenTotals {
    total_tokens: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    reasoning_tokens: i64,
}

fn build_weekly_window(
    items: &[&Timed<'_>],
    start: DateTime<Utc>,
    now: DateTime<Utc>,
) -> WeeklyWindowDto {
    let (source, totals, cost, unpriced, session_count) = fold_event_totals(items);
    let days = WEEKLY_WINDOW_DAYS as f64;

    WeeklyWindowDto {
        source: source.as_str().to_string(),
        application: source.application_name().to_string(),
        window_days: WEEKLY_WINDOW_DAYS,
        start: iso(start),
        end: iso(now),
        total_tokens: totals.total_tokens,
        input_tokens: totals.input_tokens,
        output_tokens: totals.output_tokens,
        cache_read_tokens: totals.cache_read_tokens,
        cache_creation_tokens: totals.cache_creation_tokens,
        reasoning_tokens: totals.reasoning_tokens,
        session_count,
        cost,
        unpriced,
        daily_average_tokens: totals.total_tokens as f64 / days,
        daily_average_cost: cost.map(|amount| amount / days),
    }
}

fn split_windows(
    entries: &[Timed<'_>],
    now: DateTime<Utc>,
    window_len: Duration,
) -> Vec<BillingWindowDto> {
    if entries.is_empty() {
        return Vec::new();
    }

    let mut blocks: Vec<(DateTime<Utc>, Vec<&Timed<'_>>)> = Vec::new();
    let mut start = floor_to_utc_hour(entries[0].at);
    let mut current: Vec<&Timed<'_>> = Vec::new();

    for entry in entries {
        if let Some(last) = current.last() {
            if entry.at - start > window_len || entry.at - last.at > window_len {
                blocks.push((start, std::mem::take(&mut current)));
                start = floor_to_utc_hour(entry.at);
            }
        } else {
            start = floor_to_utc_hour(entry.at);
        }
        current.push(entry);
    }
    if !current.is_empty() {
        blocks.push((start, current));
    }

    blocks
        .into_iter()
        .map(|(start, items)| build_window(start, &items, now, window_len))
        .collect()
}

fn build_window(
    start: DateTime<Utc>,
    items: &[&Timed<'_>],
    now: DateTime<Utc>,
    window_len: Duration,
) -> BillingWindowDto {
    let end = start + window_len;
    let first = items[0];
    let last = items[items.len() - 1];
    let last_activity = last.at;
    let is_active = now < end && now - last_activity < window_len;
    let (source, totals, cost, unpriced, session_count) = fold_event_totals(items);
    let elapsed_minutes = if is_active {
        (now - start).num_minutes().max(0)
    } else {
        (std::cmp::min(now, end) - start).num_minutes().max(0)
    };
    let remaining_minutes = if is_active {
        Some((end - now).num_minutes().max(0))
    } else {
        None
    };
    let burn = burn_rate(first.at, last_activity, totals.total_tokens, cost);
    let projection = if is_active {
        match (burn.as_ref(), remaining_minutes) {
            (Some(rate), Some(remaining)) => Some(project_usage(
                totals.total_tokens,
                cost,
                rate,
                remaining as f64,
            )),
            _ => None,
        }
    } else {
        None
    };

    BillingWindowDto {
        source: source.as_str().to_string(),
        application: source.application_name().to_string(),
        start: iso(start),
        end: iso(end),
        last_activity: iso(last_activity),
        is_active,
        elapsed_minutes,
        remaining_minutes,
        total_tokens: totals.total_tokens,
        input_tokens: totals.input_tokens,
        output_tokens: totals.output_tokens,
        cache_read_tokens: totals.cache_read_tokens,
        cache_creation_tokens: totals.cache_creation_tokens,
        reasoning_tokens: totals.reasoning_tokens,
        session_count,
        cost,
        unpriced,
        burn,
        projection,
    }
}

fn burn_rate(
    first: DateTime<Utc>,
    last: DateTime<Utc>,
    total_tokens: i64,
    cost: Option<f64>,
) -> Option<BurnRateDto> {
    let duration_minutes = (last - first).num_seconds() as f64 / 60.0;
    if duration_minutes <= 0.0 {
        return None;
    }
    Some(BurnRateDto {
        tokens_per_minute: total_tokens as f64 / duration_minutes,
        cost_per_hour: cost.map(|amount| amount / duration_minutes * 60.0),
    })
}

fn project_usage(
    used_tokens: i64,
    used_cost: Option<f64>,
    burn: &BurnRateDto,
    remaining_minutes: f64,
) -> ProjectionDto {
    ProjectionDto {
        total_tokens: (used_tokens as f64 + burn.tokens_per_minute * remaining_minutes).round()
            as i64,
        cost: match (used_cost, burn.cost_per_hour) {
            (Some(amount), Some(hourly)) => {
                Some(((amount + hourly / 60.0 * remaining_minutes) * 100.0).round() / 100.0)
            }
            _ => None,
        },
    }
}

pub fn parse_occurred_at(value: &str) -> Option<DateTime<Utc>> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(parsed.with_timezone(&Utc));
    }
    for format in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(value, format) {
            return Some(naive.and_utc());
        }
    }
    None
}

fn floor_to_utc_hour(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    timestamp
        .with_minute(0)
        .and_then(|value| value.with_second(0))
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(timestamp)
}

fn iso(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Secs, true)
}
