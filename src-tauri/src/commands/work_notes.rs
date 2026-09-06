use tauri::Manager;

use crate::domain::{
    DetectedEngine, WorkNotesDto, WorkNotesHistoryPage, WorkNotesHistoryQuery, WorkNotesParams,
    WorkNotesPreviewDto, WorkNotesProgressDto, WorkNotesRange,
};
use crate::paths;
use crate::work_notes::{self, ProcessRunner, RecordingRunner};
use crate::AppState;

fn params(
    range: WorkNotesRange,
    extra_instructions: Option<String>,
    engine: Option<String>,
    model: Option<String>,
    confirmed: Option<bool>,
) -> WorkNotesParams {
    WorkNotesParams {
        range,
        extra_instructions: extra_instructions.unwrap_or_default(),
        engine: engine.unwrap_or_default(),
        model: model.unwrap_or_default(),
        confirmed: confirmed.unwrap_or(false),
    }
}

#[tauri::command]
pub async fn preview_work_notes(
    app: tauri::AppHandle,
    range: WorkNotesRange,
    extra_instructions: Option<String>,
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
            &params(range, extra_instructions, engine_id, model, None),
            chrono::Local::now(),
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
    extra_instructions: Option<String>,
    confirmed: Option<bool>,
) -> Result<(), String> {
    work_notes::require_engine(&engine_id)?;
    let state = app.state::<AppState>();
    state
        .work_notes
        .begin(&range, &engine_id, model.as_deref())?;
    let params = params(
        range,
        extra_instructions,
        Some(engine_id.clone()),
        model,
        confirmed,
    );
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let result = (|| {
            let prices = state.effective_prices();
            let now = chrono::Local::now();
            let prepared = {
                let conn = state.lock_read()?;
                work_notes::prepare(&conn, &prices, &params, now, false)?
            };
            let process = ProcessRunner;
            let runner = RecordingRunner::new(&process, params.engine_id());
            let output = work_notes::run(
                prepared,
                &prices,
                &runner,
                &paths::app_data_dir(),
                &params,
                now,
                &state.work_notes,
            );
            let records = runner.take();
            if !records.is_empty()
                || output.writes.notes.is_some()
                || !output.writes.sessions.is_empty()
            {
                let conn = state.lock_write()?;
                work_notes::flush_generated(&conn, &records)?;
                work_notes::persist_cache(&conn, &output.writes)?;
            }
            output.result
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

#[tauri::command]
pub async fn list_work_notes_history(
    app: tauri::AppHandle,
    query: WorkNotesHistoryQuery,
) -> Result<WorkNotesHistoryPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        work_notes::history(&conn, &query)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn get_work_notes_history_entry(
    app: tauri::AppHandle,
    id: i64,
) -> Result<WorkNotesDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        work_notes::history_entry(&conn, id)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn delete_work_notes_history_entry(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        work_notes::delete_history_entry(&conn, id)
    })
    .await
    .map_err(|error| error.to_string())?
}
