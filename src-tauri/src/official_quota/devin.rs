//! Devin / Windsurf 官方额度：`POST /exa.seat_management_pb.SeatManagementService/GetUserStatus`。
//!
//! Cognition 收购 Windsurf 后统一发 Devin 凭证，所以两者是同一条链路：apiKey 存在
//! VSCode 风格 `state.vscdb` 的 `windsurfAuthStatus` 里（和 Antigravity 同构的
//! `{apiKey, userStatusProtoBinaryBase64}`），形如 `devin-…`。
//!
//! 客户端目录两边都要找：装 Windsurf 的在 `Windsurf/`，装 Devin 的在 `Devin/`。
//!
//! 接口是 Connect 协议（`Connect-Protocol-Version: 1`），apiKey 走 body 的 metadata
//! 而不是 Authorization 头。

use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::{display_plan_label, sanitize_percent, QuotaSnapshot};
use crate::vscode_state;

/// 装了哪个客户端就从哪个目录读，两个都可能存在。
const APP_DIRS: [&str; 2] = ["Windsurf", "Devin"];
const AUTH_STATUS_KEY: &str = "windsurfAuthStatus";
const API_SERVER: &str = "https://server.codeium.com";
const SERVICE_PATH: &str = "exa.seat_management_pb.SeatManagementService/GetUserStatus";
/// 客户端自报的兼容版本，跟着 openusage 走；服务端只用它做兼容判断。
const COMPAT_VERSION: &str = "1.108.2";
const TIMEOUT: Duration = Duration::from_secs(15);
const NOT_SIGNED_IN: &str = "尚未登录 Devin / Windsurf，请先打开客户端并登录";

pub fn fetch_usage() -> Result<QuotaSnapshot, String> {
    let api_key = load_api_key()?;
    let raw = request_user_status(&api_key)?;
    Ok(
        QuotaSnapshot::new(parse_user_status(&raw)?, Utc::now().to_rfc3339())
            .with_plan(parse_plan(&raw)),
    )
}

pub fn parse_plan(raw: &str) -> Option<String> {
    let value: Value = serde_json::from_str(raw).ok()?;
    value
        .pointer("/userStatus/planStatus/planInfo/planName")
        .and_then(Value::as_str)
        .and_then(display_plan_label)
}

fn load_api_key() -> Result<String, String> {
    for app in APP_DIRS {
        let Some(dir) = vscode_state::global_storage_dir(app) else {
            continue;
        };
        if let Some(key) = read_api_key_at(&dir)? {
            return Ok(key);
        }
    }
    Err(NOT_SIGNED_IN.to_string())
}

/// 探针：Windsurf / Devin 任一客户端里有没有 apiKey。
pub fn has_local_api_key() -> bool {
    load_api_key().is_ok()
}

pub fn read_api_key_at(global_storage: &Path) -> Result<Option<String>, String> {
    let Some(conn) = vscode_state::open_read_only(global_storage)? else {
        return Ok(None);
    };
    Ok(vscode_state::read_item(&conn, AUTH_STATUS_KEY)
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| {
            value
                .get("apiKey")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(str::to_string)
        }))
}

/// `userStatus.planStatus` 下的日 / 周两档，给的是**剩余**百分比，取反才是已用。
/// 重置时间是 epoch 秒，字段名带 `Unix` 后缀。
pub fn parse_user_status(raw: &str) -> Result<Vec<OfficialQuotaWindow>, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|e| format!("Devin 额度 JSON 解析失败：{e}"))?;
    let plan = value
        .pointer("/userStatus/planStatus")
        .ok_or_else(|| "Devin 额度响应里没有 planStatus".to_string())?;

    let daily = used_percent(plan, "dailyQuotaRemainingPercent");
    let weekly = used_percent(plan, "weeklyQuotaRemainingPercent");
    // 免费档会把日额度藏起来，但如果周额度也没有，那日额度就是唯一有意义的那条。
    let hide_daily = plan
        .pointer("/planInfo/hideDailyQuota")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut windows = Vec::new();
    if daily.is_some() && (!hide_daily || weekly.is_none()) {
        windows.push(OfficialQuotaWindow {
            kind: "daily".to_string(),
            label: "日额度".to_string(),
            used_percent: daily,
            resets_at: resets_at(plan, "dailyQuotaResetAtUnix"),
            ..Default::default()
        });
    }
    if weekly.is_some() {
        windows.push(OfficialQuotaWindow {
            kind: "weekly".to_string(),
            label: "周额度".to_string(),
            used_percent: weekly,
            resets_at: resets_at(plan, "weeklyQuotaResetAtUnix"),
            ..Default::default()
        });
    }

    if windows.is_empty() {
        return Err("Devin 额度响应里没有可用的已用百分比".to_string());
    }
    Ok(windows)
}

fn used_percent(plan: &Value, field: &str) -> Option<f64> {
    plan.get(field)
        .and_then(number)
        .and_then(|remaining| sanitize_percent(100.0 - remaining))
}

/// 这个接口会把数字包成字符串（`"100"`），两种都要认。
fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

fn resets_at(plan: &Value, field: &str) -> Option<String> {
    let seconds = plan.get(field).and_then(number)? as i64;
    chrono::DateTime::from_timestamp(seconds, 0).map(|value| value.to_rfc3339())
}

fn request_user_status(api_key: &str) -> Result<String, String> {
    let body = json!({
        "metadata": {
            "apiKey": api_key,
            "ideName": "devin",
            "ideVersion": COMPAT_VERSION,
            "extensionName": "devin",
            "extensionVersion": COMPAT_VERSION,
            "locale": "en"
        }
    });
    let request = crate::net::agent_with_timeout(TIMEOUT)
        .post(&format!("{API_SERVER}/{SERVICE_PATH}"))
        .set("Content-Type", "application/json")
        .set("Connect-Protocol-Version", "1");
    match request.send_string(&body.to_string()) {
        Ok(response) => response
            .into_string()
            .map_err(|e| format!("读取 Devin 额度响应失败：{e}")),
        Err(ureq::Error::Status(401 | 403, _)) => {
            Err("Devin / Windsurf 登录已失效，请重新打开客户端登录".to_string())
        }
        Err(ureq::Error::Status(code, response)) => {
            let _ = response.into_string();
            Err(format!("拉取 Devin 额度失败：HTTP {code}"))
        }
        Err(_) => Err("无法连接 Devin 额度接口，请检查网络后重试".to_string()),
    }
}
