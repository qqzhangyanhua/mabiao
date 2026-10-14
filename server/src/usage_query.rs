//! 消耗记录查询：同时给出统一费用与客户端费用快照，供跨人对比。

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::error::AppError;
use crate::summary::BreakdownRow;

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
    /// 只看这个项目。项目是团队共享的实体，成员仍只看到自己的那部分用量（账号过滤照常生效）。
    pub project_id: Option<i64>,
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
    /// 对上已推送会话时是 `sessions.id`，否则为空。
    pub session_pk: Option<i64>,
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

impl Totals {
    /// 把若干分组的合计再加起来（分组之间不重叠）。
    pub fn sum_of(rows: &[BreakdownRow]) -> Self {
        Self {
            record_count: rows.iter().map(|r| r.record_count).sum(),
            total_tokens: rows.iter().map(|r| r.total_tokens).sum(),
            cost_snapshot_total: rows.iter().map(|r| r.cost_snapshot).sum(),
            unified_cost_total: rows.iter().map(|r| r.unified_cost).sum(),
            unified_unpriced_count: rows.iter().map(|r| r.unpriced_count).sum(),
        }
    }
}
pub async fn list(
    pool: &PgPool,
    filter: &ListFilter,
    limit: i64,
    offset: i64,
) -> Result<Vec<UsageRow>, AppError> {
    let sql = format!(
        "SELECT u.id, u.account_id, a.account, u.occurred_at, u.source, u.model, u.provider,
                u.project_path, u.project_id, u.session_id, s.id AS session_pk,
                u.input_tokens, u.output_tokens, u.cache_read_tokens, u.cache_creation_tokens,
                u.reasoning_tokens, u.total_tokens, u.native_cost, u.cost_snapshot,
                u.pricing_source, u.unified_cost, u.unified_pricing_source, u.unified_cost_source
         FROM usage_records u
         JOIN remote_accounts a ON a.id = u.account_id
         LEFT JOIN sessions s
           ON s.account_id = u.account_id
          AND s.device_pk = u.device_pk
          AND s.source = u.source
          AND s.session_id = u.session_id
         WHERE {LIST_WHERE}
         ORDER BY u.occurred_at DESC, u.id DESC
         LIMIT $8 OFFSET $9"
    );
    Ok(sqlx::query_as(&sql)
        .bind(filter.scope.account_id)
        .bind(filter.scope.from)
        .bind(filter.scope.to)
        .bind(filter.scope.project_id)
        .bind(&filter.source)
        .bind(&filter.model)
        .bind(&filter.unified_cost_source)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?)
}

/// 与 [`list`] 同一套筛选下的合计，不受分页影响。
pub async fn filtered_totals(pool: &PgPool, filter: &ListFilter) -> Result<Totals, AppError> {
    let sql = format!(
        "SELECT count(*)::bigint AS record_count,
                COALESCE(sum(total_tokens), 0)::bigint AS total_tokens,
                COALESCE(sum(cost_snapshot), 0)::float8 AS cost_snapshot_total,
                COALESCE(sum(unified_cost), 0)::float8 AS unified_cost_total,
                (count(*) FILTER (WHERE unified_pricing_source = 'unpriced'))::bigint
                    AS unified_unpriced_count
         FROM usage_records u
         WHERE {LIST_WHERE}"
    );
    Ok(sqlx::query_as(&sql)
        .bind(filter.scope.account_id)
        .bind(filter.scope.from)
        .bind(filter.scope.to)
        .bind(filter.scope.project_id)
        .bind(&filter.source)
        .bind(&filter.model)
        .bind(&filter.unified_cost_source)
        .fetch_one(pool)
        .await?)
}

/// 明细列表额外的精确筛选。空表示不过滤。
#[derive(Debug, Clone)]
pub struct ListFilter {
    pub scope: Filter,
    pub source: Option<String>,
    pub model: Option<String>,
    pub unified_cost_source: Option<String>,
}

const LIST_WHERE: &str = "($1::bigint IS NULL OR u.account_id = $1)
           AND ($2::timestamptz IS NULL OR u.occurred_at >= $2)
           AND ($3::timestamptz IS NULL OR u.occurred_at < $3)
           AND ($4::bigint IS NULL OR u.project_id = $4)
           AND ($5::text IS NULL OR u.source = $5)
           AND ($6::text IS NULL OR u.model = $6)
           AND ($7::text IS NULL OR u.unified_cost_source = $7)";

pub fn parse_cost_source(value: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(text) = value.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    if !matches!(text, "native" | "team" | "snapshot" | "none") {
        return Err(AppError::invalid(
            "unified_cost_source 只能是 native、team、snapshot 或 none",
        ));
    }
    Ok(Some(text.to_owned()))
}

pub fn parse_exact(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

const SCOPE_WHERE: &str = "($1::bigint IS NULL OR u.account_id = $1)
           AND ($2::timestamptz IS NULL OR u.occurred_at >= $2)
           AND ($3::timestamptz IS NULL OR u.occurred_at < $3)
           AND ($4::bigint IS NULL OR u.project_id = $4)";

const TOKEN_SUMS: &str = "COALESCE(sum(u.input_tokens), 0)::bigint AS input,
                COALESCE(sum(u.output_tokens), 0)::bigint AS output,
                COALESCE(sum(u.cache_read_tokens), 0)::bigint AS cache_read,
                COALESCE(sum(u.cache_creation_tokens), 0)::bigint AS cache_creation,
                COALESCE(sum(u.reasoning_tokens), 0)::bigint AS reasoning";

/// 过滤条件下全部记录的合计，不受分页影响。
pub async fn totals(pool: &PgPool, filter: &Filter) -> Result<Totals, AppError> {
    let sql = format!(
        "SELECT count(*)::bigint AS record_count,
                COALESCE(sum(total_tokens), 0)::bigint AS total_tokens,
                COALESCE(sum(cost_snapshot), 0)::float8 AS cost_snapshot_total,
                COALESCE(sum(unified_cost), 0)::float8 AS unified_cost_total,
                (count(*) FILTER (WHERE unified_pricing_source = 'unpriced'))::bigint
                    AS unified_unpriced_count
         FROM usage_records u
         WHERE {SCOPE_WHERE}"
    );
    Ok(sqlx::query_as(&sql)
        .bind(filter.account_id)
        .bind(filter.from)
        .bind(filter.to)
        .bind(filter.project_id)
        .fetch_one(pool)
        .await?)
}

/// 五口径 token。网页用来画构成，不再把明细拉到浏览器里加总。
#[derive(Debug, Clone, Default, PartialEq, Eq, sqlx::FromRow)]
pub struct TokenParts {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    pub reasoning: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
struct CompositionSql {
    dimension: String,
    key: String,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_creation: i64,
    reasoning: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyedTokens {
    pub key: String,
    pub tokens: TokenParts,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct UnpricedModel {
    pub model: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composition {
    pub totals: TokenParts,
    pub by_model: Vec<KeyedTokens>,
    pub by_source: Vec<KeyedTokens>,
    pub by_project: Vec<KeyedTokens>,
    pub unpriced_models: Vec<UnpricedModel>,
}

/// 全量 token 构成。`unified_cost_source = 'none'` 才算未定价，三列都空是还没算过。
pub async fn composition(pool: &PgPool, filter: &Filter) -> Result<Composition, AppError> {
    let sql = format!(
        "SELECT 'total'::text AS dimension, ''::text AS key, {TOKEN_SUMS}
         FROM usage_records u WHERE {SCOPE_WHERE}
         UNION ALL
         SELECT 'model', u.model, {TOKEN_SUMS}
         FROM usage_records u WHERE {SCOPE_WHERE} GROUP BY u.model
         UNION ALL
         SELECT 'source', u.source, {TOKEN_SUMS}
         FROM usage_records u WHERE {SCOPE_WHERE} GROUP BY u.source
         UNION ALL
         SELECT 'project', COALESCE(u.project_id::text, ''), {TOKEN_SUMS}
         FROM usage_records u WHERE {SCOPE_WHERE} GROUP BY u.project_id"
    );
    let rows: Vec<CompositionSql> = sqlx::query_as(&sql)
        .bind(filter.account_id)
        .bind(filter.from)
        .bind(filter.to)
        .bind(filter.project_id)
        .fetch_all(pool)
        .await?;
    let mut out = Composition {
        totals: TokenParts::default(),
        by_model: Vec::new(),
        by_source: Vec::new(),
        by_project: Vec::new(),
        unpriced_models: Vec::new(),
    };
    for row in rows {
        let bucket = KeyedTokens {
            key: row.key,
            tokens: TokenParts {
                input: row.input,
                output: row.output,
                cache_read: row.cache_read,
                cache_creation: row.cache_creation,
                reasoning: row.reasoning,
            },
        };
        match row.dimension.as_str() {
            "total" => out.totals = bucket.tokens,
            "model" => out.by_model.push(bucket),
            "source" => out.by_source.push(bucket),
            "project" => out.by_project.push(bucket),
            _ => {}
        }
    }
    let unpriced_sql = format!(
        "SELECT u.model, count(*)::bigint AS count
         FROM usage_records u
         WHERE {SCOPE_WHERE} AND u.unified_cost_source = 'none'
         GROUP BY u.model
         ORDER BY count DESC, u.model ASC
         LIMIT 200"
    );
    out.unpriced_models = sqlx::query_as(&unpriced_sql)
        .bind(filter.account_id)
        .bind(filter.from)
        .bind(filter.to)
        .bind(filter.project_id)
        .fetch_all(pool)
        .await?;
    Ok(out)
}
