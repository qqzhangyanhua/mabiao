use tauri::Manager;

use crate::domain::{DetectedEngine, WorkNotesDto, WorkNotesPreviewDto, WorkNotesRange};
use crate::paths;
use crate::work_notes::{self, ProcessRunner};
use crate::AppState;

#[tauri::command]
pub async fn preview_work_notes(
    app: tauri::AppHandle,
    range: WorkNotesRange,
) -> Result<WorkNotesPreviewDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        work_notes::preview(&conn, &prices, range, chrono::Local::now())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn build_work_notes(
    app: tauri::AppHandle,
    range: WorkNotesRange,
    engine_id: String,
    model: Option<String>,
    confirmed: Option<bool>,
) -> Result<WorkNotesDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        work_notes::build(
            &conn,
            &prices,
            range,
            chrono::Local::now(),
            &ProcessRunner,
            &paths::app_data_dir(),
            &engine_id,
            model.as_deref(),
            confirmed.unwrap_or(false),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn detect_work_note_engines() -> Result<Vec<DetectedEngine>, String> {
    tauri::async_runtime::spawn_blocking(work_notes::detect_engines)
        .await
        .map_err(|error| error.to_string())
}
