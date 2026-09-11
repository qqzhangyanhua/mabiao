//! 把 Cursor 账号用量叠进趋势、项目维度分解和应用分析。
//!
//! 不进消耗记录、不进本机 token KPI、不进本机 5 小时估计窗。
//! 缺价时走 LiteLLM 快照按模型签名兜底。

use std::collections::BTreeMap;

use chrono::Datelike;

use crate::billing_window;
use crate::cost::sum_cursor_event_costs;
use crate::domain::{
    cache_hit_rate, ApplicationAnalyticsDto, ApplicationEfficiency, ApplicationTrendPoint,
    CursorUsageEvent, EfficiencyMetrics, NamedAmount, PriceTable, ProjectApplicationRow,
    SeriesPoint,
};

/// 把 Cursor 账号用量并进使用统计时间桶（本机消耗记录之上叠加）。
pub(crate) fn attach_cursor_trend(
    points: Vec<SeriesPoint>,
    events: &[CursorUsageEvent],
    prices: &PriceTable,
    grain: &str,
) -> Vec<SeriesPoint> {
    if events.is_empty() {
        return points;
    }
    let mut buckets: BTreeMap<String, SeriesPoint> = points
        .into_iter()
        .map(|point| (point.bucket.clone(), point))
        .collect();
    let mut events_by_bucket: BTreeMap<String, Vec<&CursorUsageEvent>> = BTreeMap::new();
    for event in events {
        events_by_bucket
            .entry(bucket_key(&event.occurred_at, grain))
            .or_default()
            .push(event);
    }
    for (bucket, bucket_events) in events_by_bucket {
        let point = buckets
            .entry(bucket.clone())
            .or_insert_with(|| SeriesPoint {
                bucket,
                total_tokens: 0,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                reasoning_tokens: 0,
                cost: None,
            });
        for event in &bucket_events {
            point.total_tokens += event.total_tokens();
            point.input_tokens += event.input_tokens;
            point.output_tokens += event.output_tokens;
            point.cache_read_tokens += event.cache_read_tokens;
            point.cache_creation_tokens += event.cache_creation_tokens;
        }
        let (cost, _) = sum_cursor_event_costs(&bucket_events, prices);
        if let Some(amount) = cost {
            point.cost = Some(point.cost.unwrap_or(0.0) + amount);
        }
    }
    buckets.into_values().collect()
}

/// 项目统计没有账号级 cwd：单独挂一行 `Cursor`，不和本机「未标注」混在一起。
pub(crate) fn attach_cursor_project_breakdown(
    mut rows: Vec<NamedAmount>,
    events: &[CursorUsageEvent],
    prices: &PriceTable,
) -> Vec<NamedAmount> {
    rows.retain(|row| row.name != billing_window::CURSOR_WEEKLY_APPLICATION);
    if events.is_empty() {
        return rows;
    }
    let total_tokens: i64 = events.iter().map(CursorUsageEvent::total_tokens).sum();
    if total_tokens == 0 {
        return rows;
    }
    let event_refs: Vec<&CursorUsageEvent> = events.iter().collect();
    let (cost, unpriced) = sum_cursor_event_costs(&event_refs, prices);
    rows.push(NamedAmount {
        name: billing_window::CURSOR_WEEKLY_APPLICATION.to_string(),
        total_tokens,
        share: 0.0,
        cost,
        unpriced,
    });
    let grand: i64 = rows.iter().map(|row| row.total_tokens).sum();
    for row in &mut rows {
        row.share = if grand == 0 {
            0.0
        } else {
            row.total_tokens as f64 / grand as f64
        };
    }
    rows.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.name.cmp(&b.name))
    });
    rows
}

/// 把 Cursor 账号用量挂进来源统计（不改 summary，不进本机 Token KPI）。
pub(crate) fn attach_cursor_application(
    mut dto: ApplicationAnalyticsDto,
    events: &[CursorUsageEvent],
    grain: &str,
) -> ApplicationAnalyticsDto {
    strip_cursor_application(&mut dto);
    if events.is_empty() {
        return dto;
    }

    let mut total_tokens = 0i64;
    let mut input_tokens = 0i64;
    let mut cache_read_tokens = 0i64;
    let mut cache_creation_tokens = 0i64;
    let reasoning_tokens = 0i64;
    for event in events {
        total_tokens += event.total_tokens();
        input_tokens += event.input_tokens;
        cache_read_tokens += event.cache_read_tokens;
        cache_creation_tokens += event.cache_creation_tokens;
    }
    let session_count = events.len() as i64;
    dto.by_application.push(ApplicationEfficiency {
        source: billing_window::CURSOR_WEEKLY_SOURCE.to_string(),
        application: billing_window::CURSOR_WEEKLY_APPLICATION.to_string(),
        metrics: EfficiencyMetrics {
            total_tokens,
            session_count,
            cache_hit_rate: cache_hit_rate(cache_read_tokens, cache_creation_tokens, input_tokens),
            average_session_tokens: if session_count == 0 {
                None
            } else {
                Some(total_tokens as f64 / session_count as f64)
            },
            reasoning_share: ratio(reasoning_tokens, total_tokens),
        },
    });
    dto.by_application.sort_by(|a, b| {
        b.metrics
            .total_tokens
            .cmp(&a.metrics.total_tokens)
            .then_with(|| a.application.cmp(&b.application))
    });

    let mut trend: BTreeMap<String, ApplicationTrendPoint> = dto
        .trend
        .into_iter()
        .map(|point| (point.bucket.clone(), point))
        .collect();
    for event in events {
        let total = event.total_tokens();
        if total == 0 {
            continue;
        }
        let bucket = bucket_key(&event.occurred_at, grain);
        let point = trend
            .entry(bucket.clone())
            .or_insert_with(|| ApplicationTrendPoint {
                bucket,
                total_tokens: 0,
                values: BTreeMap::new(),
            });
        point.total_tokens += total;
        *point
            .values
            .entry(billing_window::CURSOR_WEEKLY_SOURCE.to_string())
            .or_default() += total;
    }
    dto.trend = trend.into_values().collect();

    let unlabeled = "（未标注）".to_string();
    let cursor_project_total: i64 = events.iter().map(CursorUsageEvent::total_tokens).sum();
    if cursor_project_total > 0 {
        let mut projects: BTreeMap<String, ProjectApplicationRow> = dto
            .projects
            .into_iter()
            .map(|row| (row.project.clone(), row))
            .collect();
        let row = projects
            .entry(unlabeled.clone())
            .or_insert_with(|| ProjectApplicationRow {
                project: unlabeled,
                total_tokens: 0,
                values: BTreeMap::new(),
            });
        row.total_tokens += cursor_project_total;
        *row.values
            .entry(billing_window::CURSOR_WEEKLY_SOURCE.to_string())
            .or_default() += cursor_project_total;
        let mut project_rows: Vec<ProjectApplicationRow> = projects.into_values().collect();
        project_rows.sort_by(|a, b| {
            b.total_tokens
                .cmp(&a.total_tokens)
                .then_with(|| a.project.cmp(&b.project))
        });
        dto.projects = project_rows;
    }

    dto
}

fn strip_cursor_application(dto: &mut ApplicationAnalyticsDto) {
    dto.by_application
        .retain(|row| row.source != billing_window::CURSOR_WEEKLY_SOURCE);
    for point in &mut dto.trend {
        if let Some(value) = point.values.remove(billing_window::CURSOR_WEEKLY_SOURCE) {
            point.total_tokens -= value;
        }
    }
    dto.trend
        .retain(|point| !point.values.is_empty() || point.total_tokens > 0);
    for row in &mut dto.projects {
        if let Some(value) = row.values.remove(billing_window::CURSOR_WEEKLY_SOURCE) {
            row.total_tokens -= value;
        }
    }
    dto.projects
        .retain(|row| !row.values.is_empty() || row.total_tokens > 0);
}

fn bucket_key(occurred_at: &str, grain: &str) -> String {
    if grain == "hour" {
        return occurred_at.get(0..13).unwrap_or(occurred_at).to_string();
    }
    let date = occurred_at.get(0..10).unwrap_or(occurred_at);
    if grain == "week" {
        if let Ok(parsed) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") {
            let week = parsed.iso_week();
            return format!("{:04}-W{:02}", week.year(), week.week());
        }
    }
    if grain == "month" {
        return date.get(0..7).unwrap_or(date).to_string();
    }
    date.to_string()
}

fn ratio(numerator: i64, denominator: i64) -> Option<f64> {
    if denominator <= 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}
