use std::time::Duration;

use serde_json::Value;

use crate::cursor_account;
use crate::cursor_credentials;
use crate::domain::OfficialQuotaWindow;
use crate::official_quota::{
    display_plan_label, parse_resets_at, sanitize_percent, HttpRequest, QuotaEnv, QuotaPaths,
    QuotaSnapshot, QuotaTransport,
};

const USAGE_SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";
const TIMEOUT: Duration = Duration::from_secs(15);

pub fn fetch_usage_summary() -> Result<QuotaSnapshot, String> {
    fetch_usage_summary_in(&QuotaEnv::live(QuotaPaths::app_data()))
}

pub fn fetch_usage_summary_in(env: &QuotaEnv) -> Result<QuotaSnapshot, String> {
    let credential = env
        .cursor_storage_dir()
        .and_then(|dir| cursor_credentials::read_credential_at(&dir).ok().flatten())
        .filter(|value| !value.is_expired_at(env.now().timestamp_millis()))
        .ok_or_else(cursor_account::missing_token_error)?;
    let mut snapshot = fetch_with_session_token_in(env, &credential.session_token)?;
    if snapshot.plan.is_none() {
        snapshot.plan = credential
            .membership
            .as_deref()
            .and_then(display_plan_label);
    }
    Ok(snapshot)
}

/// 用已经拼好的 session cookie 取用量。不读本机库，也不刷新。
pub(crate) fn fetch_with_session_token_in(
    env: &QuotaEnv,
    token: &str,
) -> Result<QuotaSnapshot, String> {
    let url = env.rebase_url(USAGE_SUMMARY_URL);
    let raw = request_usage_summary_via(env.transport.as_ref(), token, &url)?;
    let windows = parse_usage_summary(&raw)?;
    let plan = parse_membership_type(&raw);
    Ok(QuotaSnapshot::new(windows, env.now().to_rfc3339()).with_plan(plan))
}

/// `GET /api/usage-summary` 顶层的 `membershipType`，如 `pro` / `ultra`。
pub fn parse_membership_type(raw: &str) -> Option<String> {
    let value: Value = serde_json::from_str(raw).ok()?;
    value
        .get("membershipType")
        .or_else(|| value.get("membership_type"))
        .and_then(Value::as_str)
        .and_then(display_plan_label)
}

/// 把 `GET /api/usage-summary` 拆成独立窗口：总量 / Auto / API / 按需。
/// 缺某一档就跳过，全部都没有才报结构变更。
pub fn parse_usage_summary(raw: &str) -> Result<Vec<OfficialQuotaWindow>, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|e| format!("Cursor 限额 JSON 解析失败：{e}"))?;
    let resets_at = value.get("billingCycleEnd").and_then(parse_resets_at);
    let plan = value
        .pointer("/individualUsage/plan")
        .or_else(|| value.get("plan"));

    let mut windows = Vec::new();
    if let Some(plan) = plan.filter(|node| enabled(node).unwrap_or(true)) {
        push_window(
            &mut windows,
            plan_percent(plan, "totalPercentUsed"),
            "billing_cycle",
            "总量",
            resets_at.clone(),
        );
        push_window(
            &mut windows,
            named_percent(plan, "autoPercentUsed"),
            "auto",
            "Auto",
            resets_at.clone(),
        );
        push_window(
            &mut windows,
            named_percent(plan, "apiPercentUsed"),
            "api",
            "API",
            resets_at.clone(),
        );
    }
    if let Some(window) = parse_on_demand(&value, resets_at.clone()) {
        windows.push(window);
    }

    if windows.is_empty() {
        if resets_at.is_some() {
            windows.push(OfficialQuotaWindow {
                kind: "billing_cycle".to_string(),
                label: "总量".to_string(),
                used_percent: None,
                resets_at,
                ..Default::default()
            });
        } else {
            return Err("Cursor 限额响应里没有可用的已用百分比".to_string());
        }
    }
    Ok(windows)
}

fn plan_percent(plan: &Value, field: &str) -> Option<f64> {
    named_percent(plan, field).or_else(|| percent_from_used_limit(plan))
}

fn named_percent(node: &Value, field: &str) -> Option<f64> {
    node.get(field)
        .and_then(Value::as_f64)
        .and_then(sanitize_percent)
}

fn parse_on_demand(root: &Value, resets_at: Option<String>) -> Option<OfficialQuotaWindow> {
    let individual = root.pointer("/individualUsage/onDemand");
    let team = root.pointer("/teamUsage/onDemand");
    let node = individual
        .filter(|demand| enabled(demand).unwrap_or(false))
        .filter(|demand| percent_from_used_limit(demand).is_some())
        .or_else(|| team.filter(|demand| percent_from_used_limit(demand).is_some()))?;
    Some(OfficialQuotaWindow {
        kind: "on_demand".to_string(),
        label: "按需".to_string(),
        used_percent: percent_from_used_limit(node).and_then(sanitize_percent),
        resets_at,
        ..Default::default()
    })
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

fn enabled(node: &Value) -> Option<bool> {
    node.get("enabled").and_then(Value::as_bool)
}

fn percent_from_used_limit(plan: &Value) -> Option<f64> {
    let used = plan.get("used").and_then(Value::as_f64)?;
    let limit = plan.get("limit").and_then(Value::as_f64)?;
    if limit <= 0.0 {
        return None;
    }
    Some((used / limit * 100.0).clamp(0.0, 100.0))
}

fn request_usage_summary_via(
    transport: &dyn QuotaTransport,
    token: &str,
    url: &str,
) -> Result<String, String> {
    let request = HttpRequest::get(url, TIMEOUT)
        .header("Cookie", format!("WorkosCursorSessionToken={token}"))
        .header("Origin", "https://cursor.com");
    let response = transport
        .http(&request)
        .map_err(|_| cursor_account::network_failure_error())?;
    match response.status {
        200..=299 => response
            .body_string()
            .map_err(|e| format!("读取 Cursor 限额响应失败：{e}")),
        401 | 403 => Err(cursor_account::auth_expired_error()),
        code => Err(format!("拉取 Cursor 限额失败：HTTP {code}")),
    }
}
