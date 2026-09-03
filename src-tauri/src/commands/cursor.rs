use crate::{adapters, cursor_account, cursor_session, cursor_session_detail, ingest, query};
use tauri::Manager;

use crate::domain::{
    CodeVolumeSummary, CursorAccountEventPage, CursorAccountEventQuery, CursorAccountUsageDto,
    CursorSessionDetailDto, CursorSessionPage, CursorSessionQuery, CursorSessionSummaryDto, Filter,
};
use crate::AppState;

#[tauri::command]
pub async fn get_code_volume(app: tauri::AppHandle) -> Result<CodeVolumeSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let summary = ingest::load_code_volume(&ingest::default_home())?;
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        // 代码量是至今累计口径。这里只取费用标量，不跑带 COUNT DISTINCT 的全量 overview。
        let (cost, unpriced) = query::lifetime_cost(&conn, &prices)?;
        Ok(adapters::cursor::with_cost_roi(summary, cost, unpriced))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_cursor_session_summary(
    app: tauri::AppHandle,
) -> Result<CursorSessionSummaryDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        cursor_session::load_summary(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_cursor_sessions_page(
    app: tauri::AppHandle,
    query: CursorSessionQuery,
) -> Result<CursorSessionPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        cursor_session::sessions_page(&conn, &query)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_cursor_session_detail(
    app: tauri::AppHandle,
    source_file: String,
) -> Result<CursorSessionDetailDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        cursor_session_detail::load_detail(&conn, &ingest::default_home(), &source_file)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_cursor_account_events_page(
    app: tauri::AppHandle,
    query: CursorAccountEventQuery,
) -> Result<CursorAccountEventPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        cursor_account::events_page(&conn, &query)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn refresh_cursor_account_usage(
    app: tauri::AppHandle,
) -> Result<CursorAccountUsageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = cursor_account::current_token()?;
        let state = app.state::<AppState>();
        let start_date_ms = {
            let conn = state.lock_read()?;
            cursor_account::incremental_start_ms(&conn)?
        };
        let pages = cursor_account::fetch_refresh_pages(&resolved, start_date_ms);
        let conn = state.lock_write()?;
        cursor_account::apply_fetched_pages(&conn, pages)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_cursor_account_usage(
    app: tauri::AppHandle,
    filter: Option<Filter>,
) -> Result<CursorAccountUsageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        cursor_account::load_summary_filtered(&conn, filter.as_ref())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn has_cursor_session_token() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(cursor_account::has_token)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_cursor_credential_status() -> Result<cursor_account::CursorCredentialStatus, String>
{
    tauri::async_runtime::spawn_blocking(cursor_account::credential_status)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clear_cursor_account_usage(
    app: tauri::AppHandle,
) -> Result<CursorAccountUsageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        cursor_account::clear_cache(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
}
