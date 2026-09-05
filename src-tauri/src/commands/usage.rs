use crate::{query, report};
use serde::Deserialize;
use tauri::Manager;

use crate::domain::{
    ApplicationAnalyticsDto, BillingWindowsDto, Filter, FilterOptions, LowCacheHitSessionsDto,
    NamedAmount, OverviewDto, ReportDto, ReportPeriod, SeriesPoint, SessionPage, SessionQuery,
    SessionRow, UnpricedGroupDto, UsageCallPage, WorkTimelineDto,
};
use crate::AppState;

#[tauri::command]
pub async fn get_overview(app: tauri::AppHandle, filter: Filter) -> Result<OverviewDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::overview(&conn, &filter, &prices)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_report(app: tauri::AppHandle, period: ReportPeriod) -> Result<ReportDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        report::build(&conn, &prices, period, chrono::Local::now())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_billing_windows(
    app: tauri::AppHandle,
    filter: Filter,
) -> Result<BillingWindowsDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::billing_windows(&conn, &filter, &prices, chrono::Utc::now())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_trend(
    app: tauri::AppHandle,
    filter: Filter,
    grain: String,
) -> Result<Vec<SeriesPoint>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::trend(&conn, &filter, &prices, &grain)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_application_analytics(
    app: tauri::AppHandle,
    filter: Filter,
    grain: String,
) -> Result<ApplicationAnalyticsDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        query::application_analytics(&conn, &filter, &grain)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_low_cache_hit_sessions(
    app: tauri::AppHandle,
    filter: Filter,
    source: String,
    limit: Option<u32>,
) -> Result<LowCacheHitSessionsDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        query::low_cache_hit_sessions(
            &conn,
            &filter,
            &source,
            usize::try_from(limit.unwrap_or(20)).unwrap_or(20),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Deserialize)]
pub(crate) struct NamedQuery {
    filter: Filter,
    dimension: String,
}

#[tauri::command]
pub async fn get_breakdown(
    app: tauri::AppHandle,
    query: NamedQuery,
) -> Result<Vec<NamedAmount>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::breakdown(&conn, &query.filter, &prices, &query.dimension)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_usage_calls_page(
    app: tauri::AppHandle,
    filter: Filter,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<UsageCallPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::usage_calls_page(
            &conn,
            &filter,
            &prices,
            page.unwrap_or(1),
            page_size.unwrap_or(20),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_top_sessions(
    app: tauri::AppHandle,
    filter: Filter,
    limit: Option<usize>,
) -> Result<Vec<SessionRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::top_sessions(&conn, &filter, &prices, limit.unwrap_or(20))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_sessions_page(
    app: tauri::AppHandle,
    filter: Filter,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<SessionPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::sessions_page(
            &conn,
            &prices,
            &SessionQuery {
                filter,
                sort_by: Some("time".into()),
                sort_dir: Some("desc".into()),
                page,
                page_size,
                include_cost: Some(true),
                ..SessionQuery::default()
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 单日工作时间线：只看 `day`（本地日历日 `YYYY-MM-DD`），独立于顶栏范围筛选。
#[tauri::command]
pub async fn get_work_timeline(
    app: tauri::AppHandle,
    day: String,
) -> Result<WorkTimelineDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        query::work_timeline(&conn, &day)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_filter_options(app: tauri::AppHandle) -> Result<FilterOptions, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        query::filter_options(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_unpriced_diagnosis(
    app: tauri::AppHandle,
) -> Result<Vec<UnpricedGroupDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::unpriced_diagnosis(&conn, &prices)
    })
    .await
    .map_err(|e| e.to_string())?
}
