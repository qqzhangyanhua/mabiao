use chrono::Utc;

use crate::remote_server::{self, address, store::RemoteServerPaths, LoginInput, RemoteServerDto};

#[tauri::command]
pub fn get_remote_server() -> RemoteServerDto {
    remote_server::panel(&RemoteServerPaths::app_data(), Utc::now())
}

/// 表单在保存前就能提示地址是否合法；规则只在 Rust 有一份。
#[tauri::command]
pub fn validate_remote_server_url(base_url: String) -> Result<String, String> {
    address::normalize_base_url(&base_url)
}

#[tauri::command]
pub async fn remote_server_login(input: LoginInput) -> Result<RemoteServerDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        remote_server::login(&RemoteServerPaths::app_data(), input, Utc::now())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn remote_server_logout() -> Result<RemoteServerDto, String> {
    remote_server::logout(&RemoteServerPaths::app_data(), Utc::now())
}

#[tauri::command]
pub fn rename_remote_device(name: String) -> Result<RemoteServerDto, String> {
    remote_server::rename_device(&RemoteServerPaths::app_data(), &name, Utc::now())
}

#[tauri::command]
pub async fn verify_remote_server() -> Result<RemoteServerDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        remote_server::verify(&RemoteServerPaths::app_data(), Utc::now())
    })
    .await
    .map_err(|e| e.to_string())?
}
