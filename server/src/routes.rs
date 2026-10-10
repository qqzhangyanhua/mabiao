use std::path::PathBuf;

use axum::extract::DefaultBodyLimit;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use push_protocol::{check_protocol_version, LoginRequest, LoginResponse, RemoteRole};
use tower_http::services::ServeDir;

use crate::api::{
    AccountView, CoverageView, CreateMemberRequest, DeviceView, PricingOverview, RecomputeResponse,
    SessionListQuery, SessionListResponse, SetTeamPriceRequest, SetTeamPriceResponse,
    SnapshotMetaView, SummaryQuery, SummaryResponse, UsageQuery, UsageResponse,
};
use crate::auth::{AdminAccount, AuthedAccount};
use crate::error::{ApiJson, ApiQuery, AppError};
use crate::sessions::{self, parse_timestamp};
use crate::summary;
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
        .route("/api/v1/usage/summary", get(usage_summary))
        .route("/api/v1/sessions", get(session_list))
        .merge(push_routes)
        .with_state(state)
}

/// 在 API 之上托管管理网页的构建产物。
///
/// 网页用 hash 路由，所以不需要「未知路径回退到 index.html」：那样会让拼错的 `/api/...`
/// 返回 200 的 HTML。API 路由先匹配，其余才落到静态文件。
pub fn router_with_web(state: AppState, web_dir: PathBuf) -> Router {
    router(state).fallback_service(ServeDir::new(web_dir))
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

fn build_filter(
    account_id: Option<i64>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Filter, AppError> {
    Ok(Filter {
        account_id,
        from: parse_bound("from", from)?,
        to: parse_bound("to", to)?,
    })
}

/// 成员只看自己的；管理员默认看全体，可用 `account_id` 收窄。
/// 成员指定别人的 `account_id` 得到 403，与其它按账号归属的接口走同一个 `can_access`。
fn scope_account(caller: &AuthedAccount, requested: Option<i64>) -> Result<Option<i64>, AppError> {
    match requested {
        Some(id) if !caller.can_access(id) => Err(AppError::forbidden("只能查看自己的数据")),
        Some(id) => Ok(Some(id)),
        None if caller.is_admin() => Ok(None),
        None => Ok(Some(caller.id)),
    }
}

fn parse_paging(limit: Option<i64>, offset: Option<i64>) -> Result<(i64, i64), AppError> {
    let limit = limit.unwrap_or(usage_query::DEFAULT_LIMIT);
    if !(1..=usage_query::MAX_LIMIT).contains(&limit) {
        return Err(AppError::invalid(format!(
            "limit 要在 1 到 {} 之间",
            usage_query::MAX_LIMIT
        )));
    }
    let offset = offset.unwrap_or(0);
    if offset < 0 {
        return Err(AppError::invalid("offset 不能为负"));
    }
    Ok((limit, offset))
}

async fn usage_list(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiQuery(query): ApiQuery<UsageQuery>,
) -> Result<Json<UsageResponse>, AppError> {
    let account_id = scope_account(&caller, query.account_id)?;
    let (limit, offset) = parse_paging(query.limit, query.offset)?;
    let filter = build_filter(account_id, query.from.as_deref(), query.to.as_deref())?;
    let rows = usage_query::list(&state.pool, &filter, limit, offset).await?;
    let totals = usage_query::totals(&state.pool, &filter).await?;
    Ok(Json(UsageResponse {
        records: rows.into_iter().map(Into::into).collect(),
        totals: totals.into(),
    }))
}

/// 网页的团队总览与成员页：按天、人、来源、模型、项目拆分，范围规则同 `usage_list`。
async fn usage_summary(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiQuery(query): ApiQuery<SummaryQuery>,
) -> Result<Json<SummaryResponse>, AppError> {
    let account_id = scope_account(&caller, query.account_id)?;
    let tz_offset = query.tz_offset_minutes.unwrap_or(0);
    if !(summary::MIN_TZ_OFFSET_MINUTES..=summary::MAX_TZ_OFFSET_MINUTES).contains(&tz_offset) {
        return Err(AppError::invalid("tz_offset_minutes 要在 -720 到 840 之间"));
    }
    let filter = build_filter(account_id, query.from.as_deref(), query.to.as_deref())?;
    let result = summary::build(&state.pool, &filter, tz_offset).await?;
    Ok(Json(result.into()))
}

/// 会话目录列表（不含正文）。范围规则同 `usage_list`。
async fn session_list(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiQuery(query): ApiQuery<SessionListQuery>,
) -> Result<Json<SessionListResponse>, AppError> {
    let account_id = scope_account(&caller, query.account_id)?;
    let (limit, offset) = parse_paging(query.limit, query.offset)?;
    let filter = build_filter(account_id, query.from.as_deref(), query.to.as_deref())?;
    let rows = sessions::list(&state.pool, &filter, limit, offset).await?;
    let total = sessions::count(&state.pool, &filter).await?;
    Ok(Json(SessionListResponse {
        sessions: rows.into_iter().map(Into::into).collect(),
        total,
    }))
}
