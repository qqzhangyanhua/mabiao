//! Command Code 套餐额度。
//!
//! 只打一条：`GET {origin}/alpha/billing/credits`，Bearer。
//! 套餐 key 回 5 小时 / 7 天窗（`windowLimits.fiveHour` / `weekly`）；
//! 按量 key 没有窗口，不当成成功。端点对齐 magpie `planquota.go`。

use serde_json::Value;

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::parse_resets_at;

use super::{origin_of_normalized, QuotaRequest};

const WHO: &str = "Command Code 额度接口";

pub fn urls(base: &str) -> Vec<QuotaRequest> {
    vec![QuotaRequest {
        url: format!("{}/alpha/billing/credits", origin_of_normalized(base)),
        required: true,
    }]
}

pub fn parse(bodies: &[&str]) -> Result<Vec<OfficialQuotaWindow>, String> {
    let raw = bodies.first().copied().unwrap_or_default();
    if raw.trim().is_empty() {
        return Err(format!("{WHO}返回了空响应，请确认 base URL 与预设类型"));
    }
    let root: Value = serde_json::from_str(raw).map_err(|_| {
        format!("{WHO}返回的不是合法 JSON（多半是网页或登录页），请确认 base URL 与预设类型")
    })?;
    let limits = root.get("windowLimits").cloned().unwrap_or(Value::Null);
    let mut windows = Vec::new();
    push_window(&mut windows, &limits, "fiveHour", "hours_5", "5 小时");
    push_window(&mut windows, &limits, "weekly", "days_7", "7 天");
    if windows.is_empty() {
        return Err(format!(
            "{WHO}里没有 5 小时 / 7 天窗口，这把 key 可能不是套餐 key"
        ));
    }
    Ok(windows)
}

fn push_window(
    out: &mut Vec<OfficialQuotaWindow>,
    limits: &Value,
    field: &str,
    kind: &str,
    label: &str,
) {
    let Some(node) = limits.get(field).filter(|n| !n.is_null()) else {
        return;
    };
    let Some(limit) = json_f64(node, "cap").filter(|v| *v > 0.0) else {
        return;
    };
    let Some(used) = json_f64(node, "used") else {
        return;
    };
    let resets_at = node.get("resetAt").and_then(parse_resets_at);
    out.push(OfficialQuotaWindow {
        kind: kind.to_string(),
        label: label.to_string(),
        used_percent: Some((used / limit * 100.0).clamp(0.0, 100.0)),
        resets_at,
        used_amount: Some(used),
        limit_amount: Some(limit),
        currency: None,
        ..Default::default()
    });
}

fn json_f64(value: &Value, name: &str) -> Option<f64> {
    let node = value.get(name)?;
    if let Some(n) = node.as_f64() {
        return Some(n);
    }
    node.as_str()?.trim().parse().ok()
}
