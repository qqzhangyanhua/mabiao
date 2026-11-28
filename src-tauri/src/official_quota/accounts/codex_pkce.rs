//! Codex 公开 PKCE 浏览器授权。
//!
//! 公开客户端标识来自 `codex_usage::OAUTH_CLIENT_ID`，没有 client secret。
//! 能绑上 `127.0.0.1` 就临时听回调；绑不上就把说明交回界面，同一套状态校验
//! 接受粘贴的回调链接。取消会停掉监听。令牌交换只走调用方给的 agent。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use super::{upsert_account, AccountSecret, QuotaAccountPaths, QuotaAccountSource};
use crate::domain::OfficialQuotaProvider;
use crate::official_quota::codex_usage::OAUTH_CLIENT_ID;
use crate::official_quota::{read_identity, QuotaIdentity};

/// Codex CLI 注册过的回环端口。授权地址里的 redirect_uri 用这一端口。
pub const CODEX_PKCE_REDIRECT_PORT: u16 = 1455;
const REDIRECT_PATH: &str = "/auth/callback";
const SCOPE: &str = "openid profile email offline_access";
const USER_AGENT: &str = "mabiao";

const CANCELLED: &str = "已取消浏览器授权，没有保存额度账号";
const STATE_MISMATCH: &str = "回调状态对不上，没有保存额度账号";
const PORT_TAKEN_HINT: &str =
    "本地回环端口已被占用。请在浏览器完成登录后，把地址栏里的回调链接粘贴到下面";
const NO_CODE: &str = "回调链接里没有授权码，没有保存额度账号";
const NO_STATE: &str = "回调链接里没有状态参数，没有保存额度账号";
const BAD_URL: &str = "回调链接无法识别，没有保存额度账号";
const EMPTY_URL: &str = "请粘贴浏览器地址栏里的回调链接";
const NETWORK: &str = "无法连接 Codex 登录服务，没有保存额度账号。请检查网络后重试";
const NO_TOKEN: &str = "Codex 没有返回访问令牌，没有保存额度账号";
const NO_REFRESH: &str = "Codex 没有返回刷新令牌，没有保存额度账号";
const IDENTITY_FAILED: &str = "无法确认 Codex 额度账号，没有保存额度账号";
const DENIED: &str = "授权没有完成，没有保存额度账号";

/// 授权页和令牌端点。测试把它们指到回环桩。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexPkceEndpoints {
    pub authorize_url: String,
    pub token_url: String,
}

impl CodexPkceEndpoints {
    pub fn openai() -> Self {
        Self {
            authorize_url: "https://auth.openai.com/oauth/authorize".to_string(),
            token_url: crate::official_quota::codex_usage::OAUTH_TOKEN_URL.to_string(),
        }
    }

    pub fn from_base(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            authorize_url: format!("{base}/oauth/authorize"),
            token_url: format!("{base}/oauth/token"),
        }
    }
}

/// 交给界面的授权地址。`code_verifier` 不在这里。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodexPkceStartDto {
    pub session_id: String,
    pub authorize_url: String,
    pub redirect_uri: String,
    pub listening: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paste_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CodexPkcePoll {
    Pending,
    Authorized { saved: super::SavedQuotaAccountDto },
}

enum Slot {
    Waiting,
    Ready(String),
    Busy,
    Finished(Result<super::SavedQuotaAccountDto, String>),
}

struct Shared {
    cancelled: AtomicBool,
    /// 停监听，但不算用户取消。粘贴回调和交换完成都走这里。
    stop: AtomicBool,
    slot: Mutex<Slot>,
    join: Mutex<Option<std::thread::JoinHandle<()>>>,
    local_addr: Mutex<Option<SocketAddr>>,
    verifier: String,
    state: String,
    redirect_uri: String,
    token_url: String,
}

/// 一次进行中的浏览器授权。取消标记和监听线程可以跨线程看到。
#[derive(Clone)]
pub struct CodexPkceLogin {
    authorize_url: String,
    listening: bool,
    paste_hint: Option<String>,
    shared: Arc<Shared>,
}

impl std::fmt::Debug for CodexPkceLogin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CodexPkceLogin")
            .field("authorize_url", &self.authorize_url)
            .field("listening", &self.listening)
            .field("redirect_uri", &self.shared.redirect_uri)
            .finish()
    }
}

impl CodexPkceLogin {
    pub fn authorize_url(&self) -> &str {
        &self.authorize_url
    }

    pub fn redirect_uri(&self) -> &str {
        &self.shared.redirect_uri
    }

    pub fn listening(&self) -> bool {
        self.listening
    }

    pub fn listen_port(&self) -> Option<u16> {
        if !self.listening {
            return None;
        }
        self.shared
            .local_addr
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .map(|addr| addr.port())
    }

    pub fn paste_hint(&self) -> Option<&str> {
        self.paste_hint.as_deref()
    }

    pub fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::SeqCst);
        self.shutdown();
    }

    fn cancelled(&self) -> bool {
        self.shared.cancelled.load(Ordering::SeqCst)
    }

    fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let addr = *self
            .shared
            .local_addr
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(addr) = addr {
            let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(200));
        }
        if let Some(handle) = self
            .shared
            .join
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = handle.join();
        }
    }
}

pub fn start_codex_pkce_login(
    endpoints: &CodexPkceEndpoints,
    listen_port: u16,
) -> Result<CodexPkceLogin, String> {
    let (verifier, challenge) = pkce_pair()?;
    let state = random_token()?;
    let (listener, bound_port) = bind_loopback(listen_port)?;
    let redirect_port = bound_port.unwrap_or(listen_port);
    let redirect_uri = redirect_uri_for(redirect_port);
    let authorize_url = authorize_url(endpoints, &redirect_uri, &challenge, &state)?;
    let listening = bound_port.is_some();
    let shared = Arc::new(Shared {
        cancelled: AtomicBool::new(false),
        stop: AtomicBool::new(false),
        slot: Mutex::new(Slot::Waiting),
        join: Mutex::new(None),
        local_addr: Mutex::new(None),
        verifier,
        state,
        redirect_uri,
        token_url: endpoints.token_url.clone(),
    });
    if let Some(listener) = listener {
        let addr = listener
            .local_addr()
            .map_err(|error| format!("无法监听本地回调：{error}"))?;
        *shared
            .local_addr
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(addr);
        let shared_thread = Arc::clone(&shared);
        let handle = std::thread::spawn(move || serve_callback(listener, shared_thread));
        *shared
            .join
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handle);
    }
    Ok(CodexPkceLogin {
        authorize_url,
        listening,
        paste_hint: if listening {
            None
        } else {
            Some(PORT_TAKEN_HINT.to_string())
        },
        shared,
    })
}

pub fn poll_codex_pkce_login(
    login: &CodexPkceLogin,
    agent: &ureq::Agent,
    paths: &QuotaAccountPaths,
    backoff_path: &std::path::Path,
) -> Result<CodexPkcePoll, String> {
    if login.cancelled() {
        return Err(CANCELLED.to_string());
    }
    let code = {
        let mut slot = login
            .shared
            .slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &mut *slot {
            Slot::Waiting => return Ok(CodexPkcePoll::Pending),
            Slot::Busy => return Ok(CodexPkcePoll::Pending),
            Slot::Ready(code) => {
                let code = std::mem::take(code);
                *slot = Slot::Busy;
                code
            }
            Slot::Finished(result) => {
                let copied = clone_result(result);
                drop(slot);
                login.shutdown();
                return copied.map(|saved| CodexPkcePoll::Authorized { saved });
            }
        }
    };
    let result = exchange_and_store(login, agent, &code, paths, backoff_path);
    finish(login, result.clone());
    result.map(|saved| CodexPkcePoll::Authorized { saved })
}

pub fn submit_codex_pkce_callback(
    login: &CodexPkceLogin,
    callback_url: &str,
    agent: &ureq::Agent,
    paths: &QuotaAccountPaths,
    backoff_path: &std::path::Path,
) -> Result<super::SavedQuotaAccountDto, String> {
    if login.cancelled() {
        return Err(CANCELLED.to_string());
    }
    let (code, state) = parse_callback(callback_url)?;
    if state != login.shared.state {
        let result = Err(STATE_MISMATCH.to_string());
        finish(login, result.clone());
        return result;
    }
    {
        let mut slot = login
            .shared
            .slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*slot {
            Slot::Finished(result) => return clone_result(result),
            Slot::Busy => return Err("正在完成授权，请稍候".to_string()),
            Slot::Waiting | Slot::Ready(_) => *slot = Slot::Busy,
        }
    }
    login.shutdown();
    let result = exchange_and_store(login, agent, &code, paths, backoff_path);
    finish(login, result.clone());
    result
}

/// 只打开这次会话生成的 https://auth.openai.com 授权页。
pub fn open_codex_authorize(url: &str) -> Result<(), String> {
    ensure_openai_authorize(url)?;
    let mut command = browser_command(url)?;
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法打开授权页面：{error}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn clone_result(
    result: &Result<super::SavedQuotaAccountDto, String>,
) -> Result<super::SavedQuotaAccountDto, String> {
    match result {
        Ok(saved) => Ok(saved.clone()),
        Err(error) => Err(error.clone()),
    }
}

fn finish(login: &CodexPkceLogin, result: Result<super::SavedQuotaAccountDto, String>) {
    {
        let mut slot = login
            .shared
            .slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Slot::Finished(result);
    }
    login.shutdown();
}

fn exchange_and_store(
    login: &CodexPkceLogin,
    agent: &ureq::Agent,
    code: &str,
    paths: &QuotaAccountPaths,
    backoff_path: &std::path::Path,
) -> Result<super::SavedQuotaAccountDto, String> {
    if login.cancelled() {
        return Err(CANCELLED.to_string());
    }
    let body = post_form(
        agent,
        &login.shared.token_url,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", OAUTH_CLIENT_ID),
            ("code", code),
            ("redirect_uri", login.shared.redirect_uri.as_str()),
            ("code_verifier", login.shared.verifier.as_str()),
        ],
    )?;
    if login.cancelled() {
        return Err(CANCELLED.to_string());
    }
    if let Some(code) = json_str(&body, "error") {
        return Err(format!("Codex 授权没有完成（{code}），没有保存额度账号"));
    }
    let access_token = json_str(&body, "access_token")
        .filter(|token| !token.contains(char::is_whitespace))
        .ok_or_else(|| NO_TOKEN.to_string())?
        .to_string();
    let refresh_token = json_str(&body, "refresh_token")
        .filter(|token| !token.contains(char::is_whitespace))
        .ok_or_else(|| NO_REFRESH.to_string())?
        .to_string();
    let id_token = json_str(&body, "id_token").map(str::to_string);
    let account_id = id_token
        .as_deref()
        .and_then(account_id_from_jwt)
        .or_else(|| account_id_from_jwt(&access_token));
    let email = id_token
        .as_deref()
        .and_then(email_from_jwt)
        .or_else(|| email_from_jwt(&access_token));
    if login.cancelled() {
        return Err(CANCELLED.to_string());
    }
    let identity = identity_for(&access_token, account_id.as_deref(), email.as_deref())?;
    if login.cancelled() {
        return Err(CANCELLED.to_string());
    }
    let expires_at = json_u64(&body, "expires_in").and_then(expires_at_from_seconds);
    upsert_account(
        paths,
        OfficialQuotaProvider::Codex,
        &identity,
        AccountSecret {
            access_token,
            refresh_token: Some(refresh_token),
            refresh_issuer: Some(login.shared.token_url.clone()),
            refresh_client_id: Some(OAUTH_CLIENT_ID.to_string()),
            account_id,
            access_expires_at: expires_at,
            ..AccountSecret::default()
        },
        QuotaAccountSource::Oauth,
        backoff_path,
    )
}

fn identity_for(
    access_token: &str,
    account_id: Option<&str>,
    email: Option<&str>,
) -> Result<QuotaIdentity, String> {
    let dir = std::env::temp_dir().join(format!("mabiao-codex-id-{:016x}", super::random_bits()));
    let outcome = (|| {
        let mut tokens = serde_json::Map::new();
        tokens.insert(
            "access_token".to_string(),
            Value::String(access_token.to_string()),
        );
        if let Some(account_id) = account_id.map(str::trim).filter(|id| !id.is_empty()) {
            tokens.insert(
                "account_id".to_string(),
                Value::String(account_id.to_string()),
            );
        }
        let bytes = serde_json::to_vec(&serde_json::json!({ "tokens": tokens }))
            .map_err(|error| error.to_string())?;
        super::write_atomic(&dir.join("auth.json"), &bytes, true)?;
        let mut identity = read_identity(OfficialQuotaProvider::Codex, &dir)
            .map_err(|_| IDENTITY_FAILED.to_string())?;
        if let Some(email) = email
            .map(str::trim)
            .filter(|email| !email.is_empty() && *email != access_token)
        {
            identity.label = email.to_string();
        }
        Ok(identity)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    outcome
}

fn stopped(shared: &Shared) -> bool {
    shared.cancelled.load(Ordering::SeqCst) || shared.stop.load(Ordering::SeqCst)
}

fn serve_callback(listener: TcpListener, shared: Arc<Shared>) {
    loop {
        if stopped(&shared) {
            break;
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                if stopped(&shared) {
                    break;
                }
                if handle_stream(&mut stream, &shared) {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

/// 收到真正的回调（对上或对不上）就停。其它请求继续听。
fn handle_stream(stream: &mut TcpStream, shared: &Shared) -> bool {
    let Some(head) = read_head(stream) else {
        return false;
    };
    let Some(target) = request_target(&head) else {
        respond(stream, 404, "没有这个地址");
        return false;
    };
    let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
        respond(stream, 400, BAD_URL);
        return false;
    };
    if url.path() != REDIRECT_PATH {
        respond(stream, 404, "没有这个地址");
        return false;
    }
    if let Some(error) = query_first(&url, "error") {
        let message = if error == "access_denied" {
            DENIED.to_string()
        } else {
            format!("Codex 授权没有完成（{error}），没有保存额度账号")
        };
        fail_waiting(shared, message.clone());
        respond(stream, 400, &message);
        return true;
    }
    let Some(code) = query_first(&url, "code") else {
        fail_waiting(shared, NO_CODE.to_string());
        respond(stream, 400, NO_CODE);
        return true;
    };
    let Some(state) = query_first(&url, "state") else {
        fail_waiting(shared, NO_STATE.to_string());
        respond(stream, 400, NO_STATE);
        return true;
    };
    if state != shared.state {
        fail_waiting(shared, STATE_MISMATCH.to_string());
        respond(stream, 400, STATE_MISMATCH);
        return true;
    }
    offer_code(shared, code);
    respond(stream, 200, "登录回调已收到，请回到码表。");
    true
}

fn offer_code(shared: &Shared, code: String) {
    let mut slot = shared
        .slot
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if matches!(*slot, Slot::Waiting) {
        *slot = Slot::Ready(code);
    }
}

fn fail_waiting(shared: &Shared, message: String) {
    let mut slot = shared
        .slot
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if matches!(*slot, Slot::Waiting | Slot::Ready(_)) {
        *slot = Slot::Finished(Err(message));
    }
}

fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if raw.len() > 8192 {
            return None;
        }
        match stream.read(&mut chunk) {
            Ok(0) => return None,
            Ok(read) => {
                raw.extend_from_slice(&chunk[..read]);
                if let Some(at) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                    return Some(String::from_utf8_lossy(&raw[..at]).into_owned());
                }
            }
            Err(_) => return None,
        }
    }
}

fn request_target(head: &str) -> Option<&str> {
    let mut parts = head.lines().next()?.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    if method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD") {
        Some(target)
    } else {
        None
    }
}

fn query_first(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn respond(stream: &mut TcpStream, status: u16, message: &str) {
    let message = message
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let body =
        format!("<!doctype html><meta charset=\"utf-8\"><title>码表</title><p>{message}</p>");
    let header = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn parse_callback(raw: &str) -> Result<(String, String), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(EMPTY_URL.to_string());
    }
    let url = Url::parse(trimmed).map_err(|_| BAD_URL.to_string())?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(BAD_URL.to_string());
    }
    let code = query_first(&url, "code").ok_or_else(|| NO_CODE.to_string())?;
    let state = query_first(&url, "state").ok_or_else(|| NO_STATE.to_string())?;
    Ok((code, state))
}

fn bind_loopback(port: u16) -> Result<(Option<TcpListener>, Option<u16>), String> {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => {
            let bound = listener
                .local_addr()
                .map_err(|error| format!("无法监听本地回调：{error}"))?
                .port();
            Ok((Some(listener), Some(bound)))
        }
        Err(_) if port != 0 => Ok((None, None)),
        Err(error) => Err(format!("无法监听本地回调：{error}")),
    }
}

fn redirect_uri_for(port: u16) -> String {
    if port == CODEX_PKCE_REDIRECT_PORT {
        format!("http://localhost:{port}{REDIRECT_PATH}")
    } else {
        format!("http://127.0.0.1:{port}{REDIRECT_PATH}")
    }
}

fn authorize_url(
    endpoints: &CodexPkceEndpoints,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
) -> Result<String, String> {
    let mut url = Url::parse(&endpoints.authorize_url).map_err(|_| "授权地址无效".to_string())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", OAUTH_CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("prompt", "login");
    Ok(url.to_string())
}

fn pkce_pair() -> Result<(String, String), String> {
    let verifier = random_token()?;
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    Ok((verifier, challenge))
}

fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| "无法生成登录校验参数".to_string())?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
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
    serde_json::from_str(text.trim()).map_err(|_| {
        if status == 200 {
            "Codex 授权响应无法识别".to_string()
        } else {
            format!("Codex 授权失败：HTTP {status}，没有保存额度账号")
        }
    })
}

fn read_response(response: ureq::Response) -> Result<String, String> {
    response
        .into_string()
        .map_err(|error| format!("读取 Codex 授权响应失败：{error}"))
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

fn expires_at_from_seconds(seconds: u64) -> Option<String> {
    chrono::Utc::now()
        .checked_add_signed(chrono::Duration::seconds(i64::try_from(seconds).ok()?))
        .map(|time| time.to_rfc3339())
}

fn account_id_from_jwt(token: &str) -> Option<String> {
    let claims = jwt_claims(token)?;
    string_claim(&claims, "chatgpt_account_id").or_else(|| {
        claims
            .get("https://api.openai.com/auth")
            .and_then(|auth| string_claim(auth, "chatgpt_account_id"))
    })
}

fn email_from_jwt(token: &str) -> Option<String> {
    string_claim(&jwt_claims(token)?, "email")
}

fn string_claim(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn jwt_claims(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let signature = parts.next()?;
    if parts.next().is_some() || payload.is_empty() || signature.is_empty() {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn ensure_openai_authorize(url: &str) -> Result<(), String> {
    let parsed = Url::parse(url).map_err(|_| "授权地址无效".to_string())?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("授权地址必须是 https://auth.openai.com".to_string());
    }
    if parsed.host_str() != Some("auth.openai.com") || parsed.path() != "/oauth/authorize" {
        return Err("授权地址必须是 https://auth.openai.com".to_string());
    }
    Ok(())
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
        return Err("授权地址无效".to_string());
    }
    let mut command = Command::new("cmd");
    command.arg("/C");
    command.raw_arg("start \"\"");
    command.raw_arg(format!("\"{url}\""));
    Ok(command)
}
