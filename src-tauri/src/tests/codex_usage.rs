use crate::official_quota::codex_usage;

/// 真机响应的形状：两个窗口挂在 `rate_limit` 下，种类由 `limit_window_seconds` 决定。
const LIVE_SHAPE: &str = r#"{
    "plan_type": "plus",
    "rate_limit": {
        "primary_window": { "used_percent": 25.5, "limit_window_seconds": 18000, "reset_at": 1787500000 },
        "secondary_window": { "used_percent": 61, "limit_window_seconds": 604800, "reset_after_seconds": 3600 }
    },
    "rate_limit_reset_credits": 2
}"#;

#[test]
fn codex_usage_reads_plan_type() {
    assert_eq!(
        codex_usage::parse_plan_type(LIVE_SHAPE).as_deref(),
        Some("Plus")
    );
    assert!(codex_usage::parse_plan_type(r#"{"rate_limit":{}}"#).is_none());
}

#[test]
fn codex_usage_reads_both_windows_by_duration() {
    let windows = codex_usage::parse_usage(LIVE_SHAPE, &[], 1_787_000_000).unwrap();
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].kind, "session_5h");
    assert_eq!(windows[0].label, "5 小时");
    assert_eq!(windows[0].used_percent, Some(25.5));
    assert_eq!(windows[1].kind, "weekly");
    assert_eq!(windows[1].label, "7 天");
    // reset_after_seconds 是相对量，要按当前时间换算：1787000000 + 3600。
    assert_eq!(
        windows[1].resets_at.as_deref(),
        Some("2026-08-17T21:53:20+00:00")
    );
}

#[test]
fn codex_usage_keys_off_duration_not_slot_position() {
    // Codex 会把临时只剩一条的周限额挪进 primary 槽，这时不能按位置认成 5 小时。
    let raw =
        r#"{"rate_limit":{"primary_window":{"used_percent":10,"limit_window_seconds":604800}}}"#;
    let windows = codex_usage::parse_usage(raw, &[], 0).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, "weekly");
    assert_eq!(windows[0].label, "7 天");
}

#[test]
fn codex_usage_falls_back_to_response_headers_for_percent() {
    // 响应体缺 used_percent 时，同一份数字在响应头里。
    let raw = r#"{"rate_limit":{"primary_window":{"limit_window_seconds":18000}}}"#;
    let headers = vec![("primary_window".to_string(), 42.0)];
    let windows = codex_usage::parse_usage(raw, &headers, 0).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].used_percent, Some(42.0));

    // 体和头都没有就跳过，不当成 0%。
    assert!(codex_usage::parse_usage(raw, &[], 0).is_err());
}

#[test]
fn codex_usage_names_unknown_durations_instead_of_guessing() {
    let raw =
        r#"{"rate_limit":{"primary_window":{"used_percent":5,"limit_window_seconds":86400}}}"#;
    let windows = codex_usage::parse_usage(raw, &[], 0).unwrap();
    assert_eq!(windows[0].kind, "primary");
    assert_eq!(windows[0].label, "24 小时");
}

#[test]
fn codex_usage_reports_structure_change_instead_of_empty() {
    assert!(codex_usage::parse_usage("not json", &[], 0).is_err());
    assert!(codex_usage::parse_usage(r#"{"ok":true}"#, &[], 0).is_err());
}

#[test]
fn codex_auth_separates_subscription_login_from_api_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.json");

    std::fs::write(
        &path,
        r#"{"tokens":{"access_token":"tok","account_id":"acct-1","refresh_token":"r"}}"#,
    )
    .unwrap();
    let auth = codex_usage::load_auth(&path).unwrap();
    assert_eq!(auth.access_token, "tok");
    assert_eq!(auth.account_id.as_deref(), Some("acct-1"));

    // 纯 API key 的账号按量计费，没有额度百分比——要给一句说得通的话，不是解析错误。
    std::fs::write(&path, r#"{"OPENAI_API_KEY":"sk-xxx"}"#).unwrap();
    assert!(codex_usage::load_auth(&path)
        .unwrap_err()
        .contains("没有订阅额度"));

    std::fs::write(&path, r#"{"tokens":{"refresh_token":"r"}}"#).unwrap();
    assert!(codex_usage::load_auth(&path).is_err());
    assert!(codex_usage::load_auth(&dir.path().join("missing.json")).is_err());
}
