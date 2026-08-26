use crate::official_quota::devin;

/// 真机响应的形状：额度在 `userStatus.planStatus` 下，给的是剩余百分比，
/// 重置时间是 epoch 秒，且数字可能被包成字符串。
const LIVE_SHAPE: &str = r#"{
    "userStatus": {
        "planStatus": {
            "planInfo": { "planName": "Free", "teamsTier": "TEAMS_TIER_DEVIN_FREE" },
            "dailyQuotaRemainingPercent": 100,
            "weeklyQuotaRemainingPercent": "60",
            "dailyQuotaResetAtUnix": 1787385600,
            "weeklyQuotaResetAtUnix": "1787472000"
        }
    }
}"#;

#[test]
fn devin_quota_reads_plan_name() {
    assert_eq!(devin::parse_plan(LIVE_SHAPE).as_deref(), Some("Free"));
    assert!(devin::parse_plan(r#"{"userStatus":{"planStatus":{}}}"#).is_none());
}

#[test]
fn devin_quota_inverts_remaining_and_reads_stringified_numbers() {
    let windows = devin::parse_user_status(LIVE_SHAPE).unwrap();
    let kinds: Vec<&str> = windows.iter().map(|w| w.kind.as_str()).collect();
    assert_eq!(kinds, ["daily", "weekly"]);
    assert_eq!(windows[0].label, "日额度");
    assert_eq!(windows[0].used_percent, Some(0.0));
    // 字符串包着的 "60" 也要认，取反成已用 40%。
    assert_eq!(windows[1].used_percent, Some(40.0));
    assert_eq!(
        windows[0].resets_at.as_deref(),
        Some("2026-08-22T08:00:00+00:00")
    );
    assert_eq!(
        windows[1].resets_at.as_deref(),
        Some("2026-08-23T08:00:00+00:00")
    );
}

#[test]
fn devin_quota_hides_daily_only_when_a_weekly_window_exists() {
    // 免费档会藏日额度，但周额度还在时才藏得起。
    let both = r#"{"userStatus":{"planStatus":{
        "planInfo":{"hideDailyQuota":true},
        "dailyQuotaRemainingPercent":90,"weeklyQuotaRemainingPercent":50}}}"#;
    let windows = devin::parse_user_status(both).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, "weekly");

    // 周额度也没有时，日额度是唯一有意义的那条，不能一起藏掉。
    let daily_only = r#"{"userStatus":{"planStatus":{
        "planInfo":{"hideDailyQuota":true},"dailyQuotaRemainingPercent":90}}}"#;
    let windows = devin::parse_user_status(daily_only).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, "daily");
    assert_eq!(windows[0].used_percent, Some(10.0));
}

#[test]
fn devin_quota_reports_structure_change_instead_of_empty() {
    assert!(devin::parse_user_status("not json").is_err());
    assert!(devin::parse_user_status(r#"{"ok":true}"#).is_err());
    assert!(devin::parse_user_status(r#"{"userStatus":{"planStatus":{}}}"#).is_err());
}

#[test]
fn devin_api_key_absent_when_state_db_missing() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(devin::read_api_key_at(dir.path()).unwrap(), None);
}
