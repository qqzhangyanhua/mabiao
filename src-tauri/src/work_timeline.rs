//! 单日工作时间线：把当天各会话画成时间轴片段，供前端泳道布局渲染。
//! 只用已归一的消耗记录字段（项目名/来源/模型），不解析对话正文（见 CONTEXT.md）。
//! 会话区间 = 传入记录里能看到的该会话 occurred_at 范围，裁剪到本地日历日 `day` 后展示；
//! token 归属按每条记录 occurred_at 是否落在这天判定，跨午夜的会话只统计落在当天的部分。

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};

use crate::aggregate::assign_latest;
use crate::billing_window::parse_occurred_at;
use crate::domain::{UsageRecord, WorkSegment, WorkSessionSpan, WorkTimelineDto};

/// 给 SQL 层用的宽口径日期边界（前一天 ~ 后一天），覆盖本地时区可能造成的 ±1 天偏移；
/// 精确裁剪仍在 `assemble` 里按本地日历日判定，这里只是避免全表扫描的粗筛。
pub fn broad_date_bounds(day: &str) -> Option<(String, String)> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    Some((
        (date - Duration::days(1)).format("%Y-%m-%d").to_string(),
        (date + Duration::days(1)).format("%Y-%m-%d").to_string(),
    ))
}

/// 本地日历日对应的 UTC 半开区间，格式 `YYYY-MM-DDTHH:MM:SS`（无时区后缀），
/// 供 SQL 与 `occurred_at` 做前缀安全的字典序比较：`>= start AND < end`。
pub(crate) fn local_day_sql_bounds(day: &str) -> Option<(String, String)> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let midnight = date.and_hms_opt(0, 0, 0)?;
    let start = local_midnight_to_utc(midnight);
    let end = start + Duration::days(1);
    Some((sql_ts(start), sql_ts(end)))
}

fn sql_ts(timestamp: DateTime<Utc>) -> String {
    timestamp.format("%Y-%m-%dT%H:%M:%S").to_string()
}

pub(crate) struct SessionAcc {
    pub source: String,
    pub session_id: String,
    pub project: String,
    pub project_at: Option<String>,
    pub model: String,
    pub model_at: Option<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub day_tokens: i64,
    pub day_turns: i64,
}

/// 构建单日工作时间线。`records` 只需覆盖到各会话在 `day` 附近的记录（调用方可用
/// `broad_date_bounds` 粗筛再查询）；会话区间基于传入记录里能看到的 occurred_at 范围，
/// 不会去追溯该会话在此范围之外的历史，实践中足以覆盖跨午夜场景。
///
/// `extra` 是没有消耗记录的会话区间（目前是 Cursor 本机会话）。同一
/// `(source, session_id)` 与记录合并：时间取并集，token 仍只来自记录。
pub fn build(records: &[UsageRecord], extra: &[WorkSessionSpan], day: &str) -> WorkTimelineDto {
    let Some(date) = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok() else {
        return WorkTimelineDto::empty(day);
    };
    let Some(midnight) = date.and_hms_opt(0, 0, 0) else {
        return WorkTimelineDto::empty(day);
    };
    let day_start = local_midnight_to_utc(midnight);
    let day_end = day_start + Duration::days(1);

    let mut sessions: BTreeMap<(String, String), SessionAcc> = BTreeMap::new();
    for record in records {
        let Some(at) = parse_occurred_at(&record.occurred_at) else {
            continue;
        };
        let in_day = at >= day_start && at < day_end;
        let key = (
            record.source.as_str().to_string(),
            record.session_id.clone(),
        );
        let entry = sessions.entry(key.clone()).or_insert_with(|| SessionAcc {
            source: key.0,
            session_id: key.1,
            project: String::new(),
            project_at: None,
            model: String::new(),
            model_at: None,
            start: at,
            end: at,
            day_tokens: 0,
            day_turns: 0,
        });
        if at < entry.start {
            entry.start = at;
        }
        if at > entry.end {
            entry.end = at;
        }
        assign_latest(
            &mut entry.project,
            &mut entry.project_at,
            &record.project,
            &record.occurred_at,
        );
        assign_latest(
            &mut entry.model,
            &mut entry.model_at,
            &record.model,
            &record.occurred_at,
        );
        if in_day {
            entry.day_tokens += record.total_tokens;
            entry.day_turns += 1;
        }
    }
    assemble(sessions, extra, day)
}

/// SQL 路径按会话聚合后走这里，与内存路径共用裁剪和强度指标。
pub(crate) fn assemble(
    mut sessions: BTreeMap<(String, String), SessionAcc>,
    extra: &[WorkSessionSpan],
    day: &str,
) -> WorkTimelineDto {
    let Some(date) = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok() else {
        return WorkTimelineDto::empty(day);
    };
    let Some(midnight) = date.and_hms_opt(0, 0, 0) else {
        return WorkTimelineDto::empty(day);
    };
    let day_start = local_midnight_to_utc(midnight);
    let day_end = day_start + Duration::days(1);

    for span in extra {
        merge_span(&mut sessions, span);
    }

    let turn_count: i64 = sessions.values().map(|acc| acc.day_turns).sum();

    // 裁剪到当天的区间（UTC 时刻），用于强度指标计算与片段输出。
    let mut clipped: Vec<(DateTime<Utc>, DateTime<Utc>)> = Vec::new();
    let mut segments: Vec<WorkSegment> = sessions
        .into_values()
        // 工作片段数 = 会话区间与当天区间有交集，不要求该会话在这天有具体的一条记录。
        .filter(|acc| acc.start < day_end && acc.end >= day_start)
        .map(|acc| {
            let clip_start = acc.start.max(day_start);
            let clip_end = acc.end.min(day_end).max(clip_start);
            clipped.push((clip_start, clip_end));
            WorkSegment {
                session_id: acc.session_id,
                source: acc.source,
                project: acc.project,
                model: acc.model,
                start: iso(clip_start),
                end: iso(clip_end),
                total_tokens: acc.day_tokens,
            }
        })
        .collect();
    segments.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });

    let total_tokens: i64 = segments.iter().map(|segment| segment.total_tokens).sum();
    let segment_count = segments.len() as i64;
    let ai_exec_minutes: f64 = clipped.iter().map(|(s, e)| minutes_between(*s, *e)).sum();
    let peak_parallel: i64 = peak_parallel_sweep(&clipped);
    let parallel_intensity: Option<f64> = parallel_intensity(&clipped, ai_exec_minutes);

    WorkTimelineDto {
        day: day.to_string(),
        total_tokens,
        segment_count,
        turn_count,
        ai_exec_minutes,
        peak_parallel,
        parallel_intensity,
        segments,
    }
}

/// 把一条无消耗记录的会话区间并进累加器。起止时间取并集；项目/模型按时间较新者覆盖。
fn merge_span(sessions: &mut BTreeMap<(String, String), SessionAcc>, span: &WorkSessionSpan) {
    let Some(start) = parse_occurred_at(&span.started_at) else {
        return;
    };
    let Some(parsed_end) = parse_occurred_at(&span.ended_at) else {
        return;
    };
    let end = parsed_end.max(start);
    let key = (span.source.clone(), span.session_id.clone());
    let entry = sessions.entry(key.clone()).or_insert_with(|| SessionAcc {
        source: key.0,
        session_id: key.1,
        project: String::new(),
        project_at: None,
        model: String::new(),
        model_at: None,
        start,
        end,
        day_tokens: 0,
        day_turns: 0,
    });
    if start < entry.start {
        entry.start = start;
    }
    if end > entry.end {
        entry.end = end;
    }
    assign_latest(
        &mut entry.project,
        &mut entry.project_at,
        &span.project,
        &span.ended_at,
    );
    assign_latest(
        &mut entry.model,
        &mut entry.model_at,
        &span.model,
        &span.ended_at,
    );
}

/// 扫描线求峰值并行：把每段区间拆成 (start, +1) / (end, -1) 事件，按时刻排序后累加取最大值。
/// 端点口径含起不含止——相邻会话首尾相接不算重叠。
fn peak_parallel_sweep(intervals: &[(DateTime<Utc>, DateTime<Utc>)]) -> i64 {
    let mut events: Vec<(DateTime<Utc>, i32)> = Vec::with_capacity(intervals.len() * 2);
    for (start, end) in intervals {
        events.push((*start, 1));
        events.push((*end, -1));
    }
    // 先按时刻升序，同一时刻先处理离开（-1）再处理进入（+1），保证首尾相接不算重叠。
    events.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut current: i64 = 0;
    let mut peak: i64 = 0;
    for (_, delta) in events {
        current += delta as i64;
        if current > peak {
            peak = current;
        }
    }
    peak
}

/// 并行强度 = 累计执行时长 ÷ 会话区间并集时长。无重叠时两者相等，结果为 1.0x；
/// 全部重叠时并集时长 = 单段时长，结果 = 段数。空日返回 `None`。
fn parallel_intensity(
    intervals: &[(DateTime<Utc>, DateTime<Utc>)],
    ai_exec_minutes: f64,
) -> Option<f64> {
    if intervals.is_empty() || ai_exec_minutes <= 0.0 {
        return None;
    }
    let mut sorted: Vec<(DateTime<Utc>, DateTime<Utc>)> = intervals.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut union_minutes: f64 = 0.0;
    let mut union_start = sorted[0].0;
    let mut union_end = sorted[0].1;
    for (start, end) in sorted.iter().skip(1) {
        if *start > union_end {
            union_minutes += minutes_between(union_start, union_end);
            union_start = *start;
            union_end = *end;
        } else if *end > union_end {
            union_end = *end;
        }
    }
    union_minutes += minutes_between(union_start, union_end);
    if union_minutes <= 0.0 {
        return None;
    }
    Some(ai_exec_minutes / union_minutes)
}

/// 两个 UTC 时刻之间的分钟数（浮点，保留亚分钟精度）。
fn minutes_between(start: DateTime<Utc>, end: DateTime<Utc>) -> f64 {
    (end - start).num_milliseconds() as f64 / 60_000.0
}

/// 本地日历日零点 -> UTC 时刻。夏令时切换缺失该本地时刻的极端情况下退化为按 UTC 处理。
fn local_midnight_to_utc(naive: NaiveDateTime) -> DateTime<Utc> {
    match naive.and_local_timezone(Local) {
        LocalResult::Single(dt) => dt.with_timezone(&Utc),
        LocalResult::Ambiguous(earliest, _) => earliest.with_timezone(&Utc),
        LocalResult::None => Utc.from_utc_datetime(&naive),
    }
}

pub(crate) fn local_midnight_utc(date: NaiveDate) -> DateTime<Utc> {
    local_midnight_to_utc(date.and_hms_opt(0, 0, 0).expect("midnight is valid"))
}

pub(crate) fn rfc3339_millis(timestamp: DateTime<Utc>) -> String {
    iso(timestamp)
}

fn iso(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
