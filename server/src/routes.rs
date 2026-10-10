use axum::extract::DefaultBodyLimit;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use push_protocol::{check_protocol_version, LoginRequest, LoginResponse, RemoteRole};

use crate::api::{
    AccountView, CoverageView, CreateMemberRequest, DeviceView, PricingOverview, RecomputeResponse,
    SetTeamPriceRequest, SetTeamPriceResponse, SnapshotMetaView, UsageQuery, UsageResponse,
};
use crate::auth::{AdminAccount, AuthedAccount};
use crate::error::{ApiJson, ApiQuery, AppError};
use crate::sessions::parse_timestamp;
use crate::team_pricing::{self, Scope};
use crate::usage_query::{self, Filter};
use crate::{accounts, coverage, devices, password, push, tokens, AppState};

/// 注意：这里没有注册 / 自助开户的路由，账号只能由管理员接口或服务端命令行创建。
pub fn router(state: AppState) -> Router {
    // 推送体可能很大；只在推送路由上放宽，其它接口保持 axum 默认上限。
    let push_routes = Router::new()
        .route("/api/v1/push/session", post(push::push_session))
        .route("/api/v1/push/usage", post(push::push_usage))
        .layer(DefaultBodyLimit::max(push::PUSH_BODY_LIMIT_BYTES));

    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/login", post(login))
        .route("/api/v1/me", get(me))
        .route("/api/v1/accounts/{id}/devices", get(account_devices))
        .route(
            "/api/v1/admin/accounts",
            get(admin_list_accounts).post(admin_create_member),
        )
        .route(
            "/api/v1/admin/accounts/{id}/deactivate",
            post(admin_deactivate_account),
        )
        .route("/api/v1/sessions/{id}", delete(push::delete_session))
        .route("/api/v1/admin/coverage", get(admin_coverage))
        .route("/api/v1/admin/pricing", get(admin_pricing))
        .route("/api/v1/admin/pricing/prices", put(admin_set_price))
        .route(
            "/api/v1/admin/pricing/prices/{id}",
            delete(admin_delete_price),
        )
        .route("/api/v1/admin/pricing/recompute", post(admin_recompute))
        .route("/api/v1/usage", get(usage_list))
        .merge(push_routes)
        .with_state(state)
}

async fn healthz(State(state): State<AppState>) -> Result<&'static str, AppError> {
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok("ok")
}

async fn login(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    check_protocol_version(request.protocol_version)?;

    let found = accounts::find_by_name(&state.pool, &request.account)
        .await?
        .filter(|row| row.active);

    let verified = match &found {
        Some(row) => password::verify_async(&request.password, &row.password_hash).await?,
        None => {
            password::verify_against_dummy_async(&request.password).await?;
            false
        }
    };
    let row = match (found, verified) {
        (Some(row), true) => row,
        _ => return Err(AppError::invalid_credentials()),
    };

    let (token, expires_at) = tokens::issue(&state.pool, row.id).await?;
    Ok(Json(LoginResponse {
        token,
        expires_at: expires_at.to_rfc3339(),
        role: row.role(),
        account: row.account,
    }))
}

async fn me(
    State(state): State<AppState>,
    caller: AuthedAccount,
) -> Result<Json<AccountView>, AppError> {
    let row = accounts::get(&state.pool, caller.id)
        .await?
        .ok_or_else(AppError::token_expired)?;
    Ok(Json(row.into()))
}

async fn account_devices(
    State(state): State<AppState>,
    caller: AuthedAccount,
    Path(account_id): Path<i64>,
) -> Result<Json<Vec<DeviceView>>, AppError> {
    if !caller.can_access(account_id) {
        return Err(AppError::forbidden("只能查看自己的数据"));
    }
    if accounts::get(&state.pool, account_id).await?.is_none() {
        return Err(AppError::not_found("账号不存在"));
    }
    let rows = devices::list(&state.pool, account_id).await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

async fn admin_list_accounts(
    State(state): State<AppState>,
    _admin: AdminAccount,
) -> Result<Json<Vec<AccountView>>, AppError> {
    let rows = accounts::list(&state.pool).await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

async fn admin_create_member(
    State(state): State<AppState>,
    _admin: AdminAccount,
    ApiJson(request): ApiJson<CreateMemberRequest>,
) -> Result<(StatusCode, Json<AccountView>), AppError> {
    let row = accounts::create(
        &state.pool,
        &request.account,
        &request.password,
        RemoteRole::Member,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

async fn admin_deactivate_account(
    State(state): State<AppState>,
    AdminAccount(admin): AdminAccount,
    Path(account_id): Path<i64>,
) -> Result<Json<AccountView>, AppError> {
    if admin.id == account_id {
        return Err(AppError::conflict("不能停用自己"));
    }
    let row = accounts::deactivate(&state.pool, account_id)
        .await?
        .ok_or_else(|| AppError::not_found("账号不存在"))?;
    Ok(Json(row.into()))
}

async fn admin_coverage(
    State(state): State<AppState>,
    _admin: AdminAccount,
) -> Result<Json<Vec<CoverageView>>, AppError> {
    let rows = coverage::list(&state.pool).await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

async fn admin_pricing(
    State(state): State<AppState>,
    _admin: AdminAccount,
) -> Result<Json<PricingOverview>, AppError> {
    let prices = team_pricing::list(&state.pool).await?;
    let snapshot = team_pricing::builtin_snapshot();
    Ok(Json(PricingOverview {
        prices: prices.into_iter().map(Into::into).collect(),
        snapshot: SnapshotMetaView {
            as_of: snapshot.as_of.clone(),
            source: snapshot.source.clone(),
            count: snapshot.entries.len(),
        },
    }))
}

/// 新增或原地修改一条团队价目，并立刻重算同名模型已入库的消耗记录。
async fn admin_set_price(
    State(state): State<AppState>,
    AdminAccount(admin): AdminAccount,
    ApiJson(request): ApiJson<SetTeamPriceRequest>,
) -> Result<Json<SetTeamPriceResponse>, AppError> {
    let price = team_pricing::normalize_price(&request)?;
    let mut tx = state.pool.begin().await?;
    team_pricing::lock_exclusive(&mut tx).await?;
    let row = team_pricing::upsert(&mut tx, &price, admin.id).await?;
    let table = team_pricing::load_table(&mut tx).await?;
    let recomputed = team_pricing::recompute(&mut tx, &table, Scope::Model(&row.model)).await?;
    tx.commit().await?;
    Ok(Json(SetTeamPriceResponse {
        price: row.into(),
        recomputed,
    }))
}

/// 删掉一条团队价目：该模型若没有别的团队价目，会重新回落到内置快照。
async fn admin_delete_price(
    State(state): State<AppState>,
    _admin: AdminAccount,
    Path(id): Path<i64>,
) -> Result<Json<RecomputeResponse>, AppError> {
    let mut tx = state.pool.begin().await?;
    team_pricing::lock_exclusive(&mut tx).await?;
    let model = team_pricing::delete(&mut tx, id)
        .await?
        .ok_or_else(|| AppError::not_found("团队价目不存在"))?;
    let table = team_pricing::load_table(&mut tx).await?;
    let recomputed = team_pricing::recompute(&mut tx, &table, Scope::Model(&model)).await?;
    tx.commit().await?;
    Ok(Json(RecomputeResponse { recomputed }))
}

/// 全量重算。服务升级后内置快照变了、或怀疑数据不一致时用。
async fn admin_recompute(
    State(state): State<AppState>,
    _admin: AdminAccount,
) -> Result<Json<RecomputeResponse>, AppError> {
    let recomputed = team_pricing::recompute_locked(&state.pool, Scope::All).await?;
    Ok(Json(RecomputeResponse { recomputed }))
}

fn parse_bound(
    name: &str,
    value: Option<&str>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, AppError> {
    value
        .map(|text| {
            parse_timestamp(text)
                .ok_or_else(|| AppError::invalid(format!("{name} 不是 RFC 3339 时间")))
        })
        .transpose()
}

/// 成员只看自己的；管理员默认看全体，可用 `account_id` 收窄。
/// 成员指定别人的 `account_id` 得到 403，与其它按账号归属的接口走同一个 `can_access`。
async fn usage_list(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiQuery(query): ApiQuery<UsageQuery>,
) -> Result<Json<UsageResponse>, AppError> {
    let account_id = match query.account_id {
        Some(id) if !caller.can_access(id) => {
            return Err(AppError::forbidden("只能查看自己的数据"));
        }
        Some(id) => Some(id),
        None if caller.is_admin() => None,
        None => Some(caller.id),
    };
    let limit = query.limit.unwrap_or(usage_query::DEFAULT_LIMIT);
    if !(1..=usage_query::MAX_LIMIT).contains(&limit) {
        return Err(AppError::invalid(format!(
            "limit 要在 1 到 {} 之间",
            usage_query::MAX_LIMIT
        )));
    }
    let offset = query.offset.unwrap_or(0);
    if offset < 0 {
        return Err(AppError::invalid("offset 不能为负"));
    }
    let filter = Filter {
        account_id,
        from: parse_bound("from", query.from.as_deref())?,
        to: parse_bound("to", query.to.as_deref())?,
    };
    let rows = usage_query::list(&state.pool, &filter, limit, offset).await?;
    let totals = usage_query::totals(&state.pool, &filter).await?;
    Ok(Json(UsageResponse {
        records: rows.into_iter().map(Into::into).collect(),
        totals: totals.into(),
    }))
}
