use chrono::Utc;
use tauri::{Emitter, Manager};

use crate::push::history::{self, PushHistoryEntry};
use crate::push::{self, PushEnv, PushOutcome, PushPreviewDto, PushRange, PushRunInput};
use crate::remote_server::store::RemoteServerPaths;
use crate::{ingest, paths, AppState};

const PROGRESS_EVENT: &str = "push-progress";
const HISTORY_FILE: &str = "push_history.json";

fn history_path() -> std::path::PathBuf {
    paths::app_data_dir().join(HISTORY_FILE)
}

/// 预览只读本机、不联网：数出会推什么，用户确认后才调 `run_push`。
#[tauri::command]
pub async fn preview_push(
    app: tauri::AppHandle,
    range: PushRange,
) -> Result<PushPreviewDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = push::RunGuard::acquire()?;
        let state = app.state::<AppState>();
        let prices = state.effective_prices();
        let remote = RemoteServerPaths::app_data();
        let history = history_path();
        let home = ingest::default_home();
        push::preview(
            &PushEnv {
                conns: &*state,
                home: &home,
                prices: &prices,
                remote: &remote,
                history: &history,
                now: Utc::now(),
            },
            &range,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn run_push(app: tauri::AppHandle, input: PushRunInput) -> Result<PushOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = push::RunGuard::acquire()?;
        let state = app.state::<AppState>();
        let prices = state.effective_prices();
        let remote = RemoteServerPaths::app_data();
        let history = history_path();
        let home = ingest::default_home();
        push::run(
            &PushEnv {
                conns: &*state,
                home: &home,
                prices: &prices,
                remote: &remote,
                history: &history,
                now: Utc::now(),
            },
            &input,
            &|progress| {
                let _ = app.emit(PROGRESS_EVENT, progress);
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_push_history() -> Vec<PushHistoryEntry> {
    history::load(&history_path())
}
