//! 消耗记录查询：同时给出统一费用与客户端费用快照，供跨人对比。

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::error::AppError;

pub const DEFAULT_LIMIT: i64 = 100;
pub const MAX_LIMIT: i64 = 1000;

/// 已校验的过滤条件。`account_id` 为空表示全体，由调用方按角色决定能不能传空。
#[derive(Debug, Clone)]
pub struct Filter {
    pub account_id: Option<i64>,
    /// 含。
    pub from: Option<DateTime<Utc>>,
    /// 不含。
    pub to: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UsageRow {
    pub id: i64,
    pub account_id: i64,
    pub account: String,
    pub occurred_at: DateTime<Utc>,
    pub source: String,
    pub model: String,
    pub provider: String,
    pub project_path: String,
    pub project_id: Option<i64>,
    pub session_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
    pub native_cost: Option<f64>,
    pub cost_snapshot: Option<f64>,
    pub pricing_source: String,
    pub unified_cost: Option<f64>,
    pub unified_pricing_source: Option<String>,
    pub unified_cost_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Totals {
    pub record_count: i64,
    pub total_tokens: i64,
    pub cost_snapshot_total: f64,
    pub unified_cost_total: f64,
    pub unified_unpriced_count: i64,
}

pub async fn list(
    pool: &PgPool,
    filter: &Filter,
    limit: i64,
    offset: i64,
) -> Result<Vec<UsageRow>, AppError> {
    Ok(sqlx::query_as(
        "SELECT u.id, u.account_id, a.account, u.occurred_at, u.source, u.model, u.provider,
                u.project_path, u.project_id, u.session_id,
                u.input_tokens, u.output_tokens, u.cache_read_tokens, u.cache_creation_tokens,
                u.reasoning_tokens, u.total_tokens, u.native_cost, u.cost_snapshot,
                u.pricing_source, u.unified_cost, u.unified_pricing_source, u.unified_cost_source
         FROM usage_records u JOIN remote_accounts a ON a.id = u.account_id
         WHERE ($1::bigint IS NULL OR u.account_id = $1)
           AND ($2::timestamptz IS NULL OR u.occurred_at >= $2)
           AND ($3::timestamptz IS NULL OR u.occurred_at < $3)
         ORDER BY u.occurred_at DESC, u.id DESC
         LIMIT $4 OFFSET $5",
    )
    .bind(filter.account_id)
    .bind(filter.from)
    .bind(filter.to)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?)
}

/// 过滤条件下全部记录的合计，不受分页影响。
pub async fn totals(pool: &PgPool, filter: &Filter) -> Result<Totals, AppError> {
    Ok(sqlx::query_as(
        "SELECT count(*)::bigint AS record_count,
                COALESCE(sum(total_tokens), 0)::bigint AS total_tokens,
                COALESCE(sum(cost_snapshot), 0)::float8 AS cost_snapshot_total,
                COALESCE(sum(unified_cost), 0)::float8 AS unified_cost_total,
                (count(*) FILTER (WHERE unified_pricing_source = 'unpriced'))::bigint
                    AS unified_unpriced_count
         FROM usage_records u
         WHERE ($1::bigint IS NULL OR u.account_id = $1)
           AND ($2::timestamptz IS NULL OR u.occurred_at >= $2)
           AND ($3::timestamptz IS NULL OR u.occurred_at < $3)",
    )
    .bind(filter.account_id)
    .bind(filter.from)
    .bind(filter.to)
    .fetch_one(pool)
    .await?)
}
