use std::time::Duration;

use chrono::{Local, Utc};
use tauri::{Emitter, Manager};

use crate::push::auto::{self, AutoPushDto};
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

fn auto_state_path() -> std::path::PathBuf {
    paths::app_data_dir().join(auto::STATE_FILE)
}

#[tauri::command]
pub fn get_auto_push() -> AutoPushDto {
    auto::panel(
        &auto_state_path(),
        &RemoteServerPaths::app_data(),
        Utc::now(),
    )
}

#[tauri::command]
pub fn set_auto_push(enabled: bool) -> Result<AutoPushDto, String> {
    auto::set_enabled(&auto_state_path(), enabled)?;
    Ok(get_auto_push())
}

/// 启动后等一会儿再第一次检查，别和首屏的摄取抢盘。
const AUTO_PUSH_FIRST_CHECK: Duration = Duration::from_secs(90);
/// 之后每小时检查一次：跨过午夜、或上次失败后重试，最多晚一小时。
const AUTO_PUSH_CHECK_INTERVAL: Duration = Duration::from_secs(3600);

/// 每日自动推送的调度线程。独立于托盘的摄取定时器；开关关着时每次醒来只读一个小文件。
pub fn spawn_auto_push(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(AUTO_PUSH_FIRST_CHECK);
        loop {
            run_auto_push_once(&app);
            std::thread::sleep(AUTO_PUSH_CHECK_INTERVAL);
        }
    });
}

fn run_auto_push_once(app: &tauri::AppHandle) {
    if !auto::load(&auto_state_path()).enabled {
        return;
    }
    // 手动推送或预览正在跑就让路，下个小时再看。
    let Ok(_guard) = push::RunGuard::acquire() else {
        return;
    };
    let state = app.state::<AppState>();
    let prices = state.effective_prices();
    let remote = RemoteServerPaths::app_data();
    let history = history_path();
    let home = ingest::default_home();
    let _ = auto::run_due(
        &PushEnv {
            conns: &*state,
            home: &home,
            prices: &prices,
            remote: &remote,
            history: &history,
            now: Utc::now(),
        },
        &auto_state_path(),
        &Local,
        // 应用刚启动时昨天的会话可能还没摄取进库：先让本机数据跟上再读。
        &|| crate::tray::refresh_if_stale(app),
    );
}
