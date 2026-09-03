use crate::{budget, ingest, litellm, scan_paths};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use tauri::Manager;

use crate::domain::{BudgetConfig, BudgetStatusDto, PriceSnapshotMeta, PriceTable};
use crate::{load_prices, AppState};

fn save_prices(path: &PathBuf, prices: &PriceTable) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(prices).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_prices(state: tauri::State<AppState>) -> PriceTable {
    load_prices(&state.prices_path)
}

#[tauri::command]
pub fn save_price_table(state: tauri::State<AppState>, prices: PriceTable) -> Result<(), String> {
    save_prices(&state.prices_path, &prices)
}

/// 当前自然月的预算执行情况：本地估算的月度费用、进度与预测，供设置页展示。
#[tauri::command]
pub async fn get_budget_status(app: tauri::AppHandle) -> Result<BudgetStatusDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        let config = budget::load_config(&state.budget_path);
        budget::status(&conn, &prices, &config, chrono::Local::now())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn save_budget(state: tauri::State<AppState>, config: BudgetConfig) -> Result<(), String> {
    budget::save_config(&state.budget_path, &config)
}

#[tauri::command]
pub fn get_scan_path_config() -> scan_paths::ScanPathPanelDto {
    scan_paths::panel(&scan_paths::config_path(), &ingest::default_home())
}

#[tauri::command]
pub fn save_scan_path_config(
    overrides: BTreeMap<String, Vec<String>>,
) -> Result<scan_paths::ScanPathPanelDto, String> {
    let home = ingest::default_home();
    let path = scan_paths::config_path();
    let config = scan_paths::normalize(overrides, &home)?;
    scan_paths::save(&path, &config)?;
    Ok(scan_paths::panel(&path, &home))
}

#[tauri::command]
pub async fn pick_directory(title: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(rfd::FileDialog::new()
            .set_title(&title)
            .pick_folder()
            .map(|path| path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 当前生效的 LiteLLM 价目快照元信息（内置或已刷新）。
#[tauri::command]
pub fn get_price_snapshot(state: tauri::State<AppState>) -> PriceSnapshotMeta {
    state.snapshot_meta()
}

/// 可选刷新：webview 拉取上游原始 JSON 后交给这里解析、落盘并热更新内存快照。
#[tauri::command]
pub async fn refresh_price_snapshot(
    app: tauri::AppHandle,
    raw: String,
) -> Result<PriceSnapshotMeta, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let as_of = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let snapshot = litellm::parse_litellm_raw(&raw, &as_of)?;
        if snapshot.entries.is_empty() {
            return Err("解析 LiteLLM 价目失败：未找到任何有效模型单价".to_string());
        }
        litellm::save_snapshot(&state.snapshot_path, &snapshot)?;
        let count = snapshot.entries.len();
        {
            let mut guard = state.snapshot.lock().map_err(|e| e.to_string())?;
            *guard = snapshot;
        }
        Ok(PriceSnapshotMeta {
            as_of,
            source: litellm::SOURCE_NAME.to_string(),
            count,
            bundled: false,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 恢复为内置快照：删除本地缓存并重载内置数据。
#[tauri::command]
pub async fn reset_price_snapshot(app: tauri::AppHandle) -> Result<PriceSnapshotMeta, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        if state.snapshot_path.exists() {
            std::fs::remove_file(&state.snapshot_path).map_err(|e| e.to_string())?;
        }
        let bundled = litellm::bundled_snapshot();
        let meta = PriceSnapshotMeta {
            as_of: bundled.as_of.clone(),
            source: bundled.source.clone(),
            count: bundled.entries.len(),
            bundled: true,
        };
        {
            let mut guard = state.snapshot.lock().map_err(|e| e.to_string())?;
            *guard = bundled;
        }
        Ok(meta)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 供 webview 拉取上游价目使用的固定地址。
#[tauri::command]
pub fn get_price_snapshot_url() -> String {
    litellm::SOURCE_URL.to_string()
}
