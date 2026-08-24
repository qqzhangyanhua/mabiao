//! 自定义提供商：加载 / 构造地址 / 解析响应、DTO 合流，以及托盘与告警。
//!
//! 全部走 fixture 字符串与 tempfile，不联网、不读真实用户目录。
//!
//! 按接缝分文件，共用这里的 fixture 与构造器。

mod panel_commands;
mod parsing;
mod preview_and_test;
mod rows;
mod tray_and_alerts;

use crate::official_quota::custom::store::CustomQuotaProvider;
use crate::official_quota::custom::CustomQuotaPreset;

const SUBSCRIPTION: &str = r#"{
    "object": "billing_subscription",
    "hard_limit_usd": 50.0,
    "system_hard_limit_usd": 100.0,
    "access_until": 0
}"#;
/// `total_usage` 是**美分**：这套接口上限用美元、已用用美分，是它的固有坑。
const USAGE: &str = r#"{"object":"list","total_usage":1900.0,"daily_costs":[]}"#;

fn provider(id: &str, name: &str) -> CustomQuotaProvider {
    CustomQuotaProvider {
        id: id.to_string(),
        name: name.to_string(),
        preset: CustomQuotaPreset::OpenAiCompatible,
        base_url: "https://relay.example.com".to_string(),
        enabled: true,
    }
}

fn today() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 8, 23).unwrap()
}
