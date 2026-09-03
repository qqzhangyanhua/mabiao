use crate::{backup, clipboard, litellm, store, tray};
use std::fs;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use tauri::Manager;

use crate::AppState;

fn app_data_paths(state: &AppState) -> backup::AppDataPaths {
    backup::AppDataPaths {
        db_path: state.db_path.clone(),
        prices_path: state.prices_path.clone(),
        snapshot_path: state.snapshot_path.clone(),
        budget_path: state.budget_path.clone(),
        budget_notify_path: state.budget_notify_path.clone(),
        official_quota_path: state.official_quota_path.clone(),
        official_quota_notify_path: state.official_quota_notify_path.clone(),
    }
}

#[tauri::command]
pub async fn backup_data(app: tauri::AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dest = rfd::FileDialog::new()
            .set_title("选择备份目录")
            .pick_folder();
        let Some(base) = dest else {
            return Ok(false);
        };
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let dest = base.join(format!("mabiao-{stamp}"));
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        backup::backup_to(&conn, &dest, &app_data_paths(&state))?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 从备份目录恢复 sqlite 与用户配置，覆盖当前缓存。自定义提供商密钥不在备份里，本机已有的密钥文件不会被覆盖。返回 `false` 表示取消。
#[tauri::command]
pub async fn restore_data(app: tauri::AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let src = rfd::FileDialog::new()
            .set_title("选择备份目录")
            .pick_folder();
        let Some(src) = src else {
            return Ok(false);
        };
        let state = app.state::<AppState>();
        let paths = app_data_paths(&state);
        backup::validate_restore(&src)?;
        {
            let mut conn = state.lock_write()?;
            *conn = store::open_memory()?;
        }
        // 整池切到内存库，把所有指向备份目标文件的只读句柄都放掉。
        state.read_pool.replace_all(store::open_memory)?;
        let restored = backup::restore_from(&src, &paths);
        let db_path = paths.db_path.to_string_lossy().to_string();
        {
            let mut conn = state.lock_write()?;
            *conn = store::open_db(&db_path)?;
        }
        state
            .read_pool
            .replace_all(|| store::open_readonly(&db_path))?;
        restored?;
        let (snapshot, _) = litellm::load_snapshot(&paths.snapshot_path);
        *state.snapshot.lock().map_err(|e| e.to_string())? = snapshot;
        let _ = tray::refresh(&app);
        crate::spawn_event_index_backfill(&app);
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 弹出原生保存对话框并写入 CSV 内容；返回 `false` 表示用户取消。
#[tauri::command]
pub async fn export_csv(default_name: String, content: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("CSV", &["csv"])
            .save_file();
        match path {
            Some(path) => {
                // UTF-8 BOM 让 Excel 等工具正确识别中文，避免乱码。
                let mut bytes = vec![0xEF, 0xBB, 0xBF];
                bytes.extend_from_slice(content.as_bytes());
                fs::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(true)
            }
            None => Ok(false),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 弹出原生保存对话框并写入 JSON 内容；返回 `false` 表示用户取消。
#[tauri::command]
pub async fn export_json(default_name: String, content: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("JSON", &["json"])
            .save_file();
        match path {
            Some(path) => {
                fs::write(&path, content.as_bytes()).map_err(|e| e.to_string())?;
                Ok(true)
            }
            None => Ok(false),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn refresh_tray(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || tray::refresh(&app))
        .await
        .map_err(|e| e.to_string())?
}

/// 把 PNG（base64）解码成 RGBA 并写入系统剪贴板。不落盘。
#[tauri::command]
pub async fn copy_image_to_clipboard(base64: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || clipboard::copy_png_base64(&base64))
        .await
        .map_err(|e| e.to_string())?
}

/// 弹出原生保存对话框并写入图表 PNG（base64 编码）；返回 `false` 表示用户取消。
#[tauri::command]
pub async fn export_image(default_name: String, base64: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("PNG", &["png"])
            .save_file();
        match path {
            Some(path) => {
                let bytes = BASE64
                    .decode(base64.as_bytes())
                    .map_err(|e| e.to_string())?;
                fs::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(true)
            }
            None => Ok(false),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
