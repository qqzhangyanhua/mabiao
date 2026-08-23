use std::time::Duration;

use chrono::Utc;
use serde_json::Value;

use crate::cursor_account;
use crate::domain::OfficialQuotaWindow;
use crate::official_quota::{parse_resets_at, sanitize_percent};

const USAGE_SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";
const TIMEOUT: Duration = Duration::from_secs(15);

pub fn fetch_usage_summary() -> Result<(Vec<OfficialQuotaWindow>, String), String> {
    let token = cursor_account::current_token()?;
    let raw = request_usage_summary(&token)?;
    let windows = parse_usage_summary(&raw)?;
    Ok((windows, Utc::now().to_rfc3339()))
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

fn request_usage_summary(token: &str) -> Result<String, String> {
    let request = crate::net::agent_with_timeout(TIMEOUT)
        .get(USAGE_SUMMARY_URL)
        .set("Cookie", &format!("WorkosCursorSessionToken={token}"))
        .set("Origin", "https://cursor.com");
    match request.call() {
        Ok(response) => response
            .into_string()
            .map_err(|e| format!("读取 Cursor 限额响应失败：{e}")),
        Err(ureq::Error::Status(401 | 403, _)) => Err(cursor_account::auth_expired_error()),
        Err(ureq::Error::Status(code, response)) => {
            let _ = response.into_string();
            Err(format!("拉取 Cursor 限额失败：HTTP {code}"))
        }
        Err(_) => Err(cursor_account::network_failure_error()),
    }
}
