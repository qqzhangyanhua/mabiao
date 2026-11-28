//! GitHub Copilot 设备码登录。
//!
//! 不监听本地端口，请求走 `net::agent_with_timeout`。client id 用 GitHub CLI
//! 公开的那一个，不内嵌 client secret。轮询间隔以服务端为准；`slow_down` 没给
//! 新间隔就再加 5 秒。取消、过期、拒绝都不写额度账号。

use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{upsert_account, AccountSecret, QuotaAccountPaths, QuotaAccountSource};
use crate::domain::OfficialQuotaProvider;
use crate::official_quota::{read_identity, QuotaIdentity};

/// GitHub CLI 公开的 OAuth client id。设备码交换只带这个 id。
const GITHUB_DEVICE_CLIENT_ID: &str = "178c6fc778ccc68e1d6a";
const DEVICE_SCOPE: &str = "read:user";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const USER_AGENT: &str = "mabiao";
const DEFAULT_INTERVAL_SECONDS: u64 = 5;
const DEFAULT_EXPIRES_SECONDS: u64 = 900;

const CANCELLED: &str = "已取消设备码授权，没有保存额度账号";
const EXPIRED: &str = "设备码已过期，没有保存额度账号。请重新发起授权";
const DENIED: &str = "授权已被拒绝，没有保存额度账号";
const IDENTITY_FAILED: &str = "无法确认 GitHub 额度账号，没有保存额度账号";
const NETWORK: &str = "无法连接 GitHub 授权服务，没有保存额度账号。请检查网络后重试";

/// 设备码、换令牌、读 GitHub 登录名三个地址。测试把它们指到回环桩。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotDeviceEndpoints {
    pub device_code_url: String,
    pub access_token_url: String,
    pub user_url: String,
}

impl CopilotDeviceEndpoints {
    pub fn github() -> Self {
        Self {
            device_code_url: "https://github.com/login/device/code".to_string(),
            access_token_url: "https://github.com/login/oauth/access_token".to_string(),
            user_url: "https://api.github.com/user".to_string(),
        }
    }

    pub fn from_base(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            device_code_url: format!("{base}/login/device/code"),
            access_token_url: format!("{base}/login/oauth/access_token"),
            user_url: format!("{base}/user"),
        }
    }
}

/// 交给界面的一次性验证码。`device_code` 不在这里。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopilotDeviceStartDto {
    pub session_id: String,
    pub user_code: String,
    pub verification_url: String,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CopilotDevicePoll {
    Pending { interval_seconds: u64 },
    Authorized { saved: super::SavedQuotaAccountDto },
}

struct DeviceState {
    device_code: String,
    interval_seconds: AtomicU64,
    expires_at: Instant,
    cancelled: AtomicBool,
    /// 下一次真正打授权接口的最早时刻。提前来问只交回间隔，不把窗口再往后推。
    not_before: Mutex<Instant>,
    endpoints: CopilotDeviceEndpoints,
}

/// 一次进行中的设备码会话。取消标记和轮询间隔可以跨线程看到。
#[derive(Clone)]
pub struct CopilotDeviceLogin {
    pub user_code: String,
    pub verification_url: String,
    state: Arc<DeviceState>,
}

impl std::fmt::Debug for CopilotDeviceLogin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CopilotDeviceLogin")
            .field("user_code", &self.user_code)
            .field("verification_url", &self.verification_url)
            .field("interval_seconds", &self.interval_seconds())
            .finish()
    }
}

impl CopilotDeviceLogin {
    pub fn interval_seconds(&self) -> u64 {
        self.state.interval_seconds.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::SeqCst);
    }

    fn cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::SeqCst)
    }
}

pub fn start_copilot_device_login(
    agent: &ureq::Agent,
    endpoints: &CopilotDeviceEndpoints,
) -> Result<CopilotDeviceLogin, String> {
    let body = post_form(
        agent,
        &endpoints.device_code_url,
        &[
            ("client_id", GITHUB_DEVICE_CLIENT_ID),
            ("scope", DEVICE_SCOPE),
        ],
    )?;
    if let Some(code) = json_str(&body, "error") {
        return Err(oauth_failure(code));
    }
    let user_code = json_str(&body, "user_code")
        .ok_or_else(|| "GitHub 没有返回设备验证码".to_string())?
        .to_string();
    let device_code = json_str(&body, "device_code")
        .ok_or_else(|| "GitHub 没有返回设备码".to_string())?
        .to_string();
    let verification_url = json_str(&body, "verification_uri")
        .or_else(|| json_str(&body, "verification_url"))
        .or_else(|| json_str(&body, "verification_uri_complete"))
        .ok_or_else(|| "GitHub 没有返回验证地址".to_string())?
        .to_string();
    ensure_github_https(&verification_url)?;
    let interval = polling_interval(json_u64(&body, "interval"));
    let expires_in = json_u64(&body, "expires_in").unwrap_or(DEFAULT_EXPIRES_SECONDS);
    Ok(CopilotDeviceLogin {
        user_code,
        verification_url,
        state: Arc::new(DeviceState {
            device_code,
            interval_seconds: AtomicU64::new(interval),
            expires_at: deadline(expires_in),
            cancelled: AtomicBool::new(false),
            not_before: Mutex::new(Instant::now()),
            endpoints: endpoints.clone(),
        }),
    })
}

pub fn poll_copilot_device_login(
    login: &CopilotDeviceLogin,
    agent: &ureq::Agent,
    paths: &QuotaAccountPaths,
    backoff_path: &std::path::Path,
) -> Result<CopilotDevicePoll, String> {
    if login.cancelled() {
        return Err(CANCELLED.to_string());
    }
    if Instant::now() >= login.state.expires_at {
        return Err(EXPIRED.to_string());
    }
    if too_early(&login.state) {
        return Ok(CopilotDevicePoll::Pending {
            interval_seconds: login.interval_seconds(),
        });
    }
    let device_code = login.state.device_code.clone();
    let body = post_form(
        agent,
        &login.state.endpoints.access_token_url,
        &[
            ("client_id", GITHUB_DEVICE_CLIENT_ID),
            ("device_code", device_code.as_str()),
            ("grant_type", GRANT_TYPE),
        ],
    )?;
    // 用户可能在这一次往返期间点了取消。令牌到了也不落盘。
    if login.cancelled() {
        return Err(CANCELLED.to_string());
    }
    if let Some(code) = json_str(&body, "error") {
        return match code {
            "authorization_pending" => {
                let interval = login.interval_seconds();
                schedule_next(&login.state, interval);
                Ok(CopilotDevicePoll::Pending {
                    interval_seconds: interval,
                })
            }
            "slow_down" => {
                let interval =
                    slowed_interval(login.interval_seconds(), json_u64(&body, "interval"));
                schedule_next(&login.state, interval);
                Ok(CopilotDevicePoll::Pending {
                    interval_seconds: interval,
                })
            }
            "expired_token" => Err(EXPIRED.to_string()),
            "access_denied" => Err(DENIED.to_string()),
            other => Err(oauth_failure(other)),
        };
    }
    let token = json_str(&body, "access_token")
        .filter(|token| !token.contains(char::is_whitespace))
        .ok_or_else(|| "GitHub 没有返回访问令牌，没有保存额度账号".to_string())?;
    if login.cancelled() {
        return Err(CANCELLED.to_string());
    }
    let refresh_token = json_str(&body, "refresh_token").map(str::to_string);
    let user = github_login(agent, &login.state.endpoints.user_url, token)?;
    if login.cancelled() {
        return Err(CANCELLED.to_string());
    }
    let identity = identity_for_login(token, &user)?;
    if login.cancelled() {
        return Err(CANCELLED.to_string());
    }
    let saved = upsert_account(
        paths,
        OfficialQuotaProvider::Copilot,
        &identity,
        AccountSecret {
            access_token: token.to_string(),
            refresh_token,
            account_id: Some(user),
            ..AccountSecret::default()
        },
        QuotaAccountSource::Oauth,
        backoff_path,
    )?;
    Ok(CopilotDevicePoll::Authorized { saved })
}

/// 只打开这次会话拿到的 https://github.com 验证页。
pub fn open_copilot_verification(url: &str) -> Result<(), String> {
    ensure_github_https(url)?;
    let mut command = browser_command(url)?;
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法打开验证页面：{error}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn github_login(agent: &ureq::Agent, url: &str, token: &str) -> Result<String, String> {
    let authorization = format!("Bearer {token}");
    let (status, text) = match agent
        .get(url)
        .set("Accept", "application/json")
        .set("User-Agent", USER_AGENT)
        .set("Authorization", &authorization)
        .call()
    {
        Ok(response) => (response.status(), read_response(response)?),
        Err(ureq::Error::Status(status, response)) => (status, read_response(response)?),
        Err(_) => return Err(NETWORK.to_string()),
    };
    let value = parse_oauth_body(&text).map_err(|_| {
        if status == 200 {
            IDENTITY_FAILED.to_string()
        } else {
            format!("GitHub 授权失败：HTTP {status}，没有保存额度账号")
        }
    })?;
    json_str(&value, "login")
        .map(str::to_string)
        .ok_or_else(|| IDENTITY_FAILED.to_string())
}

/// 写成 Copilot 插件认的 `apps.json`，再走和导入同一条 `read_identity`。
fn identity_for_login(token: &str, user: &str) -> Result<QuotaIdentity, String> {
    let dir = std::env::temp_dir().join(format!("mabiao-copilot-id-{:016x}", super::random_bits()));
    let outcome = (|| {
        let path = dir.join("github-copilot").join("apps.json");
        let bytes = serde_json::to_vec(&serde_json::json!({
            "github.com:device": {
                "oauth_token": token,
                "user": user,
            }
        }))
        .map_err(|error| error.to_string())?;
        super::write_atomic(&path, &bytes, true)?;
        read_identity(OfficialQuotaProvider::Copilot, &dir).map_err(|_| IDENTITY_FAILED.to_string())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    outcome
}

fn post_form(agent: &ureq::Agent, url: &str, fields: &[(&str, &str)]) -> Result<Value, String> {
    let (status, text) = match agent
        .post(url)
        .set("Accept", "application/json")
        .set("User-Agent", USER_AGENT)
        .send_form(fields)
    {
        Ok(response) => (response.status(), read_response(response)?),
        Err(ureq::Error::Status(status, response)) => (status, read_response(response)?),
        Err(_) => return Err(NETWORK.to_string()),
    };
    parse_oauth_body(&text).map_err(|_| {
        if status == 200 {
            "GitHub 授权响应无法识别".to_string()
        } else {
            format!("GitHub 授权失败：HTTP {status}，没有保存额度账号")
        }
    })
}

fn read_response(response: ureq::Response) -> Result<String, String> {
    response
        .into_string()
        .map_err(|error| format!("读取 GitHub 授权响应失败：{error}"))
}

fn parse_oauth_body(body: &str) -> Result<Value, ()> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(());
    }
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed).map_err(|_| ());
    }
    let mut map = serde_json::Map::new();
    for pair in trimmed.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = urlencoding::decode(key).map_err(|_| ())?.into_owned();
        let value = urlencoding::decode(value).map_err(|_| ())?.into_owned();
        if matches!(key.as_str(), "interval" | "expires_in") {
            if let Ok(number) = value.parse::<u64>() {
                map.insert(key, Value::from(number));
                continue;
            }
        }
        map.insert(key, Value::String(value));
    }
    if map.is_empty() {
        return Err(());
    }
    Ok(Value::Object(map))
}

fn json_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn json_u64(value: &Value, key: &str) -> Option<u64> {
    match value.get(key)? {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn polling_interval(seconds: Option<u64>) -> u64 {
    match seconds {
        Some(seconds) if seconds > 0 => seconds,
        _ => DEFAULT_INTERVAL_SECONDS,
    }
}

/// 响应里的 `interval` 优先；没有就按 RFC 8628 在当前间隔上加 5 秒。
fn slowed_interval(current: u64, server: Option<u64>) -> u64 {
    match server {
        Some(seconds) if seconds > 0 => seconds,
        _ => current.saturating_add(5),
    }
}

fn oauth_failure(code: &str) -> String {
    format!("GitHub 授权没有完成（{code}），没有保存额度账号")
}

fn deadline(seconds: u64) -> Instant {
    Instant::now()
        .checked_add(Duration::from_secs(seconds))
        .unwrap_or_else(Instant::now)
}

fn too_early(state: &DeviceState) -> bool {
    let not_before = state
        .not_before
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Instant::now() < *not_before
}

fn schedule_next(state: &DeviceState, interval: u64) {
    state.interval_seconds.store(interval, Ordering::SeqCst);
    let mut not_before = state
        .not_before
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *not_before = Instant::now()
        .checked_add(Duration::from_secs(interval))
        .unwrap_or_else(Instant::now);
}

fn ensure_github_https(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| "验证地址无效".to_string())?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("验证地址必须是 https://github.com".to_string());
    }
    let host = parsed.host_str().unwrap_or("");
    if host == "github.com" || host.ends_with(".github.com") {
        Ok(())
    } else {
        Err("验证地址必须是 https://github.com".to_string())
    }
}

fn browser_command(url: &str) -> Result<Command, String> {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(url);
        Ok(command)
    }
    #[cfg(target_os = "windows")]
    {
        windows_open(url)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        Ok(command)
    }
}

#[cfg(target_os = "windows")]
fn windows_open(url: &str) -> Result<Command, String> {
    use std::os::windows::process::CommandExt;
    if url.chars().any(|ch| matches!(ch, '"' | '\n' | '\r' | '\0')) {
        return Err("验证地址无效".to_string());
    }
    let mut command = Command::new("cmd");
    command.arg("/C");
    command.raw_arg("start \"\"");
    command.raw_arg(format!("\"{url}\""));
    Ok(command)
}
