//! OpenCode (Zen / Go) 官方额度：`GET https://opencode.ai/zen/go/v1/usage`。
//!
//! 凭证读 OpenCode 数据目录下的 `auth.json` 里 `opencode-go.key`。数据目录按
//! OpenCode 自己的顺序解析：`OPENCODE_DATA_DIR` > `$XDG_DATA_HOME/opencode` >
//! `~/.local/share/opencode`。
//!
//! 文件不存在是「没登录 OpenCode Zen」的正常情况；文件在但读不动 / 不是合法 JSON
//! 要报出来，别把坏掉的存储当成没登录。

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::{parse_resets_at, sanitize_percent};

const USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";
const AUTH_ENTRY: &str = "opencode-go";
const TIMEOUT: Duration = Duration::from_secs(15);
const NOT_SIGNED_IN: &str = "尚未登录 OpenCode Zen，请先运行 opencode 并登录";
/// 响应 `usage` 下的三个窗口：键名 → (kind, 中文标签)。
const WINDOWS: [(&str, &str, &str); 3] = [
    ("rolling", "session", "滚动"),
    ("weekly", "weekly", "周"),
    ("monthly", "monthly", "月"),
];

pub fn fetch_usage() -> Result<(Vec<OfficialQuotaWindow>, String), String> {
    let key = load_api_key(&auth_path())?.ok_or_else(|| NOT_SIGNED_IN.to_string())?;
    fetch_with_access_token(&key)
}

/// 用调用方给的 API key 取用量。不读本机文件。
pub(crate) fn fetch_with_access_token(
    key: &str,
) -> Result<(Vec<OfficialQuotaWindow>, String), String> {
    fetch_with_access_token_at(key, None)
}

pub(crate) fn fetch_with_access_token_at(
    key: &str,
    quota_base: Option<&str>,
) -> Result<(Vec<OfficialQuotaWindow>, String), String> {
    let url = super::rebase_quota_url(USAGE_URL, quota_base);
    let raw = request_usage(key, &url)?;
    Ok((parse_usage(&raw)?, Utc::now().to_rfc3339()))
}

pub fn data_dir() -> PathBuf {
    data_dir_in(&crate::scan_paths::ScanEnv::capture_for_home(
        &crate::ingest::default_home(),
    ))
}

pub(crate) fn data_dir_in(env: &crate::scan_paths::ScanEnv) -> PathBuf {
    env.client_credential_root(crate::domain::Source::Opencode, "auth.json")
}

pub fn auth_path() -> PathBuf {
    auth_path_in(&crate::scan_paths::ScanEnv::capture_for_home(
        &crate::ingest::default_home(),
    ))
}

pub(crate) fn auth_path_in(env: &crate::scan_paths::ScanEnv) -> PathBuf {
    data_dir_in(env).join("auth.json")
}

/// 文件缺失 → `Ok(None)`（没登录）；文件在但坏了 → `Err`（别当成没登录）。
/// 只认 `opencode-go` 这一条，其它 provider 的条目忽略。
pub fn load_api_key(path: &Path) -> Result<Option<String>, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读取 OpenCode 登录态失败：{error}")),
    };
    let value: Value = serde_json::from_str(&raw)
        .map_err(|_| "OpenCode 的 auth.json 不是合法 JSON".to_string())?;
    Ok(value
        .get(AUTH_ENTRY)
        .and_then(|entry| entry.get("key"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string))
}

/// `usage.{rolling,weekly,monthly}`，每档 `percent`（已用，0–100）+ `resetsAt`（ISO）。
pub fn parse_usage(raw: &str) -> Result<Vec<OfficialQuotaWindow>, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|e| format!("OpenCode 用量 JSON 解析失败：{e}"))?;
    let usage = value
        .get("usage")
        .ok_or_else(|| "OpenCode 用量响应里没有 usage".to_string())?;

    let mut windows = Vec::new();
    for (key, kind, label) in WINDOWS {
        let Some(node) = usage.get(key) else { continue };
        let Some(percent) = node
            .get("percent")
            .and_then(Value::as_f64)
            .and_then(sanitize_percent)
        else {
            continue;
        };
        windows.push(OfficialQuotaWindow {
            kind: kind.to_string(),
            label: label.to_string(),
            used_percent: Some(percent),
            resets_at: node.get("resetsAt").and_then(parse_resets_at),
            ..Default::default()
        });
    }

    if windows.is_empty() {
        return Err("OpenCode 用量响应里没有可用的已用百分比".to_string());
    }
    Ok(windows)
}

/// 出错时上游会给 `{type, error: {type, message}}`；HTML / Cloudflare 页面则没有。
fn error_detail(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .pointer("/error/type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn request_usage(api_key: &str, url: &str) -> Result<String, String> {
    let request = crate::net::agent_for_url(url, TIMEOUT)
        .get(url)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Accept", "application/json");
    match request.call() {
        Ok(response) => response
            .into_string()
            .map_err(|e| format!("读取 OpenCode 用量响应失败：{e}")),
        Err(ureq::Error::Status(401 | 403, response)) => {
            let detail = response
                .into_string()
                .ok()
                .as_deref()
                .and_then(error_detail);
            Err(match detail {
                Some(kind) => format!("OpenCode 登录已失效（{kind}），请重新运行 opencode 登录"),
                None => "OpenCode 登录已失效，请重新运行 opencode 登录".to_string(),
            })
        }
        Err(ureq::Error::Status(code, response)) => {
            let detail = response
                .into_string()
                .ok()
                .as_deref()
                .and_then(error_detail);
            Err(match detail {
                Some(kind) => format!("拉取 OpenCode 用量失败：HTTP {code}（{kind}）"),
                None => format!("拉取 OpenCode 用量失败：HTTP {code}"),
            })
        }
        Err(_) => Err("无法连接 OpenCode 用量接口，请检查网络后重试".to_string()),
    }
}
