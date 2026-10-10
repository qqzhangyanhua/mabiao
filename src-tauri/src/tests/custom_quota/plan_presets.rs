use super::today;
use crate::official_quota::custom::{self, CustomQuotaPreset};

#[test]
fn kimi_code_urls_add_coding_v1_on_official_hosts() {
    let requests = custom::request_urls(
        CustomQuotaPreset::KimiCode,
        "https://api.kimi.com/coding",
        today(),
    )
    .unwrap();
    assert_eq!(requests[0].url, "https://api.kimi.com/coding/v1/usages");
    assert!(requests[0].required);

    let stripped = custom::request_urls(
        CustomQuotaPreset::KimiCode,
        "https://api.kimi.com/coding/v1",
        today(),
    )
    .unwrap();
    assert_eq!(stripped[0].url, "https://api.kimi.com/coding/v1/usages");

    let root =
        custom::request_urls(CustomQuotaPreset::KimiCode, "https://api.kimi.ai", today()).unwrap();
    assert_eq!(root[0].url, "https://api.kimi.ai/coding/v1/usages");
}

#[test]
fn minimax_and_zhipu_urls_are_origin_absolute() {
    let minimax = custom::request_urls(
        CustomQuotaPreset::MiniMaxCoding,
        "https://api.minimaxi.com/v1",
        today(),
    )
    .unwrap();
    assert_eq!(
        minimax[0].url,
        "https://api.minimaxi.com/v1/token_plan/remains"
    );

    let zhipu = custom::request_urls(
        CustomQuotaPreset::ZhipuCoding,
        "https://open.bigmodel.cn/coding",
        today(),
    )
    .unwrap();
    assert_eq!(
        zhipu[0].url,
        "https://open.bigmodel.cn/api/monitor/usage/quota/limit"
    );
    let zai =
        custom::request_urls(CustomQuotaPreset::ZhipuCoding, "https://api.z.ai", today()).unwrap();
    assert_eq!(zai[0].url, "https://api.z.ai/api/monitor/usage/quota/limit");
}

#[test]
fn zhipu_authorization_is_bare_key() {
    assert_eq!(
        custom::authorization_value(CustomQuotaPreset::ZhipuCoding, "sk-glm"),
        "sk-glm"
    );
    assert_eq!(
        custom::authorization_value(CustomQuotaPreset::KimiCode, "sk-kimi"),
        "Bearer sk-kimi"
    );
    assert_eq!(
        custom::authorization_value(CustomQuotaPreset::MiniMaxCoding, "sk-cp-1"),
        "Bearer sk-cp-1"
    );
    assert_eq!(
        custom::authorization_value(CustomQuotaPreset::CommandCode, "cc-key"),
        "Bearer cc-key"
    );
    assert!(CustomQuotaPreset::ZhipuCoding.implemented());
    assert!(CustomQuotaPreset::CommandCode.implemented());
    assert!(!CustomQuotaPreset::ZhipuCoding.bearer_authorization());
    assert!(CustomQuotaPreset::CommandCode.bearer_authorization());
}

#[test]
fn kimi_code_reads_string_limits_and_weekly_usage() {
    let body = r#"{
        "usage":{"limit":"100","used":"12","resetTime":"2026-09-30T05:24:18.440Z"},
        "limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},
                   "detail":{"limit":"100","remaining":"88","resetTime":"2026-09-30T05:24:18.440Z"}}]
    }"#;
    let windows = custom::parse_quota(CustomQuotaPreset::KimiCode, &[body]).unwrap();
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].kind, "minutes_300");
    assert_eq!(windows[0].label, "300 分钟");
    assert!((windows[0].used_percent.unwrap() - 12.0).abs() < 1e-9);
    assert_eq!(windows[0].used_amount, Some(12.0));
    assert_eq!(windows[0].limit_amount, Some(100.0));
    assert_eq!(windows[1].kind, "days_7");
    assert_eq!(windows[1].label, "7 天");
    assert_eq!(windows[1].used_amount, Some(12.0));
}

#[test]
fn minimax_coding_reads_remaining_percent_and_skips_unlimited() {
    let body = r#"{
        "model_remains":[
          {"model_name":"general","start_time":1770000000,"end_time":1770018000,
           "current_interval_remaining_percent":80,"current_interval_status":1,
           "current_interval_total_count":0,
           "weekly_start_time":1769900000,"weekly_end_time":1770504800,
           "current_weekly_remaining_percent":40,"current_weekly_status":1,
           "current_weekly_total_count":0},
          {"model_name":"video","current_interval_status":3,"current_weekly_status":3,
           "current_interval_total_count":0,"current_weekly_total_count":0}
        ],
        "base_resp":{"status_code":0,"status_msg":"success"}
    }"#;
    let windows = custom::parse_quota(CustomQuotaPreset::MiniMaxCoding, &[body]).unwrap();
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].kind, "hours_5");
    assert_eq!(windows[0].label, "5 小时");
    assert!((windows[0].used_percent.unwrap() - 20.0).abs() < 1e-9);
    assert_eq!(windows[1].kind, "days_7");
    assert!((windows[1].used_percent.unwrap() - 60.0).abs() < 1e-9);
}

#[test]
fn zhipu_coding_reads_hour_week_and_mcp_windows() {
    let body = r#"{
        "success":true,
        "data":{"level":"pro","limits":[
          {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":12,"nextResetTime":1770000000000},
          {"type":"TOKENS_LIMIT","unit":6,"number":1,"percentage":40,"nextResetTime":1770604800000},
          {"type":"TIME_LIMIT","unit":5,"number":1,"percentage":3,"nextResetTime":1772000000000}
        ]}
    }"#;
    let windows = custom::parse_quota(CustomQuotaPreset::ZhipuCoding, &[body]).unwrap();
    assert_eq!(windows.len(), 3);
    assert_eq!(windows[0].kind, "hours_5");
    assert_eq!(windows[0].label, "5 小时");
    assert_eq!(windows[0].used_percent, Some(12.0));
    assert_eq!(windows[1].kind, "days_7");
    assert_eq!(windows[1].used_percent, Some(40.0));
    assert_eq!(windows[2].kind, "mcp_month");
    assert_eq!(windows[2].label, "MCP · 月");
}

#[test]
fn plan_presets_reject_empty_or_failed_bodies() {
    assert!(custom::parse_quota(CustomQuotaPreset::KimiCode, &[""]).is_err());
    assert!(custom::parse_quota(
        CustomQuotaPreset::MiniMaxCoding,
        &[r#"{"base_resp":{"status_code":7,"status_msg":"no plan"}}"#]
    )
    .is_err());
    assert!(custom::parse_quota(
        CustomQuotaPreset::ZhipuCoding,
        &[r#"{"success":false,"msg":"no plan"}"#]
    )
    .is_err());
    assert!(custom::parse_quota(
        CustomQuotaPreset::CommandCode,
        &[r#"{"credits":{"monthlyCredits":70},"windowLimits":null}"#]
    )
    .is_err());
}

#[test]
fn plan_presets_never_accept_remote_http() {
    for preset in [
        CustomQuotaPreset::KimiCode,
        CustomQuotaPreset::MiniMaxCoding,
        CustomQuotaPreset::ZhipuCoding,
        CustomQuotaPreset::CommandCode,
    ] {
        let error = custom::request_urls(preset, "http://evil.example.com", today()).unwrap_err();
        assert!(error.contains("https://"), "{preset:?}: {error}");
    }
}

#[test]
fn command_code_urls_use_origin_absolute_credits() {
    let root = custom::request_urls(
        CustomQuotaPreset::CommandCode,
        "https://api.commandcode.ai",
        today(),
    )
    .unwrap();
    assert_eq!(
        root[0].url,
        "https://api.commandcode.ai/alpha/billing/credits"
    );
    assert!(root[0].required);

    let provider = custom::request_urls(
        CustomQuotaPreset::CommandCode,
        "https://api.commandcode.ai/provider/v1",
        today(),
    )
    .unwrap();
    assert_eq!(
        provider[0].url,
        "https://api.commandcode.ai/alpha/billing/credits"
    );
}

#[test]
fn command_code_reads_five_hour_and_weekly_windows() {
    // magpie planquota_test.go cmdCreditsReply：resetAt 是毫秒。
    let body = r#"{
        "credits":{"planId":"individual-goat-monthly","monthlyCredits":41.2,"purchasedCredits":5,"freeCredits":0},
        "windowLimits":{"limited":true,
          "fiveHour":{"used":3,"cap":10,"resetAt":1790000000000},
          "weekly":{"used":"12","cap":"40","resetAt":1790400000000}}
    }"#;
    let windows = custom::parse_quota(CustomQuotaPreset::CommandCode, &[body]).unwrap();
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].kind, "hours_5");
    assert_eq!(windows[0].label, "5 小时");
    assert!((windows[0].used_percent.unwrap() - 30.0).abs() < 1e-9);
    assert_eq!(windows[0].used_amount, Some(3.0));
    assert_eq!(windows[0].limit_amount, Some(10.0));
    assert_eq!(
        windows[0].resets_at.as_deref(),
        Some("2026-09-21T14:13:20+00:00")
    );
    assert_eq!(windows[1].kind, "days_7");
    assert_eq!(windows[1].label, "7 天");
    assert!((windows[1].used_percent.unwrap() - 30.0).abs() < 1e-9);
    assert_eq!(windows[1].used_amount, Some(12.0));
    assert_eq!(windows[1].limit_amount, Some(40.0));
}
