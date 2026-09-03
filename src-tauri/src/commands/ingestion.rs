use crate::{budget, ingest, store};
use tauri::Manager;

use crate::domain::{IngestReport, Source, SourceDiagnostic};
use crate::{release_idle_memory, AppState};

#[tauri::command]
pub async fn ingest(app: tauri::AppHandle) -> Result<IngestReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        let report = ingest::ingest_all(&conn, &ingest::default_home())?;
        let prices = state.effective_prices();
        let _ = budget::check_and_notify(
            &app,
            &conn,
            &prices,
            &state.budget_path,
            &state.budget_notify_path,
        );
        release_idle_memory(&state, &conn);
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_source_diagnostics(
    app: tauri::AppHandle,
) -> Result<Vec<SourceDiagnostic>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        ingest::source_diagnostics(&conn, &ingest::default_home())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn rebuild_cache(
    app: tauri::AppHandle,
    source: Option<String>,
) -> Result<IngestReport, String> {
    let source = source
        .as_deref()
        .map(|value| Source::parse(value).ok_or_else(|| format!("未知来源：{value}")))
        .transpose()?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        let report = ingest::rebuild_cache(&conn, &ingest::default_home(), source)?;
        release_idle_memory(&state, &conn);
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 永久删除某来源（或全部来源）已归档的记录，供用户在设置页显式清理旧数据。
#[tauri::command]
pub async fn purge_archived_records(
    app: tauri::AppHandle,
    source: Option<String>,
) -> Result<u64, String> {
    let source = source
        .as_deref()
        .map(|value| Source::parse(value).ok_or_else(|| format!("未知来源：{value}")))
        .transpose()?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        // 删记录和重建预聚合表要么一起生效要么都不生效，中间态被读到就是错的数字。
        let transaction = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let removed = store::purge_archived(&transaction, source)?;
        if removed > 0 {
            store::rebuild_rollup(&transaction)?;
        }
        transaction.commit().map_err(|e| e.to_string())?;
        Ok(removed)
    })
    .await
    .map_err(|e| e.to_string())?
}
