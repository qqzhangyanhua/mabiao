//! 消耗明细、token 构成、会话行为指标的 HTTP 入口。

use axum::extract::State;
use axum::Json;

use crate::api::{ScopeQuery, UsageQuery, UsageResponse};
use crate::auth::AuthedAccount;
use crate::behavior::{self, Behavior};
use crate::error::{ApiQuery, AppError};
use crate::routes::parse_paging;
use crate::scope::Scope;
use crate::usage_query::{self, Composition, ListFilter};
use crate::AppState;

pub async fn usage_list(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiQuery(query): ApiQuery<UsageQuery>,
) -> Result<Json<UsageResponse>, AppError> {
    let scope = Scope::resolve(&state.pool, &caller, query.scope_input()).await?;
    let (limit, offset) = parse_paging(query.limit, query.offset)?;
    let filter = ListFilter {
        scope,
        source: usage_query::parse_exact(query.source.as_deref()),
        model: usage_query::parse_exact(query.model.as_deref()),
        unified_cost_source: usage_query::parse_cost_source(query.unified_cost_source.as_deref())?,
    };
    let rows = usage_query::list(&state.pool, &filter, limit, offset).await?;
    let totals = usage_query::filtered_totals(&state.pool, &filter).await?;
    Ok(Json(UsageResponse {
        records: rows.into_iter().map(Into::into).collect(),
        totals,
    }))
}

pub async fn usage_composition(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiQuery(query): ApiQuery<ScopeQuery>,
) -> Result<Json<Composition>, AppError> {
    let scope = Scope::resolve(&state.pool, &caller, query.scope_input()).await?;
    Ok(Json(usage_query::composition(&state.pool, &scope).await?))
}

pub async fn session_behavior(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiQuery(query): ApiQuery<ScopeQuery>,
) -> Result<Json<Behavior>, AppError> {
    let scope = Scope::resolve(&state.pool, &caller, query.scope_input()).await?;
    Ok(Json(behavior::report(&state.pool, &scope).await?))
}
