use crate::official_quota::copilot;

const LIVE_SHAPE: &str = r#"{
    "copilot_plan": "individual",
    "quota_reset_date": "2026-09-01",
    "quota_snapshots": {
        "premium_interactions": { "entitlement": 300, "remaining": 75, "percent_remaining": 25 },
        "chat": { "unlimited": true, "entitlement": -1, "remaining": -1 },
        "completions": { "entitlement": 2000, "remaining": 1500 }
    }
}"#;

#[test]
fn copilot_quota_reads_plan() {
    assert_eq!(
        copilot::parse_plan(LIVE_SHAPE).as_deref(),
        Some("Individual")
    );
    assert_eq!(
        copilot::parse_plan(r#"{"copilot_plan":"business"}"#).as_deref(),
        Some("Business")
    );
}

#[test]
fn copilot_quota_inverts_remaining_into_used_percent() {
    let windows = copilot::parse_usage(LIVE_SHAPE).unwrap();
    let kinds: Vec<&str> = windows.iter().map(|w| w.kind.as_str()).collect();
    // chat 是无限额度，不该出现在百分比里。
    assert_eq!(kinds, ["credits", "completions"]);
    assert_eq!(windows[0].label, "高级交互");
    assert_eq!(windows[0].used_percent, Some(75.0));
    // 没有 percent_remaining 时按 remaining / entitlement 算。
    assert_eq!(windows[1].used_percent, Some(25.0));
    assert!(windows[0].resets_at.is_some());
}

#[test]
fn copilot_quota_drops_unlimited_and_zero_entitlement_placeholders() {
    let raw = r#"{"quota_snapshots":{
        "premium_interactions":{"entitlement":0,"remaining":0,"overage_permitted":true},
        "chat":{"entitlement":-1},
        "completions":{"entitlement":10,"remaining":10}
    }}"#;
    let windows = copilot::parse_usage(raw).unwrap();
    // 组织按量计费席位的零额度占位不是「0% 已用」，要丢掉。
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, "completions");
    assert_eq!(windows[0].used_percent, Some(0.0));
}

#[test]
fn copilot_quota_reports_structure_change_instead_of_empty() {
    assert!(copilot::parse_usage("not json").is_err());
    assert!(copilot::parse_usage(r#"{"copilot_plan":"free"}"#).is_err());
    assert!(copilot::parse_usage(r#"{"quota_snapshots":{"chat":{"unlimited":true}}}"#).is_err());
}

#[test]
fn copilot_token_comes_from_editor_config_or_gh_hosts() {
    // 插件配置的键名带 clientId，不稳定，所以扫所有条目。
    let apps = r#"{"github.com:Iv1.abc123":{"user":"me","oauth_token":"gho_editor"}}"#;
    assert_eq!(
        copilot::parse_copilot_config_token(apps).as_deref(),
        Some("gho_editor")
    );
    assert_eq!(
        copilot::parse_copilot_config_token(r#"{"github.com":{}}"#),
        None
    );
    assert_eq!(copilot::parse_copilot_config_token("not json"), None);

    let hosts = "github.com:\n    user: me\n    oauth_token: gho_cli\n    git_protocol: https\n";
    assert_eq!(
        copilot::parse_gh_hosts_token(hosts).as_deref(),
        Some("gho_cli")
    );

    // 企业实例段里的 token 不能被当成 github.com 的。
    let enterprise = "ghe.example.com:\n    oauth_token: gho_enterprise\n";
    assert_eq!(copilot::parse_gh_hosts_token(enterprise), None);

    let both = "ghe.example.com:\n    oauth_token: gho_enterprise\ngithub.com:\n    oauth_token: \"gho_real\"\n";
    assert_eq!(
        copilot::parse_gh_hosts_token(both).as_deref(),
        Some("gho_real")
    );
}
