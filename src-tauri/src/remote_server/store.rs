//! 远程服务的两份文件：配置一份、登录 token 一份（ADR 0026，对齐 ADR 0012）。
//!
//! 分开存是为了把 token 挡在备份之外：备份按白名单逐个文件拷，这两份都不在名单里。
//! 配置（含设备 ID）也不进备份——设备 ID 标识的是「这台机器」，换机器恢复备份后沿用旧 ID，
//! 两台机器就会在服务端互相覆盖会话。
//!
//! 密码从不落盘：它只在登录那一次请求里用，这里没有任何字段能放它。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use push_protocol::RemoteRole;
use serde::{Deserialize, Serialize};

pub const CONFIG_NAME: &str = "remote_server.json";
pub const TOKEN_NAME: &str = "remote_server_token.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteServerPaths {
    pub config: PathBuf,
    pub token: PathBuf,
}

impl RemoteServerPaths {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            config: dir.join(CONFIG_NAME),
            token: dir.join(TOKEN_NAME),
        }
    }

    pub fn app_data() -> Self {
        Self::in_dir(&crate::paths::app_data_dir())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteServerConfig {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub account: String,
    /// 首次登录生成，之后不变。随每次推送带上。
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub device_name: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredToken {
    /// 被远程服务拒绝后清空，只留 `rejected` 作为「需要重新登录」的记号。
    pub token: String,
    /// RFC 3339，来自登录响应。
    pub expires_at: String,
    pub account: String,
    pub role: RemoteRole,
    #[serde(default)]
    pub rejected: bool,
}

impl std::fmt::Debug for StoredToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredToken")
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("account", &self.account)
            .field("role", &self.role)
            .field("rejected", &self.rejected)
            .finish()
    }
}

/// 读不动或解析不了都按「没有」处理：用户重新登录一次即可，不该因为文件坏了让设置页打不开。
pub fn load_config(path: &Path) -> RemoteServerConfig {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn load_token(path: &Path) -> Option<StoredToken> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

pub fn save_config(path: &Path, config: &RemoteServerConfig) -> Result<(), String> {
    write_atomic(
        path,
        serde_json::to_string_pretty(config)
            .map_err(|e| e.to_string())?
            .as_bytes(),
        false,
    )
}

pub fn save_token(path: &Path, token: &StoredToken) -> Result<(), String> {
    write_atomic(
        path,
        serde_json::to_string_pretty(token)
            .map_err(|e| e.to_string())?
            .as_bytes(),
        true,
    )
}

/// 文件不存在算成功：退出登录要幂等。
pub fn clear_token(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("删除登录凭证失败：{error}")),
    }
}

/// 同目录临时文件再 `rename`。`private` 时临时文件创建就是 0600，
/// 不会出现「先按默认权限写出、再收权限」的可读窗口。
fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = parent.join(format!(".{file_name}.tmp"));
    let _ = fs::remove_file(&temp);

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;

    let written = options.open(&temp).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    if let Err(error) = written {
        let _ = fs::remove_file(&temp);
        return Err(format!("写入 {file_name} 失败：{error}"));
    }
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        format!("写入 {file_name} 失败：{error}")
    })
}
