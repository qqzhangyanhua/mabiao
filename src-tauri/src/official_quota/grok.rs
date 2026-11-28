use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::{
    display_plan_label, parse_resets_at, sanitize_percent, HttpRequest, QuotaEnv, QuotaPaths,
    QuotaSnapshot, QuotaTransport,
};

const BILLING_CREDITS_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const BILLING_MONTHLY_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing";
const USER_URL: &str = "https://cli-chat-proxy.grok.com/v1/user";
const SETTINGS_URL: &str = "https://cli-chat-proxy.grok.com/v1/settings";
const TIMEOUT: Duration = Duration::from_secs(12);
const SETTINGS_TIMEOUT: Duration = Duration::from_secs(5);
const LEGACY_SCOPE: &str = "https://accounts.x.ai/sign-in";
const SUPERGROK_SCOPE_PREFIX: &str = "https://auth.x.ai";
const API_KEY_SCOPE: &str = "xai::api_key";
const TOKEN_AUTH: &str = "xai-grok-cli";
const CLIENT_MODE: &str = "interactive";
/// grok-build 当前 CARGO_PKG_VERSION；本机 `~/.grok/.metadata_version` 优先。
const DEFAULT_GROK_CLI_VERSION: &str = "1.0.5";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokSession {
    pub token: String,
    pub user_id: Option<String>,
    /// 本地按 `expires_at` 判定是否已过期——过期了也保留会话，交给
    /// `fetch_rate_limits` 先现刷再用，不再直接报错让用户手动 `grok login`。
    pub expired: bool,
    pub refresh: Option<RefreshCredentials>,
    /// `expires_at` 的 RFC3339。没有这个字段时是 `None`，不当成已过期。
    pub expires_at: Option<String>,
}

/// OIDC 静默刷新要用的三件套，均来自 `auth.json` 同一条目，不落盘、只在内存里现刷现用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshCredentials {
    pub refresh_token: String,
    pub oidc_issuer: String,
    pub client_id: String,
}

pub fn fetch_rate_limits() -> Result<QuotaSnapshot, String> {
    fetch_rate_limits_in(&QuotaEnv::live(QuotaPaths::app_data()))
}

pub fn fetch_rate_limits_in(env: &QuotaEnv) -> Result<QuotaSnapshot, String> {
    let session = load_session_at(&env.grok_auth_path(), env.now())?;
    let version = grok_client_version_at(&env.grok_home());
    let mut token = ensure_fresh_token_in(env, &session)?;
    let mut user_id = resolve_user_id_in(env, &session, &token, &version)?;
    let credits_url = env.rebase_url(BILLING_CREDITS_URL);
    let mut credits_result = request_json_via(
        env.transport.as_ref(),
        &token,
        Some(&user_id),
        &credits_url,
        TIMEOUT,
        &version,
    );
    // access token 比本地记的 expires_at 更早失效（时钟偏差、服务端提前吊销）时，
    // 现刷一次再重试一遍，不要直接把「已过期」甩给用户。
    if let Err(error) = &credits_result {
        if is_expired_error(error) {
            if let Some(refresh) = &session.refresh {
                token = refresh_tokens_via(env.transport.as_ref(), refresh)?.access_token;
                user_id = resolve_user_id_in(env, &session, &token, &version)?;
                credits_result = request_json_via(
                    env.transport.as_ref(),
                    &token,
                    Some(&user_id),
                    &credits_url,
                    TIMEOUT,
                    &version,
                );
            }
        }
    }
    let mut windows = match credits_result {
        Ok(raw) => parse_credits(&raw)?,
        Err(error)
            if env.quota_base.is_none() && super::grok_grpc::should_fallback_to_grpc(&error) =>
        {
            super::grok_grpc::fetch_credits_in(env, &token, Some(&user_id), &version)?
        }
        Err(error) => return Err(error),
    };
    if let Ok(monthly_raw) = request_json_via(
        env.transport.as_ref(),
        &token,
        Some(&user_id),
        &env.rebase_url(BILLING_MONTHLY_URL),
        TIMEOUT,
        &version,
    ) {
        if let Ok(monthly) = parse_monthly(&monthly_raw) {
            merge_windows(&mut windows, monthly);
        }
    }
    if windows.is_empty() {
        return Err("Grok 限额响应里没有可用的已用百分比".to_string());
    }
    let plan = request_json_via(
        env.transport.as_ref(),
        &token,
        Some(&user_id),
        &env.rebase_url(SETTINGS_URL),
        SETTINGS_TIMEOUT,
        &version,
    )
    .ok()
    .and_then(|raw| parse_settings_plan(&raw))
    .or_else(|| parse_jwt_plan(&token));
    Ok(QuotaSnapshot::new(windows, env.now().to_rfc3339()).with_plan(plan))
}

/// 只用现成的 access token 取额度。不发现 OIDC，也不拿 refresh token 去换新的。
pub(crate) fn fetch_with_access_token_in(
    env: &QuotaEnv,
    token: &str,
    user_id: Option<&str>,
) -> Result<QuotaSnapshot, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("Grok 登录已过期，请重新运行 grok login".to_string());
    }
    let version = DEFAULT_GROK_CLI_VERSION;
    let transport = env.transport.as_ref();
    let user_id_owned = if let Some(id) = user_id.map(str::trim).filter(|id| !id.is_empty()) {
        id.to_string()
    } else {
        let raw = request_json_via(
            transport,
            token,
            None,
            &env.rebase_url(USER_URL),
            TIMEOUT,
            version,
        )?;
        parse_user_id_response(&raw)?
    };
    let credits_result = request_json_via(
        transport,
        token,
        Some(&user_id_owned),
        &env.rebase_url(BILLING_CREDITS_URL),
        TIMEOUT,
        version,
    );
    let mut windows = match credits_result {
        Ok(raw) => parse_credits(&raw)?,
        Err(error)
            if env.quota_base.is_none() && super::grok_grpc::should_fallback_to_grpc(&error) =>
        {
            super::grok_grpc::fetch_credits_in(env, token, Some(&user_id_owned), version)?
        }
        Err(error) => return Err(error),
    };
    if let Ok(monthly_raw) = request_json_via(
        transport,
        token,
        Some(&user_id_owned),
        &env.rebase_url(BILLING_MONTHLY_URL),
        TIMEOUT,
        version,
    ) {
        if let Ok(monthly) = parse_monthly(&monthly_raw) {
            merge_windows(&mut windows, monthly);
        }
    }
    if windows.is_empty() {
        return Err("Grok 限额响应里没有可用的已用百分比".to_string());
    }
    let plan = request_json_via(
        transport,
        token,
        Some(&user_id_owned),
        &env.rebase_url(SETTINGS_URL),
        SETTINGS_TIMEOUT,
        version,
    )
    .ok()
    .and_then(|raw| parse_settings_plan(&raw))
    .or_else(|| parse_jwt_plan(token));
    Ok(QuotaSnapshot::new(windows, env.now().to_rfc3339()).with_plan(plan))
}

/// 本地已知过期时提前现刷，省一次注定 401 的往返；没有刷新凭证才把过期错误原样交回。
fn ensure_fresh_token_in(env: &QuotaEnv, session: &GrokSession) -> Result<String, String> {
    if !session.expired {
        return Ok(session.token.clone());
    }
    let refresh = session
        .refresh
        .as_ref()
        .ok_or_else(|| "Grok 登录已过期，请重新运行 grok login".to_string())?;
    refresh_tokens_via(env.transport.as_ref(), refresh).map(|tokens| tokens.access_token)
}

fn is_expired_error(error: &str) -> bool {
    error == "Grok 登录已过期，请重新运行 grok login"
}

/// 内存里换一套令牌。响应里若有轮换后的 refresh token 也带回来，由调用方决定写哪。
pub(crate) struct RefreshedOidc {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

pub(crate) fn refresh_tokens_via(
    transport: &dyn QuotaTransport,
    refresh: &RefreshCredentials,
) -> Result<RefreshedOidc, String> {
    let token_endpoint = discover_token_endpoint_via(transport, &refresh.oidc_issuer)?;
    let request = HttpRequest::post_form(
        &token_endpoint,
        vec![
            ("grant_type".into(), "refresh_token".into()),
            ("client_id".into(), refresh.client_id.clone()),
            ("refresh_token".into(), refresh.refresh_token.clone()),
        ],
        TIMEOUT,
    )
    .header("Accept", "application/json");
    let response = transport
        .http(&request)
        .map_err(|_| "无法连接 Grok 登录服务，请检查网络后重试".to_string())?;
    match response.status {
        200..=299 => {
            let body = response
                .body_string()
                .map_err(|e| format!("读取 Grok 刷新响应失败：{e}"))?;
            parse_refreshed_tokens(&body)
        }
        401 | 403 => Err("Grok 登录已过期，请重新运行 grok login".to_string()),
        code => {
            let body = response.body_string_lossy();
            Err(format!(
                "刷新 Grok 登录失败：{}",
                status_detail(code, &body)
            ))
        }
    }
}

/// OIDC 发现端点动态给出 token_endpoint，不硬编码——发行方轮换网关时能跟上。
fn discover_token_endpoint_via(
    transport: &dyn QuotaTransport,
    issuer: &str,
) -> Result<String, String> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let response = transport
        .http(&HttpRequest::get(&url, TIMEOUT))
        .map_err(|_| "无法连接 Grok 登录服务，请检查网络后重试".to_string())?;
    if !response.is_success() {
        return Err("无法连接 Grok 登录服务，请检查网络后重试".to_string());
    }
    let raw = response
        .body_string()
        .map_err(|e| format!("读取 Grok 登录配置失败：{e}"))?;
    let value = parse_object(&raw, "Grok 登录配置")?;
    string_field(&value, "token_endpoint")
        .ok_or_else(|| "Grok 登录配置里没有 token_endpoint".to_string())
}

pub fn parse_refreshed_access_token(raw: &str) -> Result<String, String> {
    parse_refreshed_tokens(raw).map(|tokens| tokens.access_token)
}

fn parse_refreshed_tokens(raw: &str) -> Result<RefreshedOidc, String> {
    let value = parse_object(raw, "Grok 刷新响应")?;
    let access_token = string_field(&value, "access_token")
        .ok_or_else(|| "Grok 刷新响应里没有 access_token".to_string())?;
    Ok(RefreshedOidc {
        access_token,
        refresh_token: string_field(&value, "refresh_token"),
    })
}

/// 解析 `GET /v1/billing?format=credits`：周额度池 + Grok Build 分项 + 按需。
pub fn parse_credits(raw: &str) -> Result<Vec<OfficialQuotaWindow>, String> {
    let value = parse_object(raw, "Grok 周额度")?;
    let config = value.get("config").unwrap_or(&value);
    let weekly_resets = period_end(config);
    let weekly_period = is_weekly_period(config);
    let mut windows = Vec::new();

    let weekly_percent = named_percent(config, "creditUsagePercent");
    let build_percent = product_percent(config, "GrokBuild");
    if let Some(percent) = weekly_percent {
        push_window(
            &mut windows,
            Some(percent),
            "weekly",
            "周额度",
            weekly_resets.clone(),
        );
        push_window(
            &mut windows,
            build_percent,
            "product_grokbuild",
            "Grok Build",
            weekly_resets.clone(),
        );
    } else if let Some(percent) = build_percent {
        push_window(
            &mut windows,
            Some(percent),
            "weekly",
            "周额度",
            weekly_resets.clone(),
        );
    } else if weekly_period {
        push_window(
            &mut windows,
            Some(0.0),
            "weekly",
            "周额度",
            weekly_resets.clone(),
        );
    }

    if let Some(window) = parse_on_demand(config, weekly_resets) {
        windows.push(window);
    }
    Ok(windows)
}

/// 解析 `GET /v1/billing`：月度 included 额度。缺 used 不当成 0%。
pub fn parse_monthly(raw: &str) -> Result<Vec<OfficialQuotaWindow>, String> {
    let value = parse_object(raw, "Grok 月额度")?;
    let config = value.get("config").unwrap_or(&value);
    let used = money_val(config.get("used"))
        .or_else(|| money_val(value.pointer("/usage/totalUsed")))
        .or_else(|| money_val(value.pointer("/usage/includedUsed")));
    let limit =
        money_val(config.get("monthlyLimit")).or_else(|| money_val(value.get("monthlyLimit")));
    let Some((used, limit)) = used.zip(limit) else {
        return Ok(Vec::new());
    };
    if limit <= 0.0 {
        return Ok(Vec::new());
    }
    let percent = sanitize_percent((used / limit * 100.0).clamp(0.0, 100.0));
    let resets_at = period_end(config).or_else(|| period_end(&value));
    let mut windows = Vec::new();
    push_window(&mut windows, percent, "monthly", "月额度", resets_at);
    Ok(windows)
}

pub fn parse_auth_json(raw: &str, now: DateTime<Utc>) -> Result<GrokSession, String> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|_| "Grok 登录凭证无效，请重新运行 grok login".to_string())?;
    let Some(map) = value.as_object() else {
        return Err("Grok 登录凭证无效，请重新运行 grok login".to_string());
    };

    let mut expired_only = false;
    let mut saw_api_key_only = true;
    let mut saw_any = false;
    let mut preferred = None;
    let mut legacy = None;
    let mut other = None;
    // 过期但带 refresh_token 的会话：找不到更新的可用会话时兜底，交给
    // `fetch_rate_limits` 现刷，而不是直接要用户手动 `grok login`。
    let mut preferred_refreshable = None;
    let mut legacy_refreshable = None;
    let mut other_refreshable = None;

    for (scope, node) in map {
        if !node.is_object() {
            continue;
        }
        if is_blocked_mode(node) {
            continue;
        }
        let Some(token) = token_of(node) else {
            continue;
        };
        saw_any = true;
        if is_expired(node, now) {
            expired_only = true;
            // API key 条目没有会话刷新的概念，仍旧原样跳过。
            if !is_api_key_entry(scope, node) {
                if let Some(refresh) = refresh_of(node) {
                    let session = GrokSession {
                        token,
                        user_id: user_id_of(node),
                        expired: true,
                        refresh: Some(refresh),
                        expires_at: expires_at_of(node),
                    };
                    if scope.starts_with(SUPERGROK_SCOPE_PREFIX) {
                        preferred_refreshable.get_or_insert(session);
                    } else if scope == LEGACY_SCOPE {
                        legacy_refreshable.get_or_insert(session);
                    } else {
                        other_refreshable.get_or_insert(session);
                    }
                }
            }
            continue;
        }
        if is_api_key_entry(scope, node) {
            continue;
        }
        saw_api_key_only = false;
        let session = GrokSession {
            token,
            user_id: user_id_of(node),
            expired: false,
            refresh: refresh_of(node),
            expires_at: expires_at_of(node),
        };
        if scope.starts_with(SUPERGROK_SCOPE_PREFIX) {
            preferred = Some(session);
            break;
        }
        if scope == LEGACY_SCOPE {
            legacy = Some(session);
        } else {
            other = Some(session);
        }
    }

    if let Some(session) = preferred.or(legacy).or(other) {
        return Ok(session);
    }
    if let Some(session) = preferred_refreshable
        .or(legacy_refreshable)
        .or(other_refreshable)
    {
        return Ok(session);
    }
    if saw_any && saw_api_key_only && !expired_only {
        return Err(
            "Grok 官方额度需要 grok login 的会话登录，API key 无法查询订阅限额".to_string(),
        );
    }
    if expired_only {
        return Err("Grok 登录已过期，请重新运行 grok login".to_string());
    }
    Err("Grok 登录凭证无效，请重新运行 grok login".to_string())
}

/// 从会话条目里取 OIDC 静默刷新三件套；缺任一项就当作不可自动刷新。
fn expires_at_of(node: &Value) -> Option<String> {
    crate::official_quota::parse_resets_at(node.get("expires_at")?)
}

fn refresh_of(node: &Value) -> Option<RefreshCredentials> {
    Some(RefreshCredentials {
        refresh_token: string_field(node, "refresh_token")?,
        oidc_issuer: string_field(node, "oidc_issuer")?,
        client_id: string_field(node, "oidc_client_id")?,
    })
}

pub fn parse_user_id_response(raw: &str) -> Result<String, String> {
    let value = parse_object(raw, "Grok 用户信息")?;
    user_id_of(&value)
        .or_else(|| string_field(&value, "id"))
        .ok_or_else(|| "Grok 用户信息里没有 userId".to_string())
}

pub fn parse_settings_plan(raw: &str) -> Option<String> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let node = value.get("config").unwrap_or(&value);
    [
        "subscription_tier_display",
        "subscriptionTierDisplay",
        "subscription_tier",
        "subscriptionTier",
    ]
    .into_iter()
    .find_map(|key| string_field(node, key).or_else(|| string_field(&value, key)))
    .and_then(|value| display_plan_label(&value))
}

/// JWT `tier` 声明兜底。官方自己说它可能滞后，只在 settings 没给展示名时用。
pub fn parse_jwt_plan(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1).filter(|part| !part.is_empty())?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    if let Some(n) = claims.get("tier").and_then(Value::as_u64) {
        let raw = match n {
            0 => "free",
            1 => "supergrok",
            2 => "x_basic",
            3 => "x_premium",
            4 => "x_premium_plus",
            5 => "supergrok_heavy",
            6 => "supergrok_lite",
            7 => "supergrok_plus",
            _ => return None,
        };
        return display_plan_label(raw);
    }
    claims
        .get("tier")
        .and_then(Value::as_str)
        .and_then(display_plan_label)
}

/// 从显式的 `auth.json` 或配置目录读会话。不访问网络，也不碰默认的 `GROK_HOME`。
pub fn load_session_at(path: &Path, now: DateTime<Utc>) -> Result<GrokSession, String> {
    let file = if path.is_dir() {
        path.join("auth.json")
    } else {
        path.to_path_buf()
    };
    if !file.exists() {
        return Err("尚未登录 Grok CLI，请先运行 grok login".to_string());
    }
    let raw = std::fs::read_to_string(&file).map_err(|e| format!("读取 Grok 登录凭证失败：{e}"))?;
    parse_auth_json(&raw, now)
}

fn resolve_user_id_in(
    env: &QuotaEnv,
    session: &GrokSession,
    token: &str,
    version: &str,
) -> Result<String, String> {
    if let Some(user_id) = session
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        return Ok(user_id.to_string());
    }
    let raw = request_json_via(
        env.transport.as_ref(),
        token,
        None,
        &env.rebase_url(USER_URL),
        TIMEOUT,
        version,
    )?;
    parse_user_id_response(&raw)
}

/// 探针：只看文件在不在，不解析、不判过期——过期的账号仍该显示出来提示重登。
pub fn auth_file_exists() -> bool {
    auth_path().exists()
}

pub(crate) fn auth_path() -> PathBuf {
    auth_path_in(&crate::scan_paths::ScanEnv::capture_for_home(
        &crate::ingest::default_home(),
    ))
}

pub(crate) fn auth_path_in(env: &crate::scan_paths::ScanEnv) -> PathBuf {
    grok_home_in(env).join("auth.json")
}

pub(crate) fn grok_home_in(env: &crate::scan_paths::ScanEnv) -> PathBuf {
    env.client_credential_root(crate::domain::Source::Grok, "auth.json")
}

fn request_json_via(
    transport: &dyn QuotaTransport,
    token: &str,
    user_id: Option<&str>,
    url: &str,
    timeout: Duration,
    version: &str,
) -> Result<String, String> {
    let mut request = HttpRequest::get(url, timeout)
        .header("Authorization", format!("Bearer {token}"))
        .header("X-XAI-Token-Auth", TOKEN_AUTH)
        .header("x-grok-client-version", version)
        .header("x-grok-client-mode", CLIENT_MODE)
        .header("Accept", "application/json");
    if let Some(user_id) = user_id.filter(|id| !id.is_empty()) {
        request = request.header("x-userid", user_id);
    }
    let response = transport
        .http(&request)
        .map_err(|_| "无法连接 Grok 限额接口，请检查网络后重试".to_string())?;
    match response.status {
        200..=299 => response
            .body_string()
            .map_err(|e| format!("读取 Grok 限额响应失败：{e}")),
        401 | 403 => Err("Grok 登录已过期，请重新运行 grok login".to_string()),
        code => {
            let body = response.body_string_lossy();
            Err(format!(
                "拉取 Grok 限额失败：{}",
                status_detail(code, &body)
            ))
        }
    }
}

fn grok_client_version_at(home: &Path) -> String {
    std::fs::read_to_string(home.join(".metadata_version"))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| DEFAULT_GROK_CLI_VERSION.to_string())
}

fn status_detail(code: u16, body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| format!("HTTP {code}"))
}

fn parse_object(raw: &str, label: &str) -> Result<Value, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|e| format!("{label} JSON 解析失败：{e}"))?;
    if !value.is_object() {
        return Err(format!("{label} JSON 不是对象"));
    }
    Ok(value)
}

fn named_percent(node: &Value, field: &str) -> Option<f64> {
    node.get(field)
        .and_then(Value::as_f64)
        .or_else(|| node.get(field).and_then(Value::as_i64).map(|n| n as f64))
        .and_then(sanitize_percent)
}

fn product_percent(config: &Value, product: &str) -> Option<f64> {
    let products = config.get("productUsage")?.as_array()?;
    products.iter().find_map(|item| {
        let name = item.get("product").and_then(Value::as_str)?;
        if !name.eq_ignore_ascii_case(product) {
            return None;
        }
        named_percent(item, "usagePercent")
    })
}

fn parse_on_demand(config: &Value, resets_at: Option<String>) -> Option<OfficialQuotaWindow> {
    let used = money_val(config.get("onDemandUsed"))?;
    let cap = money_val(config.get("onDemandCap"))?;
    if cap <= 0.0 {
        return None;
    }
    let percent = sanitize_percent((used / cap * 100.0).clamp(0.0, 100.0))?;
    Some(OfficialQuotaWindow {
        kind: "on_demand".to_string(),
        label: "按需".to_string(),
        used_percent: Some(percent),
        resets_at,
        ..Default::default()
    })
}

fn money_val(node: Option<&Value>) -> Option<f64> {
    let node = node?;
    if let Some(n) = node.as_f64() {
        return Some(n);
    }
    if let Some(n) = node.as_i64() {
        return Some(n as f64);
    }
    node.get("val")
        .and_then(|val| val.as_f64().or_else(|| val.as_i64().map(|n| n as f64)))
}

fn period_end(node: &Value) -> Option<String> {
    node.pointer("/currentPeriod/end")
        .or_else(|| node.get("billingPeriodEnd"))
        .or_else(|| node.pointer("/billingCycle/billingPeriodEnd"))
        .and_then(parse_resets_at)
}

fn is_weekly_period(config: &Value) -> bool {
    let period = match config.get("currentPeriod") {
        Some(period) if period.is_object() => period,
        _ => return false,
    };
    let kind = period.get("type").and_then(Value::as_str).unwrap_or("");
    kind == "USAGE_PERIOD_TYPE_WEEKLY" && period.get("end").and_then(parse_resets_at).is_some()
}

fn push_window(
    windows: &mut Vec<OfficialQuotaWindow>,
    percent: Option<f64>,
    kind: &str,
    label: &str,
    resets_at: Option<String>,
) {
    let Some(percent) = percent else {
        return;
    };
    windows.push(OfficialQuotaWindow {
        kind: kind.to_string(),
        label: label.to_string(),
        used_percent: Some(percent),
        resets_at,
        ..Default::default()
    });
}

fn merge_windows(into: &mut Vec<OfficialQuotaWindow>, extra: Vec<OfficialQuotaWindow>) {
    for window in extra {
        if into.iter().any(|existing| existing.kind == window.kind) {
            continue;
        }
        into.push(window);
    }
}

fn token_of(node: &Value) -> Option<String> {
    string_field(node, "key").or_else(|| string_field(node, "access_token"))
}

fn user_id_of(node: &Value) -> Option<String> {
    string_field(node, "user_id")
        .or_else(|| string_field(node, "userId"))
        .or_else(|| string_field(node, "userid"))
}

fn string_field(node: &Value, field: &str) -> Option<String> {
    node.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn is_blocked_mode(node: &Value) -> bool {
    matches!(
        node.get("auth_mode").and_then(Value::as_str),
        Some("web_login" | "grok")
    )
}

fn is_api_key_entry(scope: &str, node: &Value) -> bool {
    scope == API_KEY_SCOPE || node.get("auth_mode").and_then(Value::as_str) == Some("api_key")
}

fn is_expired(node: &Value, now: DateTime<Utc>) -> bool {
    let Some(raw) = node.get("expires_at") else {
        return false;
    };
    let Some(expires) = parse_resets_at(raw)
        .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
        .map(|dt| dt.with_timezone(&Utc))
    else {
        return false;
    };
    now >= expires
}
