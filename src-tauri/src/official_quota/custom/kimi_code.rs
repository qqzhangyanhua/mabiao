//! Kimi Code 套餐额度。
//!
//! 只打一条：`GET {base}/v1/usages`，Bearer。`base` 归一化后若是
//! `api.kimi.com` / `api.kimi.ai` 且路径没有 `/coding`，补上 `/coding`，
//! 与 kimi-cli 的 `/coding/v1/usages` 对齐。

use serde_json::Value;

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::parse_resets_at;

use super::QuotaRequest;

const WHO: &str = "Kimi Code 用量接口";

pub fn urls(base: &str) -> Vec<QuotaRequest> {
    vec![QuotaRequest {
        url: format!("{}/usages", kimi_code_base(base)),
        required: true,
    }]
}

/// 归一化后的根再拼 `/v1`：`https://api.kimi.com/coding` → `…/coding/v1`。
fn kimi_code_base(base: &str) -> String {
    let with_coding = if is_kimi_host(base) && !base.contains("/coding") {
        format!("{base}/coding")
    } else {
        base.to_string()
    };
    format!("{with_coding}/v1")
}

fn is_kimi_host(base: &str) -> bool {
    let rest = base
        .strip_prefix("https://")
        .or_else(|| base.strip_prefix("http://"))
        .unwrap_or(base);
    let host = rest
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    matches!(host, "api.kimi.com" | "api.kimi.ai")
}

pub fn parse(bodies: &[&str]) -> Result<Vec<OfficialQuotaWindow>, String> {
    let raw = bodies.first().copied().unwrap_or_default();
    if raw.trim().is_empty() {
        return Err(format!("{WHO}返回了空响应，请确认 base URL 与预设类型"));
    }
    let root: Value = serde_json::from_str(raw).map_err(|_| {
        format!("{WHO}返回的不是合法 JSON（多半是网页或登录页），请确认 base URL 与预设类型")
    })?;

    let mut windows = Vec::new();
    if let Some(limits) = root.get("limits").and_then(Value::as_array) {
        for item in limits {
            let detail = item
                .get("detail")
                .filter(|node| !node.is_null())
                .or_else(|| item.get("window"))
                .cloned()
                .unwrap_or(Value::Null);
            let window = item.get("window").cloned().unwrap_or(Value::Null);
            let duration = json_f64(&window, "duration").unwrap_or(0.0);
            let unit = window.get("timeUnit").and_then(Value::as_str).unwrap_or("");
            let (kind, label) = span_label(duration, unit);
            if let Some(parsed) = window_from_detail(&detail, &kind, &label) {
                windows.push(parsed);
            }
        }
    }
    if let Some(usage) = root.get("usage") {
        if let Some(parsed) = window_from_detail(usage, "days_7", "7 天") {
            windows.push(parsed);
        }
    }
    if windows.is_empty() {
        return Err(format!("{WHO}里没有可用的额度窗口"));
    }
    Ok(windows)
}

fn span_label(duration: f64, unit: &str) -> (String, String) {
    let n = duration.max(0.0) as i64;
    if unit.contains("MINUTE") {
        return (format!("minutes_{n}"), format!("{n} 分钟"));
    }
    if unit.contains("HOUR") {
        return (format!("hours_{n}"), format!("{n} 小时"));
    }
    if unit.contains("DAY") {
        return (format!("days_{n}"), format!("{n} 天"));
    }
    ("allowance".to_string(), "额度".to_string())
}

fn window_from_detail(detail: &Value, kind: &str, label: &str) -> Option<OfficialQuotaWindow> {
    let limit = json_f64(detail, "limit").filter(|value| *value > 0.0)?;
    let used = json_f64(detail, "used")
        .or_else(|| json_f64(detail, "remaining").map(|left| limit - left))?;
    let resets_at = ["resetTime", "resetAt", "reset_at", "reset_time"]
        .into_iter()
        .find_map(|name| detail.get(name).and_then(parse_resets_at));
    Some(OfficialQuotaWindow {
        kind: kind.to_string(),
        label: label.to_string(),
        used_percent: Some((used / limit * 100.0).clamp(0.0, 100.0)),
        resets_at,
        used_amount: Some(used),
        limit_amount: Some(limit),
        currency: None,
        ..Default::default()
    })
}

fn json_f64(value: &Value, name: &str) -> Option<f64> {
    let node = value.get(name)?;
    if let Some(number) = node.as_f64() {
        return Some(number);
    }
    node.as_str()
        .and_then(|text| text.trim().parse::<f64>().ok())
}
