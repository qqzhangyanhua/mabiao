use axum::extract::DefaultBodyLimit;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use push_protocol::{check_protocol_version, LoginRequest, LoginResponse, RemoteRole};

use crate::api::{AccountView, CoverageView, CreateMemberRequest, DeviceView};
use crate::auth::{AdminAccount, AuthedAccount};
use crate::error::{ApiJson, AppError};
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
