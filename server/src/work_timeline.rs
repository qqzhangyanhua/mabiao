//! 工作时间线：把已推送会话按本地日历日裁成横条，挂上当天的消耗。
//!
//! 不发明数据：只铺 `sessions` 里有的场次（桌面端还会并入未推的 Cursor 本机会话，
//! 远端没有那份）。Token / 费用只统计该场、该日的 `usage_records`，账号用量与代码量不进。
//! 区间按**重叠**选（开始 < to 且 结束 ≥ from），与桌面端工作时间线一致，
//! 不同于会话列表按结束时间落点。

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};

use crate::error::AppError;
use crate::usage_query::Filter;

/// 防止无界区间拖垮查询。网页预设最长约一月，400 天足够。
pub const MAX_DAYS: i64 = 400;
/// 一场会话跨多天会拆成多根横条；先限制读进来的会话数。
pub const MAX_SESSIONS: i64 = 2000;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkTimelineQuery {
    pub account_id: Option<i64>,
    pub project_id: Option<i64>,
    /// 来源 slug，与 `sessions.source` / `usage_records.source` 精确匹配。
    pub source: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub tz_offset_minutes: Option<i32>,
}

/// 一根裁到当天的横条。`id` 是远端会话主键，点进去看详情。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkSegmentView {
    pub id: i64,
    pub account_id: i64,
    pub account: String,
    pub source: String,
    pub session_id: String,
    pub title: String,
    pub project: String,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub model: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub total_tokens: i64,
    pub cost_snapshot: f64,
    pub unified_cost: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkDayView {
    /// 本地日历日 `YYYY-MM-DD`。
    pub day: String,
    pub segment_count: i64,
    pub total_tokens: i64,
    pub cost_snapshot: f64,
    pub unified_cost: f64,
    pub segments: Vec<WorkSegmentView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkTimelineResponse {
    pub days: Vec<WorkDayView>,
    pub segment_count: i64,
    pub total_tokens: i64,
    pub cost_snapshot: f64,
    pub unified_cost: f64,
    /// 命中 `MAX_SESSIONS`，后面的会话没铺上去。
    pub truncated: bool,
}

#[derive(Debug, Clone, FromRow)]
struct SessionSpanRow {
    id: i64,
    account_id: i64,
    account: String,
    device_pk: i64,
    source: String,
    session_id: String,
    title: String,
    project_path: String,
    project_id: Option<i64>,
    project_name: Option<String>,
    model: String,
    span_start: DateTime<Utc>,
    span_end: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
struct DayUsageRow {
    account_id: i64,
    device_pk: i64,
    source: String,
    session_id: String,
    day: String,
    total_tokens: i64,
    cost_snapshot: f64,
    unified_cost: f64,
}

type UsageKey = (i64, i64, String, String, String);

pub async fn build(
    pool: &PgPool,
    filter: &Filter,
    source: Option<&str>,
    tz_offset_minutes: i32,
) -> Result<WorkTimelineResponse, AppError> {
    if let (Some(from), Some(to)) = (filter.from, filter.to) {
        if to < from {
            return Err(AppError::invalid("to 不能早于 from"));
        }
        // 用跨度判断，避免 `local_days_in` 先截断到上限再看起来像刚好 400 天。
        if (to - from).num_days() > MAX_DAYS {
            return Err(AppError::invalid(format!("时间范围最多 {MAX_DAYS} 天")));
        }
    }

    let sessions = list_overlapping(pool, filter, source).await?;
    let truncated = sessions.len() as i64 >= MAX_SESSIONS;
    let usage = usage_by_day(pool, filter, source, tz_offset_minutes).await?;
    Ok(assemble(
        &sessions,
        &usage,
        filter.from,
        filter.to,
        tz_offset_minutes,
        truncated,
    ))
}

async fn list_overlapping(
    pool: &PgPool,
    filter: &Filter,
    source: Option<&str>,
) -> Result<Vec<SessionSpanRow>, AppError> {
    // 重叠：结束 ≥ from 且 开始 < to。缺客户端时间时退回推送时间，免得会话从时间线上消失。
    Ok(sqlx::query_as(
        "SELECT s.id, s.account_id, a.account, s.device_pk, s.source, s.session_id, s.title,
                s.project_path, s.project_id, p.name AS project_name, s.model,
                COALESCE(s.started_at, s.ended_at, s.pushed_at) AS span_start,
                COALESCE(s.ended_at, s.started_at, s.pushed_at) AS span_end
         FROM sessions s
         JOIN remote_accounts a ON a.id = s.account_id
         LEFT JOIN projects p ON p.id = s.project_id
         WHERE ($1::bigint IS NULL OR s.account_id = $1)
           AND ($2::timestamptz IS NULL
                OR COALESCE(s.ended_at, s.started_at, s.pushed_at) >= $2)
           AND ($3::timestamptz IS NULL
                OR COALESCE(s.started_at, s.ended_at, s.pushed_at) < $3)
           AND ($4::bigint IS NULL OR s.project_id = $4)
           AND ($5::text IS NULL OR s.source = $5)
         ORDER BY span_start ASC, s.id ASC
         LIMIT $6",
    )
    .bind(filter.account_id)
    .bind(filter.from)
    .bind(filter.to)
    .bind(filter.project_id)
    .bind(source)
    .bind(MAX_SESSIONS)
    .fetch_all(pool)
    .await?)
}

async fn usage_by_day(
    pool: &PgPool,
    filter: &Filter,
    source: Option<&str>,
    tz_offset_minutes: i32,
) -> Result<Vec<DayUsageRow>, AppError> {
    Ok(sqlx::query_as(
        "SELECT u.account_id, u.device_pk, u.source, u.session_id,
                to_char((u.occurred_at AT TIME ZONE 'UTC') + $5::int * interval '1 minute',
                        'YYYY-MM-DD') AS day,
                COALESCE(sum(u.total_tokens), 0)::bigint AS total_tokens,
                COALESCE(sum(u.cost_snapshot), 0)::float8 AS cost_snapshot,
                COALESCE(sum(u.unified_cost), 0)::float8 AS unified_cost
         FROM usage_records u
         WHERE ($1::bigint IS NULL OR u.account_id = $1)
           AND ($2::timestamptz IS NULL OR u.occurred_at >= $2)
           AND ($3::timestamptz IS NULL OR u.occurred_at < $3)
           AND ($4::bigint IS NULL OR u.project_id = $4)
           AND ($6::text IS NULL OR u.source = $6)
         GROUP BY u.account_id, u.device_pk, u.source, u.session_id, 5",
    )
    .bind(filter.account_id)
    .bind(filter.from)
    .bind(filter.to)
    .bind(filter.project_id)
    .bind(tz_offset_minutes)
    .bind(source)
    .fetch_all(pool)
    .await?)
}

fn assemble(
    sessions: &[SessionSpanRow],
    usage: &[DayUsageRow],
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    tz_offset_minutes: i32,
    truncated: bool,
) -> WorkTimelineResponse {
    let usage_map: BTreeMap<UsageKey, &DayUsageRow> = usage
        .iter()
        .map(|row| {
            (
                (
                    row.account_id,
                    row.device_pk,
                    row.source.clone(),
                    row.session_id.clone(),
                    row.day.clone(),
                ),
                row,
            )
        })
        .collect();

    let days = match (from, to) {
        (Some(start), Some(end)) => local_days_in(start, end, tz_offset_minutes),
        _ => days_touched_by(sessions, tz_offset_minutes),
    };

    let mut by_day: BTreeMap<String, Vec<WorkSegmentView>> = BTreeMap::new();
    for session in sessions {
        let span_end = session.span_end.max(session.span_start);
        for day in &days {
            let Some((start, end)) =
                clip_to_day(session.span_start, span_end, *day, tz_offset_minutes)
            else {
                continue;
            };
            let day_key = day.format("%Y-%m-%d").to_string();
            let usage_row = usage_map.get(&(
                session.account_id,
                session.device_pk,
                session.source.clone(),
                session.session_id.clone(),
                day_key.clone(),
            ));
            by_day.entry(day_key).or_default().push(WorkSegmentView {
                id: session.id,
                account_id: session.account_id,
                account: session.account.clone(),
                source: session.source.clone(),
                session_id: session.session_id.clone(),
                title: session.title.clone(),
                project: session.project_path.clone(),
                project_id: session.project_id,
                project_name: session.project_name.clone(),
                model: session.model.clone(),
                start,
                end,
                total_tokens: usage_row.map(|row| row.total_tokens).unwrap_or(0),
                cost_snapshot: usage_row.map(|row| row.cost_snapshot).unwrap_or(0.0),
                unified_cost: usage_row.map(|row| row.unified_cost).unwrap_or(0.0),
            });
        }
    }

    let mut days: Vec<WorkDayView> = by_day
        .into_iter()
        .rev()
        .map(|(day, mut segments)| {
            segments.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.id.cmp(&b.id)));
            let total_tokens = segments.iter().map(|s| s.total_tokens).sum();
            let cost_snapshot = segments.iter().map(|s| s.cost_snapshot).sum();
            let unified_cost = segments.iter().map(|s| s.unified_cost).sum();
            WorkDayView {
                segment_count: segments.len() as i64,
                total_tokens,
                cost_snapshot,
                unified_cost,
                day,
                segments,
            }
        })
        .collect();
    // 只要有会话的天；空日不占一行。
    days.retain(|day| !day.segments.is_empty());

    let segment_count = days.iter().map(|d| d.segment_count).sum();
    let total_tokens = days.iter().map(|d| d.total_tokens).sum();
    let cost_snapshot = days.iter().map(|d| d.cost_snapshot).sum();
    let unified_cost = days.iter().map(|d| d.unified_cost).sum();
    WorkTimelineResponse {
        days,
        segment_count,
        total_tokens,
        cost_snapshot,
        unified_cost,
        truncated,
    }
}

pub(crate) fn day_start_utc(day: NaiveDate, tz_offset_minutes: i32) -> DateTime<Utc> {
    let midnight = day
        .and_hms_opt(0, 0, 0)
        .expect("midnight is valid")
        .and_utc();
    midnight - Duration::minutes(i64::from(tz_offset_minutes))
}

pub(crate) fn local_day_of(timestamp: DateTime<Utc>, tz_offset_minutes: i32) -> NaiveDate {
    (timestamp + Duration::minutes(i64::from(tz_offset_minutes))).date_naive()
}

fn local_days_in(from: DateTime<Utc>, to: DateTime<Utc>, tz_offset_minutes: i32) -> Vec<NaiveDate> {
    if to <= from {
        return Vec::new();
    }
    let first = local_day_of(from, tz_offset_minutes);
    let last = local_day_of(to - Duration::milliseconds(1), tz_offset_minutes);
    let mut days = Vec::new();
    let mut cursor = first;
    while cursor <= last && days.len() < MAX_DAYS as usize {
        days.push(cursor);
        cursor += Duration::days(1);
    }
    days
}

fn days_touched_by(sessions: &[SessionSpanRow], tz_offset_minutes: i32) -> Vec<NaiveDate> {
    let Some(min) = sessions.iter().map(|s| s.span_start).min() else {
        return Vec::new();
    };
    let Some(max) = sessions.iter().map(|s| s.span_end.max(s.span_start)).max() else {
        return Vec::new();
    };
    local_days_in(min, max + Duration::milliseconds(1), tz_offset_minutes)
}

fn clip_to_day(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    day: NaiveDate,
    tz_offset_minutes: i32,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let day_start = day_start_utc(day, tz_offset_minutes);
    let day_end = day_start + Duration::days(1);
    if end < day_start || start >= day_end {
        return None;
    }
    let clip_start = start.max(day_start);
    let clip_end = end.min(day_end).max(clip_start);
    Some((clip_start, clip_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    fn session(id: i64, start: &str, end: &str, source: &str, session_id: &str) -> SessionSpanRow {
        SessionSpanRow {
            id,
            account_id: 1,
            account: "alice".into(),
            device_pk: 9,
            source: source.into(),
            session_id: session_id.into(),
            title: format!("{session_id} 标题"),
            project_path: "/w/mabiao".into(),
            project_id: Some(3),
            project_name: Some("mabiao".into()),
            model: "gpt-5".into(),
            span_start: ts(start),
            span_end: ts(end),
        }
    }

    fn usage(session_id: &str, day: &str, tokens: i64, unified: f64) -> DayUsageRow {
        DayUsageRow {
            account_id: 1,
            device_pk: 9,
            source: "codex".into(),
            session_id: session_id.into(),
            day: day.into(),
            total_tokens: tokens,
            cost_snapshot: 9.0,
            unified_cost: unified,
        }
    }

    #[test]
    fn east8_midnight_is_previous_afternoon_utc() {
        let day = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        assert_eq!(day_start_utc(day, 480), ts("2026-02-28T16:00:00Z"));
        assert_eq!(local_day_of(ts("2026-02-28T16:00:00Z"), 480), day);
        assert_eq!(
            local_day_of(ts("2026-02-28T15:59:59Z"), 480),
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()
        );
    }

    #[test]
    fn clips_a_session_that_crosses_midnight() {
        let day = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let (start, end) = clip_to_day(
            ts("2026-02-28T20:00:00Z"),
            ts("2026-03-01T04:00:00Z"),
            day,
            0,
        )
        .unwrap();
        assert_eq!(start, ts("2026-03-01T00:00:00Z"));
        assert_eq!(end, ts("2026-03-01T04:00:00Z"));
    }

    #[test]
    fn skips_a_session_that_misses_the_day() {
        let day = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
        assert!(clip_to_day(
            ts("2026-03-01T10:00:00Z"),
            ts("2026-03-01T12:00:00Z"),
            day,
            0,
        )
        .is_none());
    }

    #[test]
    fn assemble_splits_across_days_and_keeps_same_day_tokens() {
        let sessions = [session(
            11,
            "2026-03-01T22:00:00Z",
            "2026-03-02T02:00:00Z",
            "codex",
            "s-night",
        )];
        let usage = [
            usage("s-night", "2026-03-01", 100, 0.1),
            usage("s-night", "2026-03-02", 40, 0.04),
        ];
        let out = assemble(
            &sessions,
            &usage,
            Some(ts("2026-03-01T00:00:00Z")),
            Some(ts("2026-03-03T00:00:00Z")),
            0,
            false,
        );
        assert_eq!(out.days.len(), 2);
        assert_eq!(out.days[0].day, "2026-03-02", "新的一天在上");
        assert_eq!(out.days[0].segments[0].total_tokens, 40);
        assert_eq!(out.days[0].segments[0].start, ts("2026-03-02T00:00:00Z"));
        assert_eq!(out.days[0].segments[0].end, ts("2026-03-02T02:00:00Z"));
        assert_eq!(out.days[1].day, "2026-03-01");
        assert_eq!(out.days[1].segments[0].total_tokens, 100);
        assert_eq!(out.days[1].total_tokens, 100);
        assert_eq!(out.total_tokens, 140);
        assert_eq!(out.segment_count, 2);
        assert!(!out.truncated);
    }

    #[test]
    fn assemble_does_not_invent_a_bar_from_usage_without_a_session() {
        let usage = [usage("orphan", "2026-03-01", 999, 1.0)];
        let out = assemble(
            &[],
            &usage,
            Some(ts("2026-03-01T00:00:00Z")),
            Some(ts("2026-03-02T00:00:00Z")),
            0,
            false,
        );
        assert!(out.days.is_empty());
        assert_eq!(out.total_tokens, 0);
    }

    #[test]
    fn overlapping_window_keeps_a_session_that_started_before_from() {
        let sessions = [session(
            1,
            "2026-02-28T20:00:00Z",
            "2026-03-01T08:00:00Z",
            "codex",
            "s-early",
        )];
        let out = assemble(
            &sessions,
            &[],
            Some(ts("2026-03-01T00:00:00Z")),
            Some(ts("2026-03-02T00:00:00Z")),
            0,
            false,
        );
        assert_eq!(out.days.len(), 1);
        assert_eq!(out.days[0].segments[0].start, ts("2026-03-01T00:00:00Z"));
        assert_eq!(out.days[0].segments[0].end, ts("2026-03-01T08:00:00Z"));
        assert_eq!(out.days[0].segments[0].total_tokens, 0);
    }
}
