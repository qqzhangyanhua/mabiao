use crate::official_quota::claude_usage;

/// 真机响应的形状：顶层三个固定窗口用 `utilization`，按模型的周窗口在 `limits[]`
/// 里用 `percent`，老的 `seven_day_<model>` 顶层键已经返回 null。
const LIVE_SHAPE: &str = r#"{
    "five_hour":  { "utilization": 23.5, "resets_at": "2026-08-22T04:00:00Z" },
    "seven_day":  { "utilization": 41.2, "resets_at": 1787500000 },
    "seven_day_sonnet": { "utilization": 12, "resets_at": "2026-08-27T00:00:00Z" },
    "seven_day_opus": null,
    "limits": [
        { "kind": "weekly_scoped", "percent": 8.5, "resets_at": "2026-08-27T00:00:00Z",
          "scope": { "model": { "display_name": "Fable" } } },
        { "kind": "five_hour", "percent": 99 }
    ],
    "extra_usage": { "is_enabled": false }
}"#;

#[test]
fn claude_usage_maps_fixed_windows_and_scoped_weekly() {
    let windows = claude_usage::parse_usage(LIVE_SHAPE).unwrap();
    let kinds: Vec<&str> = windows.iter().map(|w| w.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "session_5h",
            "weekly",
            "weekly_sonnet",
            // 按模型拆的周窗口，名字跟着接口走，不写死模型清单。
            "weekly_fable"
        ]
    );
    assert_eq!(windows[0].used_percent, Some(23.5));
    assert_eq!(windows[0].label, "5 小时");
    assert_eq!(windows[3].used_percent, Some(8.5));
    assert_eq!(windows[3].label, "7 天 Fable");
    // resets_at 既可能是 ISO 字符串也可能是 epoch 秒。
    assert!(windows[0].resets_at.is_some());
    assert!(windows[1].resets_at.is_some());
}

#[test]
fn claude_usage_skips_scoped_entries_that_are_not_weekly() {
    // `limits[]` 里混着别的 kind，只认 weekly_scoped，且必须有模型名。
    let raw = r#"{"five_hour":{"utilization":1},"limits":[
        {"kind":"five_hour","percent":50,"scope":{"model":{"display_name":"X"}}},
        {"kind":"weekly_scoped","percent":50},
        {"kind":"weekly_scoped","percent":50,"scope":{"model":{"display_name":"  "}}}
    ]}"#;
    let windows = claude_usage::parse_usage(raw).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, "session_5h");
}

#[test]
fn claude_usage_reports_structure_change_instead_of_empty() {
    assert!(claude_usage::parse_usage("not json").is_err());
    assert!(claude_usage::parse_usage(r#"{"ok":true}"#).is_err());
    // 有窗口但百分比不合法（泄漏的 epoch）时不能当成 0%。
    assert!(claude_usage::parse_usage(r#"{"five_hour":{"utilization":1776950400}}"#).is_err());
}

#[test]
fn claude_credentials_require_usage_scope_and_live_token() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".credentials.json");
    let write =
        |oauth: &str| std::fs::write(&path, format!(r#"{{"claudeAiOauth":{oauth}}}"#)).unwrap();

    write(r#"{"accessToken":"tok","scopes":["user:profile","user:inference"],"expiresAt":0}"#);
    assert_eq!(claude_usage::load_access_token(&path).unwrap(), "tok");

    // `claude setup-token` 生成的纯推理 token 没有 user:profile，接口会拒，先本地筛掉。
    write(r#"{"accessToken":"tok","scopes":["user:inference"]}"#);
    assert!(claude_usage::load_access_token(&path)
        .unwrap_err()
        .contains("重新登录"));

    write(r#"{"accessToken":"tok","expiresAt":1}"#);
    assert!(claude_usage::load_access_token(&path)
        .unwrap_err()
        .contains("已过期"));

    write(r#"{"scopes":["user:profile"]}"#);
    assert!(claude_usage::load_access_token(&path).is_err());

    assert!(claude_usage::load_access_token(&dir.path().join("missing.json")).is_err());
}

#[test]
fn claude_credentials_treat_zero_expiry_as_unknown() {
    let now = 1_800_000_000_000;
    let oauth = |raw: &str| serde_json::from_str::<serde_json::Value>(raw).unwrap();
    // 第三方代理会把 expiresAt 写成 0，这时别自己判过期，交给接口。
    assert!(!claude_usage::is_expired(&oauth(r#"{"expiresAt":0}"#), now));
    assert!(!claude_usage::is_expired(&oauth("{}"), now));
    assert!(claude_usage::is_expired(&oauth(r#"{"expiresAt":1}"#), now));
    assert!(!claude_usage::is_expired(
        &oauth(r#"{"expiresAt":1900000000000}"#),
        now
    ));
}

#[test]
fn credentials_prefer_claude_config_dir_over_default_home() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let custom = home.join("custom-claude");
    std::fs::create_dir_all(&custom).unwrap();
    std::fs::write(custom.join(".credentials.json"), "{}").unwrap();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join(".claude/.credentials.json"), "{}").unwrap();
    let overrides =
        crate::ingest::PathOverrides::from([("CLAUDE_CONFIG_DIR", vec![custom.clone()])]);
    // 修前写死 ~/.claude；修后自定义根优先。
    assert_eq!(
        claude_usage::first_existing_credentials(home, &overrides),
        custom.join(".credentials.json")
    );
}

#[test]
fn credentials_fall_back_to_default_home_when_config_dir_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let custom = home.join("custom-claude");
    std::fs::create_dir_all(&custom).unwrap();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join(".claude/.credentials.json"), "{}").unwrap();
    let overrides = crate::ingest::PathOverrides::from([("CLAUDE_CONFIG_DIR", vec![custom])]);
    assert_eq!(
        claude_usage::first_existing_credentials(home, &overrides),
        home.join(".claude/.credentials.json")
    );
}

#[test]
fn credentials_also_probe_xdg_config_claude() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let xdg = home.join(".config/claude");
    std::fs::create_dir_all(&xdg).unwrap();
    std::fs::write(xdg.join(".credentials.json"), "{}").unwrap();
    let path = claude_usage::first_existing_credentials(home, &crate::ingest::PathOverrides::new());
    assert_eq!(path, xdg.join(".credentials.json"));
}
