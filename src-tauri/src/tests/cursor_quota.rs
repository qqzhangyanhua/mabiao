use crate::official_quota;

#[test]
fn cursor_usage_summary_parses_plan_percent() {
    let raw = r#"{
        "billingCycleEnd": "2026-09-02T14:11:55.000Z",
        "membershipType": "pro",
        "individualUsage": { "plan": { "used": 800, "limit": 1000, "totalPercentUsed": 80 } }
    }"#;
    let windows = official_quota::cursor::parse_usage_summary(raw).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, "billing_cycle");
    assert_eq!(windows[0].label, "总量");
    assert_eq!(windows[0].used_percent, Some(80.0));
    assert_eq!(
        official_quota::cursor::parse_membership_type(raw).as_deref(),
        Some("Pro")
    );
}

#[test]
fn cursor_membership_type_maps_known_plans_and_skips_missing() {
    assert_eq!(
        official_quota::cursor::parse_membership_type(r#"{"membershipType":"pro_plus"}"#)
            .as_deref(),
        Some("Pro+")
    );
    assert_eq!(
        official_quota::cursor::parse_membership_type(r#"{"membershipType":"ultra"}"#).as_deref(),
        Some("Ultra")
    );
    assert_eq!(
        official_quota::cursor::parse_membership_type(r#"{"membership_type":"hobby"}"#).as_deref(),
        Some("Free")
    );
    assert!(official_quota::cursor::parse_membership_type(r#"{"ok":true}"#).is_none());
}

#[test]
fn cursor_usage_summary_parses_auto_api_and_on_demand() {
    let raw = r#"{
        "billingCycleEnd": "2026-09-02T14:11:55.000Z",
        "individualUsage": {
            "plan": {
                "enabled": true,
                "used": 940,
                "limit": 1000,
                "autoPercentUsed": 100,
                "apiPercentUsed": 44,
                "totalPercentUsed": 94
            },
            "onDemand": { "enabled": true, "used": 2309, "limit": 5000 }
        }
    }"#;
    let windows = official_quota::cursor::parse_usage_summary(raw).unwrap();
    assert_eq!(windows.len(), 4);
    assert_eq!(windows[0].kind, "billing_cycle");
    assert_eq!(windows[0].used_percent, Some(94.0));
    assert_eq!(windows[1].kind, "auto");
    assert_eq!(windows[1].label, "Auto");
    assert_eq!(windows[1].used_percent, Some(100.0));
    assert_eq!(windows[2].kind, "api");
    assert_eq!(windows[2].used_percent, Some(44.0));
    assert_eq!(windows[3].kind, "on_demand");
    assert_eq!(windows[3].label, "按需");
    assert_eq!(windows[3].used_percent, Some(46.18));
}

#[test]
fn cursor_on_demand_falls_back_to_team_when_individual_has_no_limit() {
    let raw = r#"{
        "billingCycleEnd": "2026-09-02T14:11:55.000Z",
        "individualUsage": {
            "plan": { "totalPercentUsed": 10, "autoPercentUsed": 0, "apiPercentUsed": 20 },
            "onDemand": { "enabled": true, "used": 1840, "limit": null }
        },
        "teamUsage": { "onDemand": { "used": 2500, "limit": 10000 } }
    }"#;
    let windows = official_quota::cursor::parse_usage_summary(raw).unwrap();
    assert_eq!(windows.len(), 4);
    let on_demand = windows
        .iter()
        .find(|window| window.kind == "on_demand")
        .unwrap();
    assert_eq!(on_demand.used_percent, Some(25.0));
}

#[test]
fn cursor_skips_disabled_on_demand_without_limit() {
    let raw = r#"{
        "individualUsage": {
            "plan": { "autoPercentUsed": 12, "apiPercentUsed": 8, "totalPercentUsed": 10 },
            "onDemand": { "enabled": false, "used": 0, "limit": null }
        }
    }"#;
    let windows = official_quota::cursor::parse_usage_summary(raw).unwrap();
    assert_eq!(windows.len(), 3);
    assert!(windows.iter().all(|window| window.kind != "on_demand"));
}

#[test]
fn cursor_usage_summary_keeps_error_on_unknown_shape() {
    assert!(official_quota::cursor::parse_usage_summary(r#"{"ok":true}"#).is_err());
}
