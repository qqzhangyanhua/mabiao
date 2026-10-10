//! GLM / Z.ai Coding Plan。
//!
//! 只打一条：`GET {origin}/api/monitor/usage/quota/limit`。
//! Authorization 放裸密钥，不加 Bearer（智谱 / Z.ai 这套接口的约定）。

use serde_json::Value;

use crate::domain::OfficialQuotaWindow;

use super::{origin_of_normalized, QuotaRequest};

const WHO: &str = "GLM / Z.ai Coding Plan 额度接口";

pub fn urls(base: &str) -> Vec<QuotaRequest> {
    vec![QuotaRequest {
        url: format!(
            "{}/api/monitor/usage/quota/limit",
            origin_of_normalized(base)
        ),
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
    if root.get("success").and_then(Value::as_bool) == Some(false) {
        let msg = root
            .get("msg")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .unwrap_or("接口返回了失败状态");
        return Err(msg.to_string());
    }
    let data = root
        .get("data")
        .ok_or_else(|| format!("{WHO}里没有 data，这个地址可能不是 Coding Plan 额度接口"))?;
    let limits = data
        .get("limits")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{WHO}里没有 limits"))?;

    let mut windows = Vec::new();
    for limit in limits {
        let kind_type = limit
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let unit = limit
            .get("unit")
            .and_then(Value::as_i64)
            .or_else(|| limit.get("unit").and_then(Value::as_f64).map(|n| n as i64))
            .unwrap_or(0);
        let number = limit
            .get("number")
            .and_then(Value::as_i64)
            .or_else(|| {
                limit
                    .get("number")
                    .and_then(Value::as_f64)
                    .map(|n| n as i64)
            })
            .unwrap_or(0);
        let (kind, label) = if kind_type.eq_ignore_ascii_case("TIME_LIMIT") {
            ("mcp_month".to_string(), "MCP · 月".to_string())
        } else if unit == 3 {
            let hours = if number <= 0 { 5 } else { number };
            (format!("hours_{hours}"), format!("{hours} 小时"))
        } else if unit == 6 {
            ("days_7".to_string(), "7 天".to_string())
        } else {
            ("allowance".to_string(), "额度".to_string())
        };
        let used_percent = limit.get("percentage").and_then(Value::as_f64);
        let resets_at = limit
            .get("nextResetTime")
            .and_then(|node| node.as_i64().or_else(|| node.as_f64().map(|n| n as i64)))
            .and_then(unix_to_rfc3339);
        windows.push(OfficialQuotaWindow {
            kind,
            label,
            used_percent,
            resets_at,
            used_amount: None,
            limit_amount: None,
            currency: None,
            ..Default::default()
        });
    }
    if windows.is_empty() {
        return Err(format!("{WHO}里没有可用的额度窗口"));
    }
    Ok(windows)
}

fn unix_to_rfc3339(raw: i64) -> Option<String> {
    let secs = if raw > 1_000_000_000_000 {
        raw / 1000
    } else {
        raw
    };
    chrono::DateTime::from_timestamp(secs, 0).map(|dt| dt.to_rfc3339())
}
