//! 消耗明细、token 构成、会话行为指标的 HTTP 入口。

use axum::extract::State;
use axum::Json;

use crate::api::{
    BehaviorView, CompositionBucketView, ScopeQuery, TokenPartsView, ToolStatView,
    UnpricedModelView, UsageCompositionView, UsageQuery, UsageResponse,
};
use crate::auth::AuthedAccount;
use crate::behavior::{self, Behavior, ToolStat};
use crate::error::{ApiQuery, AppError};
use crate::routes::{build_filter, parse_paging, scope_account};
use crate::usage_query::{self, Composition, KeyedTokens, ListFilter, TokenParts, UnpricedModel};
use crate::AppState;

pub async fn usage_list(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiQuery(query): ApiQuery<UsageQuery>,
) -> Result<Json<UsageResponse>, AppError> {
    let account_id = scope_account(&caller, query.account_id)?;
    let (limit, offset) = parse_paging(query.limit, query.offset)?;
    let filter = ListFilter {
        scope: build_filter(
            account_id,
            query.project_id,
            query.from.as_deref(),
            query.to.as_deref(),
        )?,
        source: usage_query::parse_exact(query.source.as_deref()),
        model: usage_query::parse_exact(query.model.as_deref()),
        unified_cost_source: usage_query::parse_cost_source(query.unified_cost_source.as_deref())?,
    };
    let rows = usage_query::list(&state.pool, &filter, limit, offset).await?;
    let totals = usage_query::filtered_totals(&state.pool, &filter).await?;
    Ok(Json(UsageResponse {
        records: rows.into_iter().map(Into::into).collect(),
        totals: totals.into(),
    }))
}

pub async fn usage_composition(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiQuery(query): ApiQuery<ScopeQuery>,
) -> Result<Json<UsageCompositionView>, AppError> {
    let account_id = scope_account(&caller, query.account_id)?;
    let filter = build_filter(
        account_id,
        query.project_id,
        query.from.as_deref(),
        query.to.as_deref(),
    )?;
    let composition = usage_query::composition(&state.pool, &filter).await?;
    Ok(Json(composition.into()))
}

pub async fn session_behavior(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiQuery(query): ApiQuery<ScopeQuery>,
) -> Result<Json<BehaviorView>, AppError> {
    let account_id = scope_account(&caller, query.account_id)?;
    let filter = build_filter(
        account_id,
        query.project_id,
        query.from.as_deref(),
        query.to.as_deref(),
    )?;
    let report = behavior::report(&state.pool, &filter).await?;
    Ok(Json(report.into()))
}

impl From<TokenParts> for TokenPartsView {
    fn from(tokens: TokenParts) -> Self {
        Self {
            input: tokens.input,
            output: tokens.output,
            cache_read: tokens.cache_read,
            cache_creation: tokens.cache_creation,
            reasoning: tokens.reasoning,
        }
    }
}

impl From<KeyedTokens> for CompositionBucketView {
    fn from(bucket: KeyedTokens) -> Self {
        Self {
            key: bucket.key,
            input: bucket.tokens.input,
            output: bucket.tokens.output,
            cache_read: bucket.tokens.cache_read,
            cache_creation: bucket.tokens.cache_creation,
            reasoning: bucket.tokens.reasoning,
        }
    }
}

impl From<UnpricedModel> for UnpricedModelView {
    fn from(row: UnpricedModel) -> Self {
        Self {
            model: row.model,
            count: row.count,
        }
    }
}

impl From<Composition> for UsageCompositionView {
    fn from(composition: Composition) -> Self {
        Self {
            totals: composition.totals.into(),
            by_model: composition.by_model.into_iter().map(Into::into).collect(),
            by_source: composition.by_source.into_iter().map(Into::into).collect(),
            by_project: composition.by_project.into_iter().map(Into::into).collect(),
            unpriced_models: composition
                .unpriced_models
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

impl From<ToolStat> for ToolStatView {
    fn from(row: ToolStat) -> Self {
        Self {
            name: row.name,
            calls: row.calls,
            failures: row.failures,
        }
    }
}

impl From<Behavior> for BehaviorView {
    fn from(report: Behavior) -> Self {
        Self {
            tool_calls: report.tool_calls,
            tool_failures: report.tool_failures,
            other_errors: report.other_errors,
            failure_rate: report.failure_rate,
            tools: report.tools.into_iter().map(Into::into).collect(),
            tools_truncated: report.tools_truncated,
        }
    }
}
