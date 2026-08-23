use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Utc};

use crate::billing_window;
use crate::cost::{derive_cost, sum_costs, sum_cursor_event_costs};
use crate::domain::{
    ApplicationAnalyticsDto, ApplicationEfficiency, ApplicationTrendPoint, BillingWindowsDto,
    CursorUsageEvent, EfficiencyMetrics, Filter, FilterOptions, NamedAmount, OverviewDto,
    PriceTable, ProjectApplicationRow, SeriesPoint, SessionRow, TurnRow, UsageRecord,
    WorkTimelineDto,
};

pub fn matches_filter(record: &UsageRecord, filter: &Filter) -> bool {
    if let Some(from) = &filter.from {
        if record.occurred_at.as_str() < from.as_str() {
            return false;
        }
    }
    if let Some(to) = &filter.to {
        if record.occurred_at.as_str() > to.as_str() {
            return false;
        }
    }
    if !filter.sources.is_empty() && !filter.sources.iter().any(|s| s == record.source.as_str()) {
        return false;
    }
    if !filter.models.is_empty() && !filter.models.iter().any(|m| m == &record.model) {
        return false;
    }
    if !filter.projects.is_empty() && !filter.projects.iter().any(|p| p == &record.project) {
        return false;
    }
    if !filter.providers.is_empty() && !filter.providers.iter().any(|p| p == &record.provider) {
        return false;
    }
    true
}

pub fn apply_filter<'a>(records: &'a [UsageRecord], filter: &Filter) -> Vec<&'a UsageRecord> {
    records
        .iter()
        .filter(|r| matches_filter(r, filter))
        .collect()
}

pub fn overview(records: &[UsageRecord], filter: &Filter, prices: &PriceTable) -> OverviewDto {
    let filtered = apply_filter(records, filter);
    let mut sessions = std::collections::BTreeSet::new();
    let mut dto = OverviewDto {
        total_tokens: 0,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 0,
        session_count: 0,
        cost: None,
        unpriced: false,
    };
    for record in &filtered {
        dto.total_tokens += record.total_tokens;
        dto.input_tokens += record.input_tokens;
        dto.output_tokens += record.output_tokens;
        dto.cache_read_tokens += record.cache_read_tokens;
        dto.cache_creation_tokens += record.cache_creation_tokens;
        dto.reasoning_tokens += record.reasoning_tokens;
        sessions.insert((
            record.source.as_str().to_string(),
            record.session_id.clone(),
        ));
    }
    dto.session_count = sessions.len() as i64;
    let (cost, unpriced) = sum_costs(&filtered, prices);
    dto.cost = cost;
    dto.unpriced = unpriced;
    dto
}

pub fn billing_windows(
    records: &[UsageRecord],
    filter: &Filter,
    prices: &PriceTable,
    now: DateTime<Utc>,
) -> BillingWindowsDto {
    let scoped = Filter {
        from: None,
        to: None,
        sources: filter.sources.clone(),
        models: filter.models.clone(),
        projects: filter.projects.clone(),
        providers: filter.providers.clone(),
    };
    // Cursor 账号用量在 query.rs 里从独立表挂进 7 天滚动；内存路径只对照消耗记录。
    billing_window::summarize(apply_filter(records, &scoped), prices, now)
}

pub fn trend(
    records: &[UsageRecord],
    filter: &Filter,
    prices: &PriceTable,
    grain: &str,
) -> Vec<SeriesPoint> {
    let filtered = apply_filter(records, filter);
    let mut buckets: BTreeMap<String, TrendBucket> = BTreeMap::new();
    for record in filtered {
        let key = bucket_key(&record.occurred_at, grain);
        let entry = buckets.entry(key).or_default();
        entry.total_tokens += record.total_tokens;
        entry.input_tokens += record.input_tokens;
        entry.output_tokens += record.output_tokens;
        entry.cache_read_tokens += record.cache_read_tokens;
        entry.cache_creation_tokens += record.cache_creation_tokens;
        entry.reasoning_tokens += record.reasoning_tokens;
        let derived = derive_cost(record, prices);
        if let Some(amount) = derived.amount {
            entry.cost = Some(entry.cost.unwrap_or(0.0) + amount);
        }
        if derived.unpriced {
            entry.unpriced = true;
        }
    }
    buckets
        .into_iter()
        .map(|(bucket, acc)| SeriesPoint {
            bucket,
            total_tokens: acc.total_tokens,
            input_tokens: acc.input_tokens,
            output_tokens: acc.output_tokens,
            cache_read_tokens: acc.cache_read_tokens,
            cache_creation_tokens: acc.cache_creation_tokens,
            reasoning_tokens: acc.reasoning_tokens,
            cost: acc.cost,
        })
        .collect()
}

/// 把 Cursor 账号用量并进使用统计时间桶（本机消耗记录之上叠加）。
pub fn attach_cursor_trend(
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
pub fn attach_cursor_project_breakdown(
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

#[derive(Default)]
struct TrendBucket {
    total_tokens: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    reasoning_tokens: i64,
    cost: Option<f64>,
    unpriced: bool,
}

pub(crate) fn bucket_key(occurred_at: &str, grain: &str) -> String {
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

pub fn application_analytics(
    records: &[UsageRecord],
    filter: &Filter,
    grain: &str,
) -> ApplicationAnalyticsDto {
    let filtered = apply_filter(records, filter);
    let mut summary = EfficiencyAcc::default();
    let mut applications: BTreeMap<String, EfficiencyAcc> = BTreeMap::new();
    let mut trend: BTreeMap<String, ApplicationTrendPoint> = BTreeMap::new();
    let mut projects: BTreeMap<String, ProjectApplicationRow> = BTreeMap::new();

    for record in filtered {
        let source = record.source.as_str().to_string();
        let session = (source.clone(), record.session_id.clone());

        summary.add(record, session.clone());
        applications
            .entry(source.clone())
            .or_default()
            .add(record, session);

        let bucket = bucket_key(&record.occurred_at, grain);
        let trend_point = trend
            .entry(bucket.clone())
            .or_insert_with(|| ApplicationTrendPoint {
                bucket,
                total_tokens: 0,
                values: BTreeMap::new(),
            });
        trend_point.total_tokens += record.total_tokens;
        *trend_point.values.entry(source.clone()).or_default() += record.total_tokens;

        let project = if record.project.is_empty() {
            "（未标注）".to_string()
        } else {
            record.project.clone()
        };
        let project_row =
            projects
                .entry(project.clone())
                .or_insert_with(|| ProjectApplicationRow {
                    project,
                    total_tokens: 0,
                    values: BTreeMap::new(),
                });
        project_row.total_tokens += record.total_tokens;
        *project_row.values.entry(source).or_default() += record.total_tokens;
    }

    let mut by_application: Vec<ApplicationEfficiency> = applications
        .into_iter()
        .filter_map(|(source, acc)| {
            crate::domain::Source::parse(&source).map(|parsed| ApplicationEfficiency {
                source,
                application: parsed.application_name().to_string(),
                metrics: acc.finish(),
            })
        })
        .collect();
    by_application.sort_by(|a, b| {
        b.metrics
            .total_tokens
            .cmp(&a.metrics.total_tokens)
            .then_with(|| a.application.cmp(&b.application))
    });

    let mut project_rows: Vec<ProjectApplicationRow> = projects.into_values().collect();
    project_rows.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.project.cmp(&b.project))
    });

    ApplicationAnalyticsDto {
        summary: summary.finish(),
        by_application,
        trend: trend.into_values().collect(),
        projects: project_rows,
    }
}

/// 把 Cursor 账号用量挂进应用统计（不改 summary，不进本机 Token KPI）。
pub fn attach_cursor_application(
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
    let reasoning_tokens = 0i64;
    for event in events {
        total_tokens += event.total_tokens();
        input_tokens += event.input_tokens;
        cache_read_tokens += event.cache_read_tokens;
    }
    let session_count = events.len() as i64;
    dto.by_application.push(ApplicationEfficiency {
        source: billing_window::CURSOR_WEEKLY_SOURCE.to_string(),
        application: billing_window::CURSOR_WEEKLY_APPLICATION.to_string(),
        metrics: EfficiencyMetrics {
            total_tokens,
            session_count,
            cache_hit_rate: ratio(cache_read_tokens, input_tokens + cache_read_tokens),
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

#[derive(Default)]
struct EfficiencyAcc {
    total_tokens: i64,
    input_tokens: i64,
    cache_read_tokens: i64,
    reasoning_tokens: i64,
    sessions: std::collections::BTreeSet<(String, String)>,
}

impl EfficiencyAcc {
    fn add(&mut self, record: &UsageRecord, session: (String, String)) {
        self.total_tokens += record.total_tokens;
        self.input_tokens += record.input_tokens;
        self.cache_read_tokens += record.cache_read_tokens;
        self.reasoning_tokens += record.reasoning_tokens;
        self.sessions.insert(session);
    }

    fn finish(self) -> EfficiencyMetrics {
        let session_count = self.sessions.len() as i64;
        let cache_context = self.input_tokens + self.cache_read_tokens;
        EfficiencyMetrics {
            total_tokens: self.total_tokens,
            session_count,
            cache_hit_rate: ratio(self.cache_read_tokens, cache_context),
            average_session_tokens: if session_count == 0 {
                None
            } else {
                Some(self.total_tokens as f64 / session_count as f64)
            },
            reasoning_share: ratio(self.reasoning_tokens, self.total_tokens),
        }
    }
}

fn ratio(numerator: i64, denominator: i64) -> Option<f64> {
    if denominator <= 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}

pub fn by_name(
    records: &[UsageRecord],
    filter: &Filter,
    prices: &PriceTable,
    selector: impl Fn(&UsageRecord) -> String,
) -> Vec<NamedAmount> {
    let filtered = apply_filter(records, filter);
    let mut map: BTreeMap<String, (i64, Option<f64>, bool)> = BTreeMap::new();
    let mut grand = 0i64;
    for record in filtered {
        let name = selector(record);
        let key = if name.is_empty() {
            "（未标注）".to_string()
        } else {
            name
        };
        let entry = map.entry(key).or_insert((0, None, false));
        entry.0 += record.total_tokens;
        grand += record.total_tokens;
        let derived = derive_cost(record, prices);
        if let Some(amount) = derived.amount {
            entry.1 = Some(entry.1.unwrap_or(0.0) + amount);
        }
        if derived.unpriced {
            entry.2 = true;
        }
    }
    let mut rows: Vec<NamedAmount> = map
        .into_iter()
        .map(|(name, (total_tokens, cost, unpriced))| NamedAmount {
            name,
            total_tokens,
            share: if grand == 0 {
                0.0
            } else {
                total_tokens as f64 / grand as f64
            },
            cost,
            unpriced,
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.total_tokens));
    rows
}

pub fn top_sessions(
    records: &[UsageRecord],
    filter: &Filter,
    prices: &PriceTable,
    limit: usize,
) -> Vec<SessionRow> {
    let filtered = apply_filter(records, filter);
    let mut map: BTreeMap<(String, String), SessionAcc> = BTreeMap::new();
    for record in filtered {
        let key = (
            record.source.as_str().to_string(),
            record.session_id.clone(),
        );
        let entry = map.entry(key).or_insert_with(|| SessionAcc {
            source: record.source.as_str().to_string(),
            session_id: record.session_id.clone(),
            project: String::new(),
            model: String::new(),
            source_file: String::new(),
            project_at: None,
            model_at: None,
            source_file_at: None,
            total_tokens: 0,
            started_at: record.occurred_at.clone(),
            ended_at: record.occurred_at.clone(),
            cost: None,
            unpriced: false,
        });
        entry.total_tokens += record.total_tokens;
        if record.occurred_at < entry.started_at || entry.started_at.is_empty() {
            entry.started_at = record.occurred_at.clone();
        }
        if record.occurred_at > entry.ended_at {
            entry.ended_at = record.occurred_at.clone();
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
        assign_latest(
            &mut entry.source_file,
            &mut entry.source_file_at,
            &record.source_file,
            &record.occurred_at,
        );
        let derived = derive_cost(record, prices);
        if let Some(amount) = derived.amount {
            entry.cost = Some(entry.cost.unwrap_or(0.0) + amount);
        }
        if derived.unpriced {
            entry.unpriced = true;
        }
    }
    let mut rows: Vec<SessionRow> = map
        .into_values()
        .map(|acc| SessionRow {
            session_id: acc.session_id,
            source: acc.source,
            project: acc.project,
            model: acc.model,
            total_tokens: acc.total_tokens,
            started_at: acc.started_at,
            ended_at: acc.ended_at,
            source_file: acc.source_file,
            cost: acc.cost,
            unpriced: acc.unpriced,
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.total_tokens));
    rows.truncate(limit);
    rows
}

struct SessionAcc {
    source: String,
    session_id: String,
    project: String,
    model: String,
    source_file: String,
    project_at: Option<String>,
    model_at: Option<String>,
    source_file_at: Option<String>,
    total_tokens: i64,
    started_at: String,
    ended_at: String,
    cost: Option<f64>,
    unpriced: bool,
}

/// 取「occurred_at 最晚」的字段值；时间并列时取字典序更大者。与 SQL 侧
/// `query.rs::latest_nonempty_expr` 同序，`work_timeline` 也复用它决定 project/model 标签。
pub(crate) fn assign_latest(
    field: &mut String,
    field_at: &mut Option<String>,
    value: &str,
    occurred_at: &str,
) {
    if value.is_empty() {
        return;
    }
    let newer = match field_at.as_deref() {
        None => true,
        Some(prev) => occurred_at > prev || (occurred_at == prev && value > field.as_str()),
    };
    if newer {
        *field = value.to_string();
        *field_at = Some(occurred_at.to_string());
    }
}

pub fn session_turns(
    records: &[UsageRecord],
    session_id: &str,
    source: Option<&str>,
    filter: &Filter,
    prices: &PriceTable,
) -> Vec<TurnRow> {
    let mut rows: Vec<TurnRow> = records
        .iter()
        .filter(|r| r.session_id == session_id)
        .filter(|r| source.map(|s| r.source.as_str() == s).unwrap_or(true))
        .filter(|r| matches_filter(r, filter))
        .map(|record| {
            let derived = derive_cost(record, prices);
            TurnRow {
                occurred_at: record.occurred_at.clone(),
                model: record.model.clone(),
                provider: record.provider.clone(),
                input_tokens: record.input_tokens,
                output_tokens: record.output_tokens,
                cache_read_tokens: record.cache_read_tokens,
                cache_creation_tokens: record.cache_creation_tokens,
                reasoning_tokens: record.reasoning_tokens,
                total_tokens: record.total_tokens,
                source_file: record.source_file.clone(),
                cost: derived.amount,
                unpriced: derived.unpriced,
                cost_source: derived.cost_source,
                cost_note: Some(derived.cost_note()),
            }
        })
        .collect();
    rows.sort_by(|a, b| a.occurred_at.cmp(&b.occurred_at));
    rows
}

/// 单日工作时间线：不经 `Filter`，只看 `day`（本地日历日）这一天，独立于顶栏范围筛选。
/// 内存路径默认不带 Cursor 本机会话区间；带区间的对照走 `work_timeline_with_spans`。
pub fn work_timeline(records: &[UsageRecord], day: &str) -> WorkTimelineDto {
    crate::work_timeline::build(records, &[], day)
}

pub fn work_timeline_with_spans(
    records: &[UsageRecord],
    extra: &[crate::domain::WorkSessionSpan],
    day: &str,
) -> WorkTimelineDto {
    crate::work_timeline::build(records, extra, day)
}

pub fn filter_options(records: &[UsageRecord]) -> FilterOptions {
    let mut sources = std::collections::BTreeSet::new();
    let mut models = std::collections::BTreeSet::new();
    let mut projects = std::collections::BTreeSet::new();
    let mut providers = std::collections::BTreeSet::new();
    for record in records {
        sources.insert(record.source.as_str().to_string());
        if !record.model.is_empty() {
            models.insert(record.model.clone());
        }
        if !record.project.is_empty() {
            projects.insert(record.project.clone());
        }
        if !record.provider.is_empty() {
            providers.insert(record.provider.clone());
        }
    }
    FilterOptions {
        sources: sources.into_iter().collect(),
        models: models.into_iter().collect(),
        projects: projects.into_iter().collect(),
        providers: providers.into_iter().collect(),
    }
}
