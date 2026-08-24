//! LiteLLM Proxy 预设。
//!
//! 只打一条：`GET {base}/key/info`，**不带** `key` 查询参数。不带参数时该接口
//! 回落成「查调用方自己」，普通 virtual key 即可，不需要 master key。密钥因此
//! 不会出现在设置页「将请求」回显里。
//!
//! 响应有两版形状（官方文档嵌套在 `info` 下，另一份顶层平铺）。解析时先读嵌套，
//! 拿不到回落顶层——这是解析器内部的容错，不是通用 JSON 指针。
//!
//! `spend` 在设了预算周期时只统计当前窗口，因此已用 / 上限是真正的窗口百分比。
//! `budget_reset_at` 是下次重置时刻。窗口类型固定为 `budget_window`：周期变化
//! 只改标签，避免告警去重键跟着用户改预算而变。

use serde_json::Value;

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::parse_resets_at;

use super::QuotaRequest;

const KIND: &str = "budget_window";
const WHO: &str = "key 信息接口";

pub fn urls(base: &str) -> Vec<QuotaRequest> {
    vec![QuotaRequest {
        url: format!("{base}/key/info"),
        required: true,
    }]
}

pub fn parse(bodies: &[&str]) -> Result<Vec<OfficialQuotaWindow>, String> {
    let raw = bodies.first().copied().unwrap_or_default();
    let root = read_json(raw)?;

    let used = number_field(&root, "spend").ok_or_else(|| {
        format!("{WHO}里没有 spend，这个地址可能不是 LiteLLM Proxy 的 key 信息接口")
    })?;

    // 上限拿不到就只报金额：没设 max_budget 的 key 仍然能看见已用。
    // 0 没有可除的分母，和 null 一样走这条降级，不新造窗口形状。
    let limit = number_field(&root, "max_budget").filter(|value| *value > 0.0);
    let label = match text_field(&root, "budget_duration") {
        Some(duration) => format!("预算 {duration}"),
        None => "预算".to_string(),
    };
    let resets_at = field(&root, "budget_reset_at").and_then(parse_resets_at);

    Ok(vec![OfficialQuotaWindow {
        kind: KIND.to_string(),
        label,
        used_percent: limit.map(|limit| (used / limit * 100.0).clamp(0.0, 100.0)),
        resets_at,
        used_amount: Some(used),
        limit_amount: limit,
        currency: Some("USD".to_string()),
        ..Default::default()
    }])
}

fn read_json(raw: &str) -> Result<Value, String> {
    if raw.trim().is_empty() {
        return Err(format!(
            "{WHO}返回了空响应，请确认 base URL 填的是 LiteLLM Proxy 根地址"
        ));
    }
    serde_json::from_str(raw).map_err(|_| {
        format!("{WHO}返回的不是合法 JSON（多半是网页或登录页），请确认 base URL 与预设类型")
    })
}

/// 先读 `info.<name>`，拿不到（缺失或 null）再读顶层。
fn field<'a>(root: &'a Value, name: &str) -> Option<&'a Value> {
    present(root.get("info").and_then(|info| info.get(name))).or_else(|| present(root.get(name)))
}

fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|node| !node.is_null())
}

fn number_field(root: &Value, name: &str) -> Option<f64> {
    let node = field(root, name)?;
    node.as_f64()
        .or_else(|| node.as_str().and_then(|text| text.trim().parse().ok()))
        .filter(|value: &f64| value.is_finite())
}

fn text_field<'a>(root: &'a Value, name: &str) -> Option<&'a str> {
    field(root, name)?
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}
