//! Claude 官方额度的首选来源：直接问 `GET /api/oauth/usage`，凭证用 Claude Code
//! 自己存的登录态。
//!
//! 这条比 statusline hook 好在零配置——不用改用户的 `settings.json`，也不用等他
//! 打开一次 Claude Code。读不到就由调用方回落到 statusline 捕获文件。
//!
//! 凭证在 `~/.claude/.credentials.json` 的 `claudeAiOauth`：`accessToken` +
//! `expiresAt`（毫秒）。macOS 上 Claude Code 以钥匙串为准、文件为镜像，这里只读文件——
//! 我们不写第三方文件，也不主动刷新（刷新会把新 token 写回去，见 ADR 0010），
//! 过期就提示用户打开一次 Claude Code。
//!
//! 接口要求登录态带 `user:profile` scope；`claude setup-token` 生成的纯推理 token
//! 没有这个 scope，会被拒，所以先在本地筛掉，给一句能照做的提示。

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::{parse_resets_at, sanitize_percent};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const USER_AGENT: &str = "claude-code/2.1.69";
const USAGE_SCOPE: &str = "user:profile";
const TIMEOUT: Duration = Duration::from_secs(12);
/// 顶层的固定窗口：键名 → (kind, 中文标签)。
const WINDOWS: [(&str, &str, &str); 3] = [
    ("five_hour", "session_5h", "5 小时"),
    ("seven_day", "weekly", "7 天"),
    ("seven_day_sonnet", "weekly_sonnet", "7 天 Sonnet"),
];

pub fn fetch_usage() -> Result<(Vec<OfficialQuotaWindow>, String), String> {
    let token = load_access_token(&credentials_path())?;
    let raw = request_usage(&token)?;
    Ok((parse_usage(&raw)?, Utc::now().to_rfc3339()))
}

pub fn credentials_path() -> PathBuf {
    crate::ingest::default_home()
        .join(".claude")
        .join(".credentials.json")
}

/// 顶层三个固定窗口用 `utilization`，按模型拆的周窗口在 `limits[]` 里用 `percent`。
/// 两边都是 0–100，缺哪个跳哪个，全缺才算结构异常。
pub fn parse_usage(raw: &str) -> Result<Vec<OfficialQuotaWindow>, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|e| format!("Claude 用量 JSON 解析失败：{e}"))?;

    let mut windows = Vec::new();
    for (key, kind, label) in WINDOWS {
        let Some(node) = value.get(key) else { continue };
        let Some(percent) = node
            .get("utilization")
            .and_then(Value::as_f64)
            .and_then(sanitize_percent)
        else {
            continue;
        };
        windows.push(OfficialQuotaWindow {
            kind: kind.to_string(),
            label: label.to_string(),
            used_percent: Some(percent),
            resets_at: node.get("resets_at").and_then(parse_resets_at),
            ..Default::default()
        });
    }
    windows.extend(scoped_weekly_windows(&value));

    if windows.is_empty() {
        return Err("Claude 用量响应里没有可用的已用百分比".to_string());
    }
    Ok(windows)
}

/// Anthropic 把按模型的周窗口从 `seven_day_<model>` 挪进了 `limits[]`（老键现在返回
/// null），条目形如 `{kind: "weekly_scoped", percent, scope.model.display_name}`。
/// 模型名不写死，接口给谁就展示谁。
fn scoped_weekly_windows(value: &Value) -> Vec<OfficialQuotaWindow> {
    let Some(entries) = value.get("limits").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|entry| entry.get("kind").and_then(Value::as_str) == Some("weekly_scoped"))
        .filter_map(|entry| {
            let model = entry
                .pointer("/scope/model/display_name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())?;
            let percent = entry
                .get("percent")
                .and_then(Value::as_f64)
                .and_then(sanitize_percent)?;
            Some(OfficialQuotaWindow {
                kind: format!("weekly_{}", model.to_lowercase().replace(' ', "_")),
                label: format!("7 天 {model}"),
                used_percent: Some(percent),
                resets_at: entry.get("resets_at").and_then(parse_resets_at),
                ..Default::default()
            })
        })
        .collect()
}

pub fn load_access_token(path: &Path) -> Result<String, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|_| "未找到 Claude Code 登录态，请先运行 claude 并登录".to_string())?;
    let oauth = serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|value| value.get("claudeAiOauth").cloned())
        .ok_or_else(|| "Claude 登录态文件结构变了，读不到 claudeAiOauth".to_string())?;

    if let Some(scopes) = oauth.get("scopes").and_then(Value::as_array) {
        if !scopes
            .iter()
            .any(|scope| scope.as_str() == Some(USAGE_SCOPE))
        {
            return Err(
                "当前 Claude 登录态没有读用量的权限，请运行 claude 重新登录一次".to_string(),
            );
        }
    }
    if is_expired(&oauth, Utc::now().timestamp_millis()) {
        return Err("Claude 登录态已过期，请打开一次 Claude Code 以刷新".to_string());
    }
    oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Claude 登录态里没有 accessToken".to_string())
}

/// `expiresAt` 缺失或为 0 时不当成过期——第三方代理会把它写成 0，让接口自己判。
pub fn is_expired(oauth: &Value, now_ms: i64) -> bool {
    match oauth.get("expiresAt").and_then(Value::as_i64) {
        Some(expires_at) if expires_at > 0 => expires_at <= now_ms,
        _ => false,
    }
}

fn request_usage(token: &str) -> Result<String, String> {
    let request = crate::net::agent_with_timeout(TIMEOUT)
        .get(USAGE_URL)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .set("anthropic-beta", OAUTH_BETA)
        .set("User-Agent", USER_AGENT);
    match request.call() {
        Ok(response) => response
            .into_string()
            .map_err(|e| format!("读取 Claude 用量响应失败：{e}")),
        Err(ureq::Error::Status(401 | 403, _)) => {
            Err("Claude 登录已失效，请打开一次 Claude Code 重新登录".to_string())
        }
        // Anthropic 对这个端点限流较紧，手动狂刷会更糟，提示里说清楚。
        Err(ureq::Error::Status(429, _)) => {
            Err("Claude 用量接口被限流，稍后会自动恢复，别反复手动刷新".to_string())
        }
        Err(ureq::Error::Status(code, response)) => {
            let _ = response.into_string();
            Err(format!("拉取 Claude 用量失败：HTTP {code}"))
        }
        Err(_) => Err("无法连接 Claude 用量接口，请检查网络后重试".to_string()),
    }
}
