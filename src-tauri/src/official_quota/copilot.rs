//! GitHub Copilot 官方额度：`GET https://api.github.com/copilot_internal/user`。
//!
//! 凭证按「不弹窗的文件优先」找：Copilot 编辑器插件的
//! `~/.config/github-copilot/apps.json`（老版本是 `hosts.json`），其次 GitHub CLI 的
//! `~/.config/gh/hosts.yml` 里的 `oauth_token`。macOS 钥匙串那条不做——会弹授权框。
//!
//! 注意 `Authorization` 用的是 `token` 而不是 `Bearer`，这是这个内部端点认的方案；
//! 另外要带上 Copilot 客户端那几个 `Editor-*` 头。

use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::{parse_resets_at, sanitize_percent};

const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";
const TIMEOUT: Duration = Duration::from_secs(15);
const NOT_SIGNED_IN: &str =
    "未找到 GitHub Copilot 登录态，请先在编辑器里登录 Copilot 或运行 gh auth login";
/// `quota_snapshots` 下的三档：键名 → (kind, 中文标签)。
const SNAPSHOTS: [(&str, &str, &str); 3] = [
    ("premium_interactions", "credits", "高级交互"),
    ("chat", "chat", "Chat"),
    ("completions", "completions", "补全"),
];

pub fn fetch_usage() -> Result<(Vec<OfficialQuotaWindow>, String), String> {
    let token = load_token().ok_or_else(|| NOT_SIGNED_IN.to_string())?;
    let raw = request_usage(&token)?;
    Ok((parse_usage(&raw)?, Utc::now().to_rfc3339()))
}

fn config_home() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::ingest::default_home().join(".config"))
}

pub fn credential_paths() -> Vec<PathBuf> {
    let config = config_home();
    vec![
        config.join("github-copilot").join("apps.json"),
        config.join("github-copilot").join("hosts.json"),
        config.join("gh").join("hosts.yml"),
    ]
}

fn load_token() -> Option<String> {
    credential_paths().into_iter().find_map(|path| {
        let raw = std::fs::read_to_string(&path).ok()?;
        if path.extension().and_then(|ext| ext.to_str()) == Some("yml") {
            parse_gh_hosts_token(&raw)
        } else {
            parse_copilot_config_token(&raw)
        }
    })
}

/// Copilot 插件的配置是个对象，键形如 `github.com:<clientId>`，值里带 `oauth_token`。
/// 键名不稳定，所以扫所有条目，取第一个有 token 的。
pub fn parse_copilot_config_token(raw: &str) -> Option<String> {
    serde_json::from_str::<Value>(raw)
        .ok()?
        .as_object()?
        .values()
        .filter_map(|entry| entry.get("oauth_token").and_then(Value::as_str))
        .map(str::trim)
        .find(|token| !token.is_empty())
        .map(str::to_string)
}

/// `gh` 的 hosts.yml 只需要取 `oauth_token:` 那一行，不值得引 YAML 依赖。
/// 只认 github.com 段之后的第一个 token，避免读到企业实例的。
pub fn parse_gh_hosts_token(raw: &str) -> Option<String> {
    let mut in_github_com = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if !line.starts_with([' ', '\t']) && trimmed.ends_with(':') {
            in_github_com = trimmed == "github.com:";
            continue;
        }
        if !in_github_com {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("oauth_token:") {
            let token = value.trim().trim_matches(['"', '\'']).trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// `quota_snapshots.{premium_interactions,chat,completions}`，每档给的是**剩余**口径：
/// `percent_remaining`，或 `remaining` / `entitlement`。取反才是已用。
pub fn parse_usage(raw: &str) -> Result<Vec<OfficialQuotaWindow>, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|e| format!("Copilot 额度 JSON 解析失败：{e}"))?;
    let resets_at = reset_date(value.get("quota_reset_date"))
        .or_else(|| reset_date(value.get("limited_user_reset_date")));
    let snapshots = value
        .get("quota_snapshots")
        .ok_or_else(|| "Copilot 额度响应里没有 quota_snapshots".to_string())?;

    let mut windows = Vec::new();
    for (key, kind, label) in SNAPSHOTS {
        let Some(node) = snapshots.get(key) else {
            continue;
        };
        let Some(percent) = used_percent(node) else {
            continue;
        };
        windows.push(OfficialQuotaWindow {
            kind: kind.to_string(),
            label: label.to_string(),
            used_percent: Some(percent),
            resets_at: resets_at.clone(),
            ..Default::default()
        });
    }

    if windows.is_empty() {
        return Err("Copilot 额度响应里没有可用的已用百分比".to_string());
    }
    Ok(windows)
}

/// GitHub 给的重置时间是纯日期（`2026-09-01`），通用解析认不了，补一层按 UTC 零点算。
fn reset_date(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(parsed) = parse_resets_at(value) {
        return Some(parsed);
    }
    let text = value.as_str()?.trim();
    let date = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
    Some(
        date.and_hms_opt(0, 0, 0)?
            .and_utc()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

/// 无限额度和「零额度占位」都不该显示成百分比：
/// 前者是 `unlimited` 或 `-1` 哨兵，后者是组织按量计费席位返回的 `entitlement: 0`。
fn used_percent(node: &Value) -> Option<f64> {
    let entitlement = node.get("entitlement").and_then(Value::as_f64);
    let remaining = node.get("remaining").and_then(Value::as_f64);
    if node.get("unlimited").and_then(Value::as_bool) == Some(true)
        || entitlement == Some(-1.0)
        || remaining == Some(-1.0)
        || entitlement == Some(0.0)
    {
        return None;
    }
    if let Some(percent_remaining) = node.get("percent_remaining").and_then(Value::as_f64) {
        return sanitize_percent(100.0 - percent_remaining);
    }
    match (entitlement, remaining) {
        (Some(entitlement), Some(remaining)) if entitlement > 0.0 => {
            sanitize_percent(100.0 - (remaining / entitlement) * 100.0)
        }
        _ => None,
    }
}

fn request_usage(token: &str) -> Result<String, String> {
    let request = crate::net::agent_with_timeout(TIMEOUT)
        .get(USAGE_URL)
        .set("Authorization", &format!("token {token}"))
        .set("Accept", "application/json")
        .set("Editor-Version", "vscode/1.96.2")
        .set("Editor-Plugin-Version", "copilot-chat/0.26.7")
        .set("User-Agent", "GitHubCopilotChat/0.26.7")
        .set("X-Github-Api-Version", "2025-04-01");
    match request.call() {
        Ok(response) => response
            .into_string()
            .map_err(|e| format!("读取 Copilot 额度响应失败：{e}")),
        Err(ureq::Error::Status(401 | 403, _)) => {
            Err("Copilot 登录已失效，请在编辑器里重新登录".to_string())
        }
        Err(ureq::Error::Status(code, response)) => {
            let _ = response.into_string();
            Err(format!("拉取 Copilot 额度失败：HTTP {code}"))
        }
        Err(_) => Err("无法连接 Copilot 额度接口，请检查网络后重试".to_string()),
    }
}
