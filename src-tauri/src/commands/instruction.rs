use crate::{instructions, query};
use tauri::Manager;

use crate::domain::{GlobalInstructionDto, WriteUserFileRequest, WriteUserFileResult};
use crate::AppState;

#[tauri::command]
pub async fn get_global_instructions(
    app: tauri::AppHandle,
    project: Option<String>,
) -> Result<GlobalInstructionDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = dirs::home_dir().ok_or_else(|| "无法确定用户主目录".to_string())?;
        let state = app.state::<AppState>();
        let (recent, usage) = {
            let conn = state.lock_read()?;
            (
                query::recent_projects(&conn)?,
                query::source_token_totals(&conn)?,
            )
        };
        Ok(instructions::scan_for_projects(
            &home,
            project.as_deref(),
            &recent,
            &usage,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn write_global_instruction(
    request: WriteUserFileRequest,
) -> Result<WriteUserFileResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = dirs::home_dir().ok_or_else(|| "无法确定用户主目录".to_string())?;
        crate::user_files::write(
            &home,
            &crate::paths::app_data_dir(),
            std::path::Path::new(&request.abs_path),
            &request.content,
            request.expected_mtime.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_global_instruction(abs_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || instructions::open_in_external_editor(&abs_path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_cursor_instruction_settings() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(instructions::cursor::open_settings)
        .await
        .map_err(|e| e.to_string())?
}
