use std::collections::BTreeMap;

use rusqlite::{params_from_iter, types::Value, Connection};

use crate::billing_window;
use crate::cursor_account;
use crate::domain::{
    cache_hit_rate, ApplicationAnalyticsDto, ApplicationEfficiency, ApplicationTrendPoint,
    EfficiencyMetrics, Filter, LowCacheHitSessionRow, LowCacheHitSessionsDto,
    ProjectApplicationRow, Source,
};
use crate::rollup_source::rollup_source;
use crate::rollup_split::rollup_plan;

use super::sql::*;

pub fn application_analytics(
    conn: &Connection,
    filter: &Filter,
    grain: &str,
) -> Result<ApplicationAnalyticsDto, String> {
    // 四组聚合先物化同一工厂子查询，切分时明细只扫一遍。
    // COUNT(DISTINCT session) 在 UNION ALL 之后计算，跨 partial 边界只计一次。
    let inner = rollup_source(
        &rollup_plan(
            filter.from.as_deref(),
            filter.to.as_deref(),
            crate::store::rollup_is_ready(conn),
            Some(grain),
        ),
        filter,
    );
    conn.execute("DROP TABLE IF EXISTS application_analytics_src", [])
        .map_err(|e| e.to_string())?;
    conn.execute(
        &format!(
            "CREATE TEMP TABLE application_analytics_src AS {}",
            inner.sql
        ),
        params_from_iter(inner.params.iter()),
    )
    .map_err(|e| e.to_string())?;
    let bucket = bucket_expr(grain);

    let summary_sql = "SELECT
            COALESCE(SUM(d.total_tokens), 0),
            COALESCE(SUM(d.input_tokens), 0),
            COALESCE(SUM(d.cache_read_tokens), 0),
            COALESCE(SUM(d.cache_creation_tokens), 0),
            COALESCE(SUM(d.reasoning_tokens), 0),
            COUNT(DISTINCT d.source || char(31) || d.session_id)
        FROM application_analytics_src d";
    let (total, input, cache_read, cache_creation, reasoning, session_count) = conn
        .query_row(summary_sql, [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let summary = EfficiencyMetrics {
        total_tokens: total,
        session_count,
        cache_hit_rate: cache_hit_rate(cache_read, cache_creation, input),
        average_session_tokens: if session_count == 0 {
            None
        } else {
            Some(total as f64 / session_count as f64)
        },
        reasoning_share: ratio(reasoning, total),
    };

    let app_sql = "SELECT d.source,
            SUM(d.total_tokens),
            SUM(d.input_tokens),
            SUM(d.cache_read_tokens),
            SUM(d.cache_creation_tokens),
            SUM(d.reasoning_tokens),
            COUNT(DISTINCT d.session_id)
        FROM application_analytics_src d
        GROUP BY d.source";
    let mut stmt = conn.prepare(app_sql).map_err(|e| e.to_string())?;
    let app_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut by_application: Vec<ApplicationEfficiency> = app_rows
        .into_iter()
        .filter_map(
            |(source, total, input, cache_read, cache_creation, reasoning, session_count)| {
                let parsed = Source::parse(&source)?;
                Some(ApplicationEfficiency {
                    source,
                    application: parsed.application_name().to_string(),
                    metrics: EfficiencyMetrics {
                        total_tokens: total,
                        session_count,
                        cache_hit_rate: cache_hit_rate(cache_read, cache_creation, input),
                        average_session_tokens: if session_count == 0 {
                            None
                        } else {
                            Some(total as f64 / session_count as f64)
                        },
                        reasoning_share: ratio(reasoning, total),
                    },
                })
            },
        )
        .collect();
    by_application.sort_by(|a, b| {
        b.metrics
            .total_tokens
            .cmp(&a.metrics.total_tokens)
            .then_with(|| a.application.cmp(&b.application))
    });

    let trend_sql = format!(
        "SELECT {bucket} AS bucket, d.source, SUM(d.total_tokens)
        FROM application_analytics_src d
        GROUP BY 1, 2
        ORDER BY 1, 2"
    );
    let mut stmt = conn.prepare(&trend_sql).map_err(|e| e.to_string())?;
    let trend_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut trend_map: BTreeMap<String, ApplicationTrendPoint> = BTreeMap::new();
    for (bucket, source, total) in trend_rows {
        let point = trend_map
            .entry(bucket.clone())
            .or_insert_with(|| ApplicationTrendPoint {
                bucket,
                total_tokens: 0,
                values: BTreeMap::new(),
            });
        point.total_tokens += total;
        *point.values.entry(source).or_default() += total;
    }

    let project_sql = "SELECT d.project, d.source, SUM(d.total_tokens)
        FROM application_analytics_src d
        GROUP BY 1, 2
        ORDER BY 1, 2";
    let mut stmt = conn.prepare(project_sql).map_err(|e| e.to_string())?;
    let project_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut projects_map: BTreeMap<String, ProjectApplicationRow> = BTreeMap::new();
    for (project, source, total) in project_rows {
        let project = if project.is_empty() {
            "（未标注）".to_string()
        } else {
            project
        };
        let row = projects_map
            .entry(project.clone())
            .or_insert_with(|| ProjectApplicationRow {
                project,
                total_tokens: 0,
                values: BTreeMap::new(),
            });
        row.total_tokens += total;
        *row.values.entry(source).or_default() += total;
    }
    let mut projects: Vec<ProjectApplicationRow> = projects_map.into_values().collect();
    projects.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.project.cmp(&b.project))
    });

    let dto = ApplicationAnalyticsDto {
        summary,
        by_application,
        trend: trend_map.into_values().collect(),
        projects,
    };
    let events = cursor_account::events_for_application_analytics(conn, filter)?;
    Ok(crate::aggregate::attach_cursor_application(
        dto, &events, grain,
    ))
}

const MAX_LOW_CACHE_HIT_SESSIONS: usize = 100;

/// 某一来源命中率最低的 N 条会话。只在该来源内排序，不跨来源排名。
///
/// 没有缓存读/写的来源 `computable = false`，不把 0% 当成命中率。
/// Cursor 账号用量不是本机会话，不能下钻。
pub fn low_cache_hit_sessions(
    conn: &Connection,
    filter: &Filter,
    source: &str,
    limit: usize,
) -> Result<LowCacheHitSessionsDto, String> {
    let limit = limit.clamp(1, MAX_LOW_CACHE_HIT_SESSIONS);
    if source == billing_window::CURSOR_WEEKLY_SOURCE || Source::parse(source).is_none() {
        return Ok(LowCacheHitSessionsDto {
            source: source.to_string(),
            computable: false,
            rows: Vec::new(),
        });
    }

    let mut scoped = filter.clone();
    scoped.sources = vec![source.to_string()];
    let inner = rollup_source(
        &rollup_plan(
            scoped.from.as_deref(),
            scoped.to.as_deref(),
            crate::store::rollup_is_ready(conn),
            None,
        ),
        &scoped,
    );
    let project = unwrap_latest_key_sql("project_key");
    let model = unwrap_latest_key_sql("model_key");
    let sql = format!(
        "SELECT session_id, source, input_tokens, cache_read_tokens, cache_creation_tokens,
                total_tokens, started_at, ended_at, {project} AS project, {model} AS model,
                cache_hit_rate
         FROM (
            SELECT d.session_id AS session_id,
                d.source AS source,
                SUM(d.input_tokens) AS input_tokens,
                SUM(d.cache_read_tokens) AS cache_read_tokens,
                SUM(d.cache_creation_tokens) AS cache_creation_tokens,
                SUM(d.total_tokens) AS total_tokens,
                MIN(d.first_at) AS started_at,
                MAX(d.last_at) AS ended_at,
                MAX(CASE WHEN d.project != '' THEN d.last_at || char(31) || d.project END) AS project_key,
                MAX(CASE WHEN d.model != '' THEN d.last_at || char(31) || d.model END) AS model_key,
                CAST(SUM(d.cache_read_tokens) AS REAL)
                    / (SUM(d.input_tokens) + SUM(d.cache_read_tokens)) AS cache_hit_rate
            FROM ({inner_sql}) d
            GROUP BY d.source, d.session_id
            HAVING (SUM(d.cache_read_tokens) > 0 OR SUM(d.cache_creation_tokens) > 0)
               AND (SUM(d.input_tokens) + SUM(d.cache_read_tokens) > 0)
         )
         ORDER BY cache_hit_rate ASC, total_tokens DESC, session_id ASC
         LIMIT ?",
        inner_sql = inner.sql
    );
    let mut params = inner.params;
    params.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(params.iter()), |row| {
            Ok(LowCacheHitSessionRow {
                session_id: row.get(0)?,
                source: row.get(1)?,
                input_tokens: row.get(2)?,
                cache_read_tokens: row.get(3)?,
                cache_creation_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                started_at: row.get(6)?,
                ended_at: row.get(7)?,
                project: row.get(8)?,
                model: row.get(9)?,
                cache_hit_rate: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(LowCacheHitSessionsDto {
        source: source.to_string(),
        computable: !rows.is_empty(),
        rows,
    })
}
