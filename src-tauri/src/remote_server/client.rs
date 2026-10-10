//! 对远程服务的 HTTP 调用。联网只在 Rust 侧发生，webview 从不直接请求远程服务。
//!
//! 所有错误在这里翻成中文人话：用户看到的是「账号或密码不对」「连不上」，不是裸状态码。

use std::time::Duration;

use super::address;
use push_protocol::{
    ApiError, ApiErrorCode, LoginRequest, LoginResponse, PushSessionRequest, PushSessionResponse,
    PushUsageRequest, PushUsageResponse,
};

const TIMEOUT: Duration = Duration::from_secs(15);
/// 一场长会话的正文可以有几十 MB，服务端要整场写库，15 秒不够。
const PUSH_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, PartialEq, Eq)]
pub enum RemoteError {
    /// 服务端不认这个 token（过期、被吊销、账号被停用）。调用方据此把本机登录态标成需要重登。
    TokenRejected,
    Message(String),
}

impl RemoteError {
    fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }

    pub fn into_message(self) -> String {
        match self {
            Self::TokenRejected => TOKEN_REJECTED.to_string(),
            Self::Message(text) => text,
        }
    }
}

pub const TOKEN_REJECTED: &str = "登录已失效，请重新登录";

fn agent(base_url: &str) -> ureq::Agent {
    crate::net::agent_without_redirects(TIMEOUT, address::is_loopback_address(base_url))
}

fn push_agent(base_url: &str) -> ureq::Agent {
    crate::net::agent_without_redirects(PUSH_TIMEOUT, address::is_loopback_address(base_url))
}

/// 推一场会话（一个请求）。
pub fn push_session(
    base_url: &str,
    token: &str,
    request: &PushSessionRequest,
) -> Result<PushSessionResponse, RemoteError> {
    post_json(base_url, token, "/api/v1/push/session", request)
}

/// 推一批消耗记录。
pub fn push_usage(
    base_url: &str,
    token: &str,
    request: &PushUsageRequest,
) -> Result<PushUsageResponse, RemoteError> {
    post_json(base_url, token, "/api/v1/push/usage", request)
}

fn post_json<B: serde::Serialize, R: serde::de::DeserializeOwned>(
    base_url: &str,
    token: &str,
    path: &str,
    body: &B,
) -> Result<R, RemoteError> {
    let response = push_agent(base_url)
        .post(&format!("{base_url}{path}"))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(body);
    settle(response)?.into_json::<R>().map_err(|_| {
        RemoteError::message("这个地址返回的不是码表远程服务的响应，请检查地址是否填对")
    })
}

/// 用密码换 token。密码只存在于这一次请求里。
pub fn login(base_url: &str, account: &str, password: &str) -> Result<LoginResponse, RemoteError> {
    let request = LoginRequest::new(account, password);
    let response = agent(base_url)
        .post(&format!("{base_url}/api/v1/login"))
        .send_json(&request);
    let response = settle(response)?;
    response.into_json::<LoginResponse>().map_err(|_| {
        RemoteError::message("这个地址返回的不是码表远程服务的响应，请检查地址是否填对")
    })
}

/// 用 token 访问一个轻量接口，确认服务端还认这次登录。
pub fn check_token(base_url: &str, token: &str) -> Result<(), RemoteError> {
    let response = agent(base_url)
        .get(&format!("{base_url}/api/v1/me"))
        .set("Authorization", &format!("Bearer {token}"))
        .call();
    settle(response).map(|_| ())
}

fn settle(result: Result<ureq::Response, ureq::Error>) -> Result<ureq::Response, RemoteError> {
    match result {
        Ok(response) if response.status() >= 300 => Err(status_error(response)),
        Ok(response) => Ok(response),
        Err(ureq::Error::Status(_, response)) => Err(status_error(response)),
        Err(ureq::Error::Transport(transport)) => Err(transport_error(&transport)),
    }
}

fn status_error(response: ureq::Response) -> RemoteError {
    let status = response.status();
    if (300..400).contains(&status) {
        return RemoteError::message(
            "这个地址发生了跳转，请直接填写最终地址（例如把 http:// 改成 https://）",
        );
    }
    match response.into_json::<ApiError>() {
        Ok(error) => api_error(error),
        Err(_) => unrecognized_status(status),
    }
}

fn api_error(error: ApiError) -> RemoteError {
    match error.code {
        ApiErrorCode::TokenExpired => RemoteError::TokenRejected,
        ApiErrorCode::InvalidCredentials => {
            RemoteError::message("账号或密码不对，或者这个账号已被管理员停用")
        }
        ApiErrorCode::UnsupportedProtocolVersion => {
            RemoteError::message("码表与远程服务的版本对不上，请把两边都升级到最新版")
        }
        ApiErrorCode::Forbidden => RemoteError::message("这个账号没有权限执行这个操作"),
        ApiErrorCode::Internal => RemoteError::message("远程服务出错了，请稍后再试或联系管理员"),
        ApiErrorCode::InvalidPayload | ApiErrorCode::NotFound | ApiErrorCode::Conflict => {
            RemoteError::message(format!("远程服务拒绝了请求：{}", error.message))
        }
    }
}

/// 响应体不是 `ApiError`：多半不是码表服务端，而是网关、代理或别的站点。
fn unrecognized_status(status: u16) -> RemoteError {
    RemoteError::message(match status {
        404 => "这个地址上没有找到码表远程服务（404），请检查地址是否填对".to_string(),
        500..=599 => format!("远程服务暂时不可用（HTTP {status}），请稍后再试"),
        _ => format!("远程服务返回了意外的响应（HTTP {status}），请检查地址是否填对"),
    })
}

fn transport_error(transport: &ureq::Transport) -> RemoteError {
    use ureq::ErrorKind;
    let detail = transport.to_string();
    let lowered = detail.to_lowercase();
    RemoteError::message(match transport.kind() {
        ErrorKind::Dns => "找不到这个地址，请检查拼写与网络".to_string(),
        ErrorKind::ConnectionFailed => "连不上远程服务，请检查地址、端口与网络".to_string(),
        _ if lowered.contains("certificate") || lowered.contains("invalid peer") => {
            "远程服务的 https 证书不可信（自签名证书需要先装进系统信任）".to_string()
        }
        _ if lowered.contains("timed out") || lowered.contains("timeout") => {
            "连接远程服务超时，请检查网络或稍后再试".to_string()
        }
        _ => format!("请求远程服务失败：{detail}"),
    })
}
