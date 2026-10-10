//! MiniMax Coding Plan。
//!
//! 只打一条：`GET https://{host}/v1/token_plan/remains`，Bearer。
//! 响应按剩余百分比报窗口；status 3 且没有总量的桶不在套餐里，跳过。

use serde_json::Value;

use crate::domain::OfficialQuotaWindow;

use super::{origin_of_normalized, QuotaRequest};

const WHO: &str = "MiniMax Coding Plan 余量接口";

pub fn urls(base: &str) -> Vec<QuotaRequest> {
    vec![QuotaRequest {
        url: format!("{}/v1/token_plan/remains", origin_of_normalized(base)),
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
    if let Some(error) = base_error(&root) {
        return Err(error);
    }
    let remains = root
        .get("model_remains")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{WHO}里没有 model_remains"))?;

    let mut windows = Vec::new();
    for bucket in remains {
        let name = bucket
            .get("model_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let interval_status = json_i64(bucket, "current_interval_status").unwrap_or(0);
        let week_status = json_i64(bucket, "current_weekly_status").unwrap_or(0);
        let interval_total = json_f64(bucket, "current_interval_total_count");
        let week_total = json_f64(bucket, "current_weekly_total_count");
        if interval_status == 3
            && week_status == 3
            && interval_total.unwrap_or(0.0) == 0.0
            && week_total.unwrap_or(0.0) == 0.0
        {
            continue;
        }
        let general = name.eq_ignore_ascii_case("general");
        push_minimax_window(
            &mut windows,
            &name,
            general,
            false,
            interval_status,
            json_f64(bucket, "current_interval_remaining_percent"),
            json_i64(bucket, "end_time"),
            json_i64(bucket, "start_time"),
            json_f64(bucket, "current_interval_usage_count"),
            interval_total,
        );
        push_minimax_window(
            &mut windows,
            &name,
            general,
            true,
            week_status,
            json_f64(bucket, "current_weekly_remaining_percent"),
            json_i64(bucket, "weekly_end_time"),
            json_i64(bucket, "weekly_start_time"),
            json_f64(bucket, "current_weekly_usage_count"),
            week_total,
        );
    }
    if windows.is_empty() {
        return Err(format!("{WHO}里没有可用的额度窗口"));
    }
    Ok(windows)
}

fn base_error(root: &Value) -> Option<String> {
    let base = root.get("base_resp")?;
    let code = json_i64(base, "status_code")?;
    if code == 0 {
        return None;
    }
    let msg = base
        .get("status_msg")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or("接口返回了失败状态");
    Some(msg.to_string())
}

#[allow(clippy::too_many_arguments)]
fn push_minimax_window(
    out: &mut Vec<OfficialQuotaWindow>,
    name: &str,
    general: bool,
    weekly: bool,
    status: i64,
    remaining: Option<f64>,
    end: Option<i64>,
    start: Option<i64>,
    count: Option<f64>,
    total: Option<f64>,
) {
    if status == 3 || remaining.is_none() && status != 2 {
        return;
    }
    let used_percent = if status == 2 {
        100.0
    } else {
        (100.0 - remaining.unwrap_or(0.0)).clamp(0.0, 100.0)
    };
    let span_secs = match (start, end) {
        (Some(start), Some(end)) if end > start => {
            Some(normalize_unix(end) - normalize_unix(start))
        }
        _ if weekly => Some(7 * 24 * 3600),
        _ => None,
    };
    let (kind, label) = minimax_label(name, general, weekly, span_secs);
    let (used_amount, limit_amount) = minimax_count(count, total, remaining, status);
    out.push(OfficialQuotaWindow {
        kind,
        label,
        used_percent: Some(used_percent),
        resets_at: end.and_then(unix_to_rfc3339),
        used_amount,
        limit_amount,
        currency: None,
        ..Default::default()
    });
}

fn minimax_label(
    name: &str,
    general: bool,
    weekly: bool,
    span_secs: Option<i64>,
) -> (String, String) {
    let base_kind = if weekly {
        "days_7".to_string()
    } else if let Some(secs) = span_secs {
        if secs > 0 && secs % 3600 == 0 {
            format!("hours_{}", secs / 3600)
        } else {
            "allowance".to_string()
        }
    } else {
        "allowance".to_string()
    };
    let base_label = if weekly {
        "7 天".to_string()
    } else if let Some(secs) = span_secs {
        if secs > 0 && secs % 86400 == 0 {
            format!("{} 天", secs / 86400)
        } else if secs > 0 && secs % 3600 == 0 {
            format!("{} 小时", secs / 3600)
        } else if secs > 0 {
            format!("{} 分钟", secs / 60)
        } else {
            "额度".to_string()
        }
    } else {
        "额度".to_string()
    };
    if general {
        (base_kind, base_label)
    } else {
        let titled = title_case(name);
        (
            format!("{}_{base_kind}", name.to_ascii_lowercase()),
            format!("{titled} · {base_label}"),
        )
    }
}

fn title_case(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn minimax_count(
    count: Option<f64>,
    total: Option<f64>,
    remaining: Option<f64>,
    status: i64,
) -> (Option<f64>, Option<f64>) {
    let (count, total, remaining) = match (count, total, remaining) {
        (Some(count), Some(total), Some(remaining)) => (count, total, remaining),
        _ => return (None, None),
    };
    if total <= 0.0 || count < 0.0 || count > total {
        return (None, None);
    }
    let as_used = ((total - count) / total * 100.0 - remaining).abs();
    let as_left = (count / total * 100.0 - remaining).abs();
    if as_used.min(as_left) > 1.0 {
        return (None, None);
    }
    let used = if as_used < as_left {
        count
    } else {
        total - count
    };
    let used = if status == 2 { total } else { used };
    (Some(used), Some(total))
}

fn json_f64(value: &Value, name: &str) -> Option<f64> {
    value.get(name).and_then(Value::as_f64)
}

fn json_i64(value: &Value, name: &str) -> Option<i64> {
    value
        .get(name)
        .and_then(|node| node.as_i64().or_else(|| node.as_f64().map(|n| n as i64)))
}

fn normalize_unix(raw: i64) -> i64 {
    if raw > 1_000_000_000_000 {
        raw / 1000
    } else {
        raw
    }
}

fn unix_to_rfc3339(raw: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(normalize_unix(raw), 0).map(|dt| dt.to_rfc3339())
}
