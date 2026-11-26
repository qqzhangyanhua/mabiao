//! 已分离的 Codex / Grok / Antigravity 在内存里续期。
//!
//! 新令牌只交给调用方。这里不写客户端目录，也不写钥匙串。

use std::time::Duration;

use serde_json::Value;

use super::AccountSecret;
use crate::domain::OfficialQuotaProvider;
use crate::official_quota::{antigravity, codex_usage, grok};

const TIMEOUT: Duration = Duration::from_secs(12);

pub(crate) struct RefreshRequest<'a> {
    pub provider: OfficialQuotaProvider,
    pub secret: &'a AccountSecret,
}

pub(crate) struct Refreshed {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

pub(crate) enum RefreshFail {
    /// 上游拒绝了这份 refresh token。调用方换成固定的「登录已失效」。
    Rejected,
    Other(String),
}

pub(crate) fn refresh_separated(request: &RefreshRequest<'_>) -> Result<Refreshed, RefreshFail> {
    let refresh_token = request
        .secret
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| RefreshFail::Other("没有可续期的 refresh token".to_string()))?;
    match request.provider {
        OfficialQuotaProvider::Codex => refresh_codex(refresh_token, request.secret),
        OfficialQuotaProvider::Grok => refresh_grok(refresh_token, request.secret),
        OfficialQuotaProvider::Antigravity => refresh_antigravity(refresh_token, request.secret),
        _ => Err(RefreshFail::Other(
            "这个提供方的隔离额度账号不能在这里续期".to_string(),
        )),
    }
}

fn refresh_codex(refresh_token: &str, secret: &AccountSecret) -> Result<Refreshed, RefreshFail> {
    let url = codex_token_url(secret.refresh_issuer.as_deref());
    let client_id = secret
        .refresh_client_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(codex_usage::OAUTH_CLIENT_ID);
    let body = post_form(
        &url,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
        ],
        "无法连接 Codex 登录服务，请检查网络后重试",
    )?;
    parse_token_body(&body)
}

fn refresh_grok(refresh_token: &str, secret: &AccountSecret) -> Result<Refreshed, RefreshFail> {
    let issuer = required(
        secret.refresh_issuer.as_deref(),
        "Grok 刷新缺少登录服务地址",
    )?;
    let client_id = required(
        secret.refresh_client_id.as_deref(),
        "Grok 刷新缺少客户端标识",
    )?;
    grok::refresh_tokens(&grok::RefreshCredentials {
        refresh_token: refresh_token.to_string(),
        oidc_issuer: issuer,
        client_id,
    })
    .map(|tokens| Refreshed {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
    })
    .map_err(classify_provider_error)
}

fn refresh_antigravity(
    refresh_token: &str,
    secret: &AccountSecret,
) -> Result<Refreshed, RefreshFail> {
    if let Some(issuer) = nonempty(secret.refresh_issuer.as_deref()) {
        let client_id = required(
            secret.refresh_client_id.as_deref(),
            "刷新 Antigravity 登录缺少客户端信息",
        )?;
        let client_secret = required(
            secret.refresh_client_secret.as_deref(),
            "刷新 Antigravity 登录缺少客户端信息",
        )?;
        let url = antigravity_token_url(&issuer);
        return antigravity::exchange_refresh_token(
            &url,
            refresh_token,
            &[(client_id, client_secret)],
        )
        .map(|(access_token, refresh_token)| Refreshed {
            access_token,
            refresh_token,
        })
        .map_err(classify_provider_error);
    }
    let clients = antigravity::local_oauth_clients().map_err(RefreshFail::Other)?;
    antigravity::exchange_refresh_token(antigravity::TOKEN_URL, refresh_token, &clients)
        .map(|(access_token, refresh_token)| Refreshed {
            access_token,
            refresh_token,
        })
        .map_err(classify_provider_error)
}

fn codex_token_url(issuer: Option<&str>) -> String {
    let Some(issuer) = nonempty(issuer) else {
        return codex_usage::OAUTH_TOKEN_URL.to_string();
    };
    if issuer.contains("/oauth/token") {
        return issuer;
    }
    format!("{}/oauth/token", issuer.trim_end_matches('/'))
}

fn antigravity_token_url(issuer: &str) -> String {
    let issuer = issuer.trim_end_matches('/');
    if issuer.ends_with("/token") {
        issuer.to_string()
    } else {
        format!("{issuer}/token")
    }
}

fn post_form(url: &str, fields: &[(&str, &str)], offline: &str) -> Result<String, RefreshFail> {
    match crate::net::agent_for_url(url, TIMEOUT)
        .post(url)
        .set("Accept", "application/json")
        .send_form(fields)
    {
        Ok(response) => response
            .into_string()
            .map_err(|error| RefreshFail::Other(format!("读取刷新响应失败：{error}"))),
        Err(ureq::Error::Status(400 | 401 | 403, _)) => Err(RefreshFail::Rejected),
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            if body.contains("invalid_grant") {
                Err(RefreshFail::Rejected)
            } else {
                Err(RefreshFail::Other(format!("刷新登录失败：HTTP {code}")))
            }
        }
        Err(_) => Err(RefreshFail::Other(offline.to_string())),
    }
}

fn parse_token_body(body: &str) -> Result<Refreshed, RefreshFail> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| RefreshFail::Other(format!("刷新响应不是 JSON：{error}")))?;
    if value
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| error == "invalid_grant")
    {
        return Err(RefreshFail::Rejected);
    }
    let access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| RefreshFail::Other("刷新响应里没有 access_token".to_string()))?
        .to_string();
    let refresh_token = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    Ok(Refreshed {
        access_token,
        refresh_token,
    })
}

fn classify_provider_error(message: String) -> RefreshFail {
    if message.contains("找不到本机") {
        return RefreshFail::Other(message);
    }
    if message.contains("invalid_grant")
        || message.contains("登录已过期")
        || message.contains("登录已失效")
        || message.contains("HTTP 400")
        || message.contains("HTTP 401")
        || message.contains("HTTP 403")
    {
        return RefreshFail::Rejected;
    }
    RefreshFail::Other(message)
}

fn required(value: Option<&str>, missing: &str) -> Result<String, RefreshFail> {
    nonempty(value).ok_or_else(|| RefreshFail::Other(missing.to_string()))
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}
