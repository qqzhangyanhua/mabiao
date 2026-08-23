//! OpenAI 兼容计费预设。
//!
//! 两个接口，都是 OpenAI 自己那套、被绝大多数中转站原样实现的：
//!
//! - `GET {base}/v1/dashboard/billing/subscription` → `hard_limit_usd`（上限，美元）
//! - `GET {base}/v1/dashboard/billing/usage?start_date&end_date` → `total_usage`（已用，**美分**）
//!
//! 单位不一致是这套接口的固有坑：上限是美元、已用是美分。解析时统一换算成美元。
//!
//! 取数窗口固定取「今天往前 99 天」到「明天」：OpenAI 自己对该接口的跨度上限是
//! 100 天，超了直接报错；多数中转站根本不看日期、直接返回账号累计消耗。
//! 因此这个跨度是「对官方合法、对中转站无害」的交集。用了超过 99 天的老账号，
//! 官方口径下会少算更早的消耗——这是接口本身的限制，不是这里的取舍。

use chrono::{Duration, NaiveDate};
use serde_json::Value;

use crate::domain::OfficialQuotaWindow;

use super::QuotaRequest;

/// 官方接口对 `start_date`/`end_date` 的跨度上限是 100 天。
const USAGE_LOOKBACK_DAYS: i64 = 99;

pub fn urls(base: &str, today: NaiveDate) -> Vec<QuotaRequest> {
    let start = today - Duration::days(USAGE_LOOKBACK_DAYS);
    // 结束日期取明天：这两个接口的 end_date 是开区间，写今天会漏掉今天的消耗。
    let end = today + Duration::days(1);
    vec![
        QuotaRequest {
            url: format!("{base}/v1/dashboard/billing/subscription"),
            // 上限接口是可选的：不少中转站只实现了用量那条，404 不该让整行取不到数，
            // 降级成「只报已用金额」仍然有用。
            required: false,
        },
        QuotaRequest {
            url: format!(
                "{base}/v1/dashboard/billing/usage?start_date={}&end_date={}",
                start.format("%Y-%m-%d"),
                end.format("%Y-%m-%d")
            ),
            // 已用是这个预设的立身之本，拿不到就没有任何可显示的东西。
            required: true,
        },
    ]
}

/// `bodies` 与 `urls` 一一对应：订阅在前、用量在后。订阅那条可以是空串
/// （接口不存在或没打通），此时只报金额、不给百分比。
pub fn parse(bodies: &[&str]) -> Result<Vec<OfficialQuotaWindow>, String> {
    let usage = read_json(bodies.get(1).copied().unwrap_or_default(), "用量接口")?;

    let used = number(&usage, "total_usage").ok_or_else(|| {
        "用量接口里没有 total_usage，这个地址可能不是 OpenAI 兼容计费接口".to_string()
    })? / 100.0;
    // 上限拿不到就只报金额：充值制的站点常常只认已用，不给总额。
    // 订阅接口整个缺席（空串 / 不是 JSON）也走这条，不算取数失败。
    let limit = serde_json::from_str::<Value>(bodies.first().copied().unwrap_or_default())
        .ok()
        .and_then(|subscription| {
            number(&subscription, "hard_limit_usd")
                .or_else(|| number(&subscription, "system_hard_limit_usd"))
        })
        .filter(|value| *value > 0.0);

    Ok(vec![OfficialQuotaWindow {
        kind: "billing_cycle".to_string(),
        label: "总量".to_string(),
        // 超支时钳到 100 而不是丢掉百分比：进度条满格才是用户要看的那个事实。
        used_percent: limit.map(|limit| (used / limit * 100.0).clamp(0.0, 100.0)),
        resets_at: None,
        used_amount: Some(used),
        limit_amount: limit,
        currency: Some("USD".to_string()),
        ..Default::default()
    }])
}

fn read_json(raw: &str, who: &str) -> Result<Value, String> {
    if raw.trim().is_empty() {
        return Err(format!(
            "{who}返回了空响应，请确认 base URL 填的是中转站根地址"
        ));
    }
    serde_json::from_str(raw).map_err(|_| {
        format!("{who}返回的不是合法 JSON（多半是网页或登录页），请确认 base URL 与预设类型")
    })
}

/// 有的站点把数字写成字符串，两种都认。
fn number(value: &Value, key: &str) -> Option<f64> {
    let node = value.get(key)?;
    node.as_f64()
        .or_else(|| node.as_str().and_then(|text| text.trim().parse().ok()))
        .filter(|value: &f64| value.is_finite())
}
