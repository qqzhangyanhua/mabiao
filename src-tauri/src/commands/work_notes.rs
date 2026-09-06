use tauri::Manager;

use crate::domain::{DetectedEngine, WorkNotesPreviewDto, WorkNotesProgressDto, WorkNotesRange};
use crate::paths;
use crate::work_notes::{self, ProcessRunner, RecordingRunner};
use crate::AppState;

#[tauri::command]
pub async fn preview_work_notes(
    app: tauri::AppHandle,
    range: WorkNotesRange,
    engine_id: Option<String>,
    model: Option<String>,
) -> Result<WorkNotesPreviewDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        work_notes::preview(
            &conn,
            &prices,
            range,
            chrono::Local::now(),
            engine_id.as_deref(),
            model.as_deref(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn start_work_notes(
    app: tauri::AppHandle,
    range: WorkNotesRange,
    engine_id: String,
    model: Option<String>,
    confirmed: Option<bool>,
) -> Result<(), String> {
    work_notes::require_engine(&engine_id)?;
    let state = app.state::<AppState>();
    state
        .work_notes
        .begin(&range, &engine_id, model.as_deref())?;
    let confirmed = confirmed.unwrap_or(false);
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let result = (|| {
            let prices = state.effective_prices();
            let prepared = {
                let conn = state.lock_read()?;
                work_notes::prepare(
                    &conn,
                    &prices,
                    range,
                    chrono::Local::now(),
                    confirmed,
                    false,
                )?
            };
            let process = ProcessRunner;
            let runner = RecordingRunner::new(&process, &engine_id);
            let dto = work_notes::run(
                prepared,
                &prices,
                &runner,
                &paths::app_data_dir(),
                &engine_id,
                model.as_deref(),
                &state.work_notes,
            );
            let records = runner.take();
            if !records.is_empty() {
                let conn = state.lock_write()?;
                work_notes::flush_generated(&conn, &records)?;
            }
            dto
        })();
        let _ = state.work_notes.finish(result);
    });
    Ok(())
}

#[tauri::command]
pub async fn get_work_notes_progress(
    app: tauri::AppHandle,
) -> Result<WorkNotesProgressDto, String> {
    let state = app.state::<AppState>();
    state.work_notes.snapshot()
}

#[tauri::command]
pub async fn cancel_work_notes(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.work_notes.request_cancel();
    Ok(())
}

#[tauri::command]
pub async fn detect_work_note_engines() -> Result<Vec<DetectedEngine>, String> {
    tauri::async_runtime::spawn_blocking(work_notes::detect_engines)
        .await
        .map_err(|error| error.to_string())
}
