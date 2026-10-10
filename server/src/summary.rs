//! 管理网页用的聚合：按天、人、来源、模型、项目拆分消耗记录。
//!
//! 聚合在服务端做，网页只展示。费用并列给统一价与客户端快照，两者都只是求和，
//! 计价规则本体仍在 `team_pricing` / `crates/pricing`。

use sqlx::PgPool;

use crate::error::AppError;
use crate::usage_query::{Filter, Totals};

/// 单个维度最多返回的条数。按费用从高到低截断，长尾不重要。
pub const MAX_BREAKDOWN_ROWS: i64 = 200;
/// 按天趋势最多十年，防止无界区间拖垮查询。
const MAX_DAYS: i64 = 3660;

/// 网页允许选的 UTC 偏移范围（分钟），对应 UTC-12:00 到 UTC+14:00。
pub const MIN_TZ_OFFSET_MINUTES: i32 = -12 * 60;
pub const MAX_TZ_OFFSET_MINUTES: i32 = 14 * 60;

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct BreakdownRow {
    /// 分组键：日期、账号名、来源、模型、项目 key（没有项目为空串）。
    pub key: String,
    pub label: String,
    /// 账号或项目的 id，其它维度为空。
    pub id: Option<i64>,
    pub record_count: i64,
    pub total_tokens: i64,
    pub cost_snapshot: f64,
    pub unified_cost: f64,
    pub unpriced_count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub totals: Totals,
    pub by_day: Vec<BreakdownRow>,
    pub by_account: Vec<BreakdownRow>,
    pub by_source: Vec<BreakdownRow>,
    pub by_model: Vec<BreakdownRow>,
    pub by_project: Vec<BreakdownRow>,
}

const FILTER: &str = "($1::bigint IS NULL OR u.account_id = $1)
    AND ($2::timestamptz IS NULL OR u.occurred_at >= $2)
    AND ($3::timestamptz IS NULL OR u.occurred_at < $3)";

const METRICS: &str = "count(*)::bigint AS record_count,
    COALESCE(sum(u.total_tokens), 0)::bigint AS total_tokens,
    COALESCE(sum(u.cost_snapshot), 0)::float8 AS cost_snapshot,
    COALESCE(sum(u.unified_cost), 0)::float8 AS unified_cost,
    (count(*) FILTER (WHERE u.unified_pricing_source = 'unpriced'))::bigint AS unpriced_count";

const RANKED: &str = "ORDER BY unified_cost DESC, total_tokens DESC, key ASC";

async fn dimension(
    pool: &PgPool,
    filter: &Filter,
    select_and_group: &str,
) -> Result<Vec<BreakdownRow>, AppError> {
    // 拼进去的都是本文件里的常量片段，没有任何用户输入。
    let sql = format!("{select_and_group} {RANKED} LIMIT {MAX_BREAKDOWN_ROWS}");
    Ok(sqlx::query_as(&sql)
        .bind(filter.account_id)
        .bind(filter.from)
        .bind(filter.to)
        .fetch_all(pool)
        .await?)
}

async fn by_day(
    pool: &PgPool,
    filter: &Filter,
    tz_offset_minutes: i32,
) -> Result<Vec<BreakdownRow>, AppError> {
    let sql = format!(
        "SELECT to_char((u.occurred_at AT TIME ZONE 'UTC') + $4::int * interval '1 minute',
                        'YYYY-MM-DD') AS key,
                to_char((u.occurred_at AT TIME ZONE 'UTC') + $4::int * interval '1 minute',
                        'YYYY-MM-DD') AS label,
                NULL::bigint AS id, {METRICS}
         FROM usage_records u WHERE {FILTER}
         GROUP BY 1 ORDER BY key ASC LIMIT {MAX_DAYS}"
    );
    Ok(sqlx::query_as(&sql)
        .bind(filter.account_id)
        .bind(filter.from)
        .bind(filter.to)
        .bind(tz_offset_minutes)
        .fetch_all(pool)
        .await?)
}

/// `tz_offset_minutes` 只影响「按天」怎么切日界；调用方先用上面的范围常量校验。
pub async fn build(
    pool: &PgPool,
    filter: &Filter,
    tz_offset_minutes: i32,
) -> Result<Summary, AppError> {
    let totals = crate::usage_query::totals(pool, filter).await?;
    let by_account = dimension(
        pool,
        filter,
        &format!(
            "SELECT a.account AS key, a.account AS label, a.id AS id, {METRICS}
             FROM usage_records u JOIN remote_accounts a ON a.id = u.account_id
             WHERE {FILTER} GROUP BY a.id, a.account"
        ),
    )
    .await?;
    let by_source = dimension(
        pool,
        filter,
        &format!(
            "SELECT u.source AS key, u.source AS label, NULL::bigint AS id, {METRICS}
             FROM usage_records u WHERE {FILTER} GROUP BY u.source"
        ),
    )
    .await?;
    let by_model = dimension(
        pool,
        filter,
        &format!(
            "SELECT u.model AS key, u.model AS label, NULL::bigint AS id, {METRICS}
             FROM usage_records u WHERE {FILTER} GROUP BY u.model"
        ),
    )
    .await?;
    let by_project = dimension(
        pool,
        filter,
        &format!(
            "SELECT COALESCE(p.key, '') AS key, COALESCE(p.name, '未归属项目') AS label,
                    p.id AS id, {METRICS}
             FROM usage_records u LEFT JOIN projects p ON p.id = u.project_id
             WHERE {FILTER} GROUP BY p.id, p.key, p.name"
        ),
    )
    .await?;
    Ok(Summary {
        totals,
        by_day: by_day(pool, filter, tz_offset_minutes).await?,
        by_account,
        by_source,
        by_model,
        by_project,
    })
}
