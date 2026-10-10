//! 设置页的「远程服务」：登录换 token、设备身份、退出登录（ADR 0026）。
//!
//! 只管凭证与身份，不推送任何数据。联网只发生在这里与 `client`，webview 不直接请求远程服务。
//! 命令层负责把阻塞的联网放进 `spawn_blocking`；这里的函数都是同步的，`now` 与路径由调用方注入。

pub mod address;
pub mod client;
pub mod store;

use chrono::{DateTime, Utc};
use push_protocol::RemoteRole;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use client::RemoteError;
use store::{RemoteServerConfig, RemoteServerPaths, StoredToken};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    NotLoggedIn,
    LoggedIn,
    /// 本机记录的有效期已过（30 天）。
    Expired,
    /// 有效期内但被远程服务拒绝（账号被停用、服务端重置等）。
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteServerDto {
    pub base_url: String,
    pub account: String,
    /// 首次登录前为 `None`。
    pub device_id: Option<String>,
    pub device_name: String,
    pub state: SessionState,
    pub role: Option<RemoteRole>,
    pub expires_at: Option<String>,
    /// 需要用户处理时的一句中文提示（过期、被拒）。
    pub notice: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct LoginInput {
    pub base_url: String,
    pub account: String,
    pub password: String,
    /// 留空沿用已有设备名；首次登录且留空则取本机主机名。
    pub device_name: Option<String>,
}

impl std::fmt::Debug for LoginInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginInput")
            .field("base_url", &self.base_url)
            .field("account", &self.account)
            .field("password", &"<redacted>")
            .field("device_name", &self.device_name)
            .finish()
    }
}

const EXPIRED_NOTICE: &str = "登录已过期（有效期 30 天），请重新登录";
const REJECTED_NOTICE: &str =
    "远程服务不再接受这次登录（账号可能被停用，或服务端被重置），请重新登录";

pub fn panel(paths: &RemoteServerPaths, now: DateTime<Utc>) -> RemoteServerDto {
    dto(
        &store::load_config(&paths.config),
        store::load_token(&paths.token).as_ref(),
        now,
    )
}

pub fn login(
    paths: &RemoteServerPaths,
    input: LoginInput,
    now: DateTime<Utc>,
) -> Result<RemoteServerDto, String> {
    let base_url = address::normalize_base_url(&input.base_url)?;
    let account = input.account.trim();
    if account.is_empty() {
        return Err("请填写远程账号".to_string());
    }
    if input.password.is_empty() {
        return Err("请填写密码".to_string());
    }

    let response = client::login(&base_url, account, &input.password).map_err(into_login_error)?;
    if DateTime::parse_from_rfc3339(&response.expires_at).is_err() || response.token.is_empty() {
        return Err("远程服务返回的登录信息不完整，请检查地址是否填对".to_string());
    }

    let previous = store::load_config(&paths.config);
    let device_id = if previous.device_id.is_empty() {
        new_device_id()
    } else {
        previous.device_id.clone()
    };
    let device_name = [input.device_name.as_deref(), Some(&previous.device_name)]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|name| !name.is_empty())
        .map_or_else(default_device_name, str::to_string);

    let config = RemoteServerConfig {
        base_url,
        account: response.account.clone(),
        device_id,
        device_name,
    };
    let token = StoredToken {
        token: response.token,
        expires_at: response.expires_at,
        account: response.account,
        role: response.role,
        rejected: false,
    };
    // 先配置后 token：中途失败时落到「未登录」，而不是「新 token 配着旧地址与账号」。
    store::save_config(&paths.config, &config)?;
    store::save_token(&paths.token, &token)?;
    Ok(dto(&config, Some(&token), now))
}

/// 退出登录：删 token 文件。配置留着，下次登录表单还是填好的。
pub fn logout(paths: &RemoteServerPaths, now: DateTime<Utc>) -> Result<RemoteServerDto, String> {
    store::clear_token(&paths.token)?;
    Ok(panel(paths, now))
}

pub fn rename_device(
    paths: &RemoteServerPaths,
    name: &str,
    now: DateTime<Utc>,
) -> Result<RemoteServerDto, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("设备名不能为空".to_string());
    }
    let mut config = store::load_config(&paths.config);
    if config.device_id.is_empty() {
        return Err("还没有登录过，登录后才会生成设备".to_string());
    }
    config.device_name = name.to_string();
    store::save_config(&paths.config, &config)?;
    Ok(panel(paths, now))
}

/// 向远程服务确认这次登录还有效。只有明确被拒（401 token_expired）才把本机登录态标成需要重登；
/// 断网、服务端暂时 5xx 不动它，只把原因报给用户。
pub fn verify(paths: &RemoteServerPaths, now: DateTime<Utc>) -> Result<RemoteServerDto, String> {
    let config = store::load_config(&paths.config);
    let Some(mut token) = store::load_token(&paths.token) else {
        return Ok(panel(paths, now));
    };
    if session_state(Some(&token), now) != SessionState::LoggedIn {
        return Ok(dto(&config, Some(&token), now));
    }
    let base_url = address::normalize_base_url(&config.base_url)?;
    match client::check_token(&base_url, &token.token) {
        Ok(()) => {}
        Err(RemoteError::TokenRejected) => {
            token.token.clear();
            token.rejected = true;
            store::save_token(&paths.token, &token)?;
        }
        Err(other) => return Err(other.into_message()),
    }
    Ok(panel(paths, now))
}

/// 登录接口对「token 被拒」没有意义，万一网关乱回 401 也按普通失败报。
fn into_login_error(error: RemoteError) -> String {
    match error {
        RemoteError::TokenRejected => "远程服务拒绝了这次登录，请检查地址是否填对".to_string(),
        other => other.into_message(),
    }
}

fn session_state(token: Option<&StoredToken>, now: DateTime<Utc>) -> SessionState {
    let Some(token) = token else {
        return SessionState::NotLoggedIn;
    };
    if token.rejected {
        return SessionState::Rejected;
    }
    if token.token.is_empty() {
        return SessionState::NotLoggedIn;
    }
    match DateTime::parse_from_rfc3339(&token.expires_at) {
        Ok(expires_at) if expires_at > now => SessionState::LoggedIn,
        _ => SessionState::Expired,
    }
}

fn dto(
    config: &RemoteServerConfig,
    token: Option<&StoredToken>,
    now: DateTime<Utc>,
) -> RemoteServerDto {
    let state = session_state(token, now);
    RemoteServerDto {
        base_url: config.base_url.clone(),
        account: config.account.clone(),
        device_id: (!config.device_id.is_empty()).then(|| config.device_id.clone()),
        device_name: config.device_name.clone(),
        state,
        role: token.map(|token| token.role),
        expires_at: token
            .filter(|_| state != SessionState::NotLoggedIn)
            .map(|token| token.expires_at.clone()),
        notice: match state {
            SessionState::Expired => Some(EXPIRED_NOTICE.to_string()),
            SessionState::Rejected => Some(REJECTED_NOTICE.to_string()),
            _ => None,
        },
    }
}

/// 设备 ID 只要在一个团队里不撞即可，不需要密码学强度。
/// 混入时间、进程号与操作系统给 `RandomState` 的随机种子，取 SHA-256 前 128 位。
fn new_device_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut seed = Sha256::new();
    seed.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos())
            .to_le_bytes(),
    );
    seed.update(std::process::id().to_le_bytes());
    for _ in 0..2 {
        seed.update(
            std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish()
                .to_le_bytes(),
        );
    }
    seed.finalize()[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn default_device_name() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "本机".to_string())
}
