//! 额度账号配置与 0600 密钥文件的原子持久化与并发锁。

use std::path::Path;

use serde::Serialize;

use super::{QuotaAccountFile, SecretFile};

/// 续期成功后只改码表自己的凭证文件。同一把锁包住读改写，避免并发刷新互相覆盖。
pub(crate) fn store_refreshed_tokens(
    path: &Path,
    id: &str,
    access_token: &str,
    refresh_token: Option<&str>,
) -> Result<(), String> {
    let access_token = access_token.trim();
    if access_token.is_empty() {
        return Err("刷新响应里没有 access_token".to_string());
    }
    let rotated = refresh_token
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    with_secret_file(path, |file| {
        let entry = file.secrets.entry(id.to_string()).or_default();
        entry.access_token = access_token.to_string();
        // 旧的过期时刻描述的是上一张 access token。留着会让下一轮误判成还要续期。
        entry.access_expires_at = None;
        if let Some(refresh_token) = rotated {
            entry.refresh_token = Some(refresh_token);
        }
        Ok(())
    })
}

pub(super) fn forget_secret(path: &Path, id: &str) -> Result<(), String> {
    let _guard = secret_file_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if !path.exists() {
        return Ok(());
    }
    let mut secrets = load_secrets(path);
    if secrets.secrets.remove(id).is_none() {
        return Ok(());
    }
    save_secrets(path, &secrets)
}

fn with_secret_file(
    path: &Path,
    update: impl FnOnce(&mut SecretFile) -> Result<(), String>,
) -> Result<(), String> {
    let _guard = secret_file_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut secrets = load_secrets(path);
    update(&mut secrets)?;
    if secrets.secrets.is_empty() && !path.exists() {
        return Ok(());
    }
    save_secrets(path, &secrets)
}

fn secret_file_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    &LOCK
}

pub(super) fn load_config(path: &Path) -> QuotaAccountFile {
    load_json(path)
}

pub(super) fn load_secrets(path: &Path) -> SecretFile {
    load_json(path)
}

fn load_json<T: Default + serde::de::DeserializeOwned>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(super) fn save_config(path: &Path, config: &QuotaAccountFile) -> Result<(), String> {
    write_json(path, config, false)
}

pub(super) fn save_secrets(path: &Path, secrets: &SecretFile) -> Result<(), String> {
    write_json(path, secrets, true)
}

fn write_json<T: Serialize>(path: &Path, value: &T, private: bool) -> Result<(), String> {
    let bytes = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    write_atomic(path, bytes.as_bytes(), private)
}

/// 同目录临时文件再 `rename`。`private` 时临时文件创建就是 0600。
pub(super) fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = parent.join(format!(".{file_name}.tmp"));
    let _ = std::fs::remove_file(&temp);

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;

    let written = options.open(&temp).and_then(|mut file| {
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()
    });
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("写入 {file_name} 失败：{error}"));
    }
    std::fs::rename(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        format!("写入 {file_name} 失败：{error}")
    })
}

pub(super) fn new_account_id(slug: &str, taken: &[String]) -> String {
    for _ in 0..64 {
        let candidate = format!("acct:{slug}:{:016x}", random_bits());
        if !taken.iter().any(|id| id == &candidate) {
            return candidate;
        }
    }
    format!("acct:{slug}:{:016x}", random_bits())
}

pub(super) fn random_bits() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos() as u64),
    );
    hasher.finish()
}
