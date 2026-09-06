use tauri::Manager;

use crate::domain::{WorkNotesDto, WorkNotesRange};
use crate::paths;
use crate::work_notes::{self, ProcessRunner};
use crate::AppState;

#[tauri::command]
pub async fn build_work_notes(
    app: tauri::AppHandle,
    range: WorkNotesRange,
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
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
