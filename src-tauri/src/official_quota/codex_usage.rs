//! Codex 官方额度的首选来源：`GET https://chatgpt.com/backend-api/wham/usage`。
//!
//! 比拉起 `codex app-server` 子进程好在不依赖 CLI 装没装、也不用等进程起来。
//! 凭证读 `~/.codex/auth.json`（`CODEX_HOME` 可覆盖）里 ChatGPT 登录的
//! `tokens.{access_token, account_id}`。
//!
//! 只对 ChatGPT 订阅有意义：纯 `OPENAI_API_KEY` 的账号按量计费，没有额度百分比，
//! 这里直接判定不可用，让调用方回落到 app-server。
//!
//! 和 Claude 一样只读不刷新——刷新会把新 token 写回第三方文件（ADR 0010）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::sanitize_percent;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const TIMEOUT: Duration = Duration::from_secs(12);
const SESSION_WINDOW_SECONDS: i64 = 5 * 60 * 60;
const WEEKLY_WINDOW_SECONDS: i64 = 7 * 24 * 60 * 60;
/// 响应体缺 `used_percent` 时的兜底：同一份数字也放在响应头里。
const PERCENT_HEADERS: [(&str, &str); 2] = [
    ("primary_window", "x-codex-primary-used-percent"),
    ("secondary_window", "x-codex-secondary-used-percent"),
];

pub fn fetch_usage() -> Result<(Vec<OfficialQuotaWindow>, String), String> {
    let auth = load_auth(&auth_path())?;
    let (raw, header_percents) = request_usage(&auth)?;
    let windows = parse_usage(&raw, &header_percents, Utc::now().timestamp())?;
    Ok((windows, Utc::now().to_rfc3339()))
}

pub fn auth_path() -> PathBuf {
    std::env::var("CODEX_HOME")
        .ok()
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| crate::ingest::default_home().join(".codex"))
        .join("auth.json")
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexAuth {
    pub access_token: String,
    pub account_id: Option<String>,
}

pub fn load_auth(path: &Path) -> Result<CodexAuth, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|_| "未找到 Codex 登录态，请先运行 codex 并登录".to_string())?;
    let value: Value =
        serde_json::from_str(&raw).map_err(|_| "Codex 登录态文件不是合法 JSON".to_string())?;
    let tokens = value.get("tokens").ok_or_else(|| {
        // 只有 API key 的账号按量计费，没有额度概念，说清楚而不是报个解析错误。
        "当前 Codex 用的是 API key，没有订阅额度".to_string()
    })?;
    let access_token = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "Codex 登录态里没有 access_token".to_string())?;
    Ok(CodexAuth {
        access_token: access_token.to_string(),
        account_id: tokens
            .get("account_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
    })
}

/// `rate_limit.{primary_window,secondary_window}`，每个窗口带 `used_percent`、
/// `limit_window_seconds`、`reset_at`（epoch 秒）或 `reset_after_seconds`。
///
/// 窗口种类按 `limit_window_seconds` 认，而不是按 primary/secondary 的位置——
/// Codex 会把临时只剩一条的周限额挪到 primary 槽里。
pub fn parse_usage(
    raw: &str,
    header_percents: &[(String, f64)],
    now_secs: i64,
) -> Result<Vec<OfficialQuotaWindow>, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|e| format!("Codex 用量 JSON 解析失败：{e}"))?;
    let rate_limit = value.get("rate_limit");

    let mut windows = Vec::new();
    for (slot, _) in PERCENT_HEADERS {
        let node = rate_limit.and_then(|node| node.get(slot));
        let percent = node
            .and_then(|node| node.get("used_percent"))
            .and_then(Value::as_f64)
            .or_else(|| {
                header_percents
                    .iter()
                    .find(|(name, _)| name == slot)
                    .map(|(_, percent)| *percent)
            })
            .and_then(sanitize_percent);
        let Some(percent) = percent else { continue };
        let seconds = node
            .and_then(|node| node.get("limit_window_seconds"))
            .and_then(Value::as_i64);
        windows.push(OfficialQuotaWindow {
            kind: kind_for(seconds, slot),
            label: label_for(seconds, slot),
            used_percent: Some(percent),
            resets_at: node.and_then(|node| resets_at(node, now_secs)),
            ..Default::default()
        });
    }

    if windows.is_empty() {
        return Err("Codex 用量响应里没有可用的已用百分比".to_string());
    }
    Ok(windows)
}

fn kind_for(seconds: Option<i64>, slot: &str) -> String {
    match seconds {
        Some(SESSION_WINDOW_SECONDS) => "session_5h".to_string(),
        Some(WEEKLY_WINDOW_SECONDS) => "weekly".to_string(),
        _ if slot == "primary_window" => "primary".to_string(),
        _ => "secondary".to_string(),
    }
}

fn label_for(seconds: Option<i64>, slot: &str) -> String {
    match seconds {
        Some(SESSION_WINDOW_SECONDS) => "5 小时".to_string(),
        Some(WEEKLY_WINDOW_SECONDS) => "7 天".to_string(),
        Some(value) if value > 0 => format!("{} 小时", value / 3600),
        _ if slot == "primary_window" => "主窗口".to_string(),
        _ => "次窗口".to_string(),
    }
}

fn resets_at(node: &Value, now_secs: i64) -> Option<String> {
    let epoch = node.get("reset_at").and_then(Value::as_i64).or_else(|| {
        node.get("reset_after_seconds")
            .and_then(Value::as_i64)
            .map(|after| now_secs + after)
    })?;
    chrono::DateTime::from_timestamp(epoch, 0).map(|value| value.to_rfc3339())
}

fn request_usage(auth: &CodexAuth) -> Result<(String, Vec<(String, f64)>), String> {
    let mut request = crate::net::agent_with_timeout(TIMEOUT)
        .get(USAGE_URL)
        .set("Authorization", &format!("Bearer {}", auth.access_token))
        .set("Accept", "application/json")
        .set("User-Agent", "ai-usage-stats");
    if let Some(account_id) = auth.account_id.as_deref() {
        request = request.set("ChatGPT-Account-Id", account_id);
    }
    match request.call() {
        Ok(response) => {
            let percents = PERCENT_HEADERS
                .iter()
                .filter_map(|(slot, header)| {
                    let value = response.header(header)?.trim().parse::<f64>().ok()?;
                    Some((slot.to_string(), value))
                })
                .collect();
            let body = response
                .into_string()
                .map_err(|e| format!("读取 Codex 用量响应失败：{e}"))?;
            Ok((body, percents))
        }
        Err(ureq::Error::Status(401 | 403, _)) => {
            Err("Codex 登录已失效，请重新运行 codex 登录".to_string())
        }
        Err(ureq::Error::Status(code, response)) => {
            let _ = response.into_string();
            Err(format!("拉取 Codex 用量失败：HTTP {code}"))
        }
        Err(_) => Err("无法连接 Codex 用量接口，请检查网络后重试".to_string()),
    }
}
