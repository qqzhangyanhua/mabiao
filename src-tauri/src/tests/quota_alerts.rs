use crate::official_quota;

#[test]
fn quota_alerts_dedupe_by_reset_and_skip_stale() {
    let official = crate::domain::OfficialQuotaDto {
        rows: vec![crate::domain::OfficialQuotaRow {
            provider: "claude".into(),
            application: "Claude".into(),
            windows: vec![crate::domain::OfficialQuotaWindow {
                kind: "session_5h".into(),
                label: "5 小时".into(),
                used_percent: Some(82.0),
                resets_at: Some("2026-08-18T15:00:00+00:00".into()),
                ..Default::default()
            }],
            freshness: crate::domain::OfficialQuotaFreshness::Official,
            captured_at: Some("2026-08-18T12:00:00+00:00".into()),
            error: None,
            todo: None,
            plan: None,
        }],
        alerts_enabled: true,
        stale_after_minutes: 10,
        undetected: Vec::new(),
        hidden_providers: Vec::new(),
    };
    let (after, alerts) = official_quota::notify::prepare_notifications(
        official_quota::notify::NotifyState::default(),
        &official,
    );
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].threshold, 80);
    let (_, again) = official_quota::notify::prepare_notifications(after.clone(), &official);
    assert!(again.is_empty());

    let mut stale = official.clone();
    stale.rows[0].freshness = crate::domain::OfficialQuotaFreshness::Stale;
    stale.rows[0].windows[0].used_percent = Some(100.0);
    let (_, stale_alerts) = official_quota::notify::prepare_notifications(after, &stale);
    assert!(stale_alerts.is_empty());
}

#[test]
fn quota_alerts_reset_when_resets_at_changes() {
    let first = crate::domain::OfficialQuotaDto {
        rows: vec![crate::domain::OfficialQuotaRow {
            provider: "claude".into(),
            application: "Claude".into(),
            windows: vec![crate::domain::OfficialQuotaWindow {
                kind: "weekly".into(),
                label: "7 天".into(),
                used_percent: Some(100.0),
                resets_at: Some("2026-08-20T00:00:00+00:00".into()),
                ..Default::default()
            }],
            freshness: crate::domain::OfficialQuotaFreshness::Official,
            captured_at: Some("2026-08-18T12:00:00+00:00".into()),
            error: None,
            todo: None,
            plan: None,
        }],
        alerts_enabled: true,
        stale_after_minutes: 10,
        undetected: Vec::new(),
        hidden_providers: Vec::new(),
    };
    let (state, alerts) = official_quota::notify::prepare_notifications(
        official_quota::notify::NotifyState::default(),
        &first,
    );
    assert_eq!(alerts[0].threshold, 100);
    let mut next = first;
    next.rows[0].windows[0].resets_at = Some("2026-08-27T00:00:00+00:00".into());
    next.rows[0].windows[0].used_percent = Some(81.0);
    let (_, alerts) = official_quota::notify::prepare_notifications(state, &next);
    assert_eq!(alerts[0].threshold, 80);
}

fn weekly_row(used: f64, resets_at: &str) -> crate::domain::OfficialQuotaDto {
    crate::domain::OfficialQuotaDto {
        rows: vec![crate::domain::OfficialQuotaRow {
            provider: "claude".into(),
            application: "Claude".into(),
            windows: vec![crate::domain::OfficialQuotaWindow {
                kind: "weekly".into(),
                label: "7 天".into(),
                used_percent: Some(used),
                resets_at: Some(resets_at.into()),
                ..Default::default()
            }],
            freshness: crate::domain::OfficialQuotaFreshness::Official,
            captured_at: Some("2026-08-18T12:00:00+00:00".into()),
            error: None,
            todo: None,
            plan: None,
        }],
        alerts_enabled: true,
        stale_after_minutes: 10,
        undetected: Vec::new(),
        hidden_providers: Vec::new(),
    }
}

#[test]
fn custom_thresholds_replace_hardcoded_80_100() {
    let dto = weekly_row(55.0, "2026-08-27T00:00:00+00:00");
    let config = crate::domain::OfficialQuotaConfig {
        alert_thresholds: vec![50, 90],
        reset_reminder_hours: 0,
        ..Default::default()
    };
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-18T12:00:00+00:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let (state, alerts) = official_quota::notify::prepare_notifications_with(
        official_quota::notify::NotifyState::default(),
        &dto,
        &config,
        now,
    );
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].threshold, 50);
    assert_eq!(
        alerts[0].kind,
        official_quota::notify::QuotaAlertKind::Threshold
    );
    let (_, again) = official_quota::notify::prepare_notifications_with(state, &dto, &config, now);
    assert!(again.is_empty());
}

#[test]
fn reset_soon_reminder_fires_once_per_window() {
    let dto = weekly_row(20.0, "2026-08-18T20:00:00+00:00");
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-18T12:00:00+00:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let config = crate::domain::OfficialQuotaConfig::default();
    let (state, alerts) = official_quota::notify::prepare_notifications_with(
        official_quota::notify::NotifyState::default(),
        &dto,
        &config,
        now,
    );
    assert_eq!(alerts.len(), 1);
    assert_eq!(
        alerts[0].kind,
        official_quota::notify::QuotaAlertKind::ResetSoon
    );
    assert_eq!(alerts[0].used_percent, 20.0);
    let (_, again) = official_quota::notify::prepare_notifications_with(state, &dto, &config, now);
    assert!(again.is_empty());
}

#[test]
fn reset_soon_skips_short_windows_and_high_usage() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-18T12:00:00+00:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let config = crate::domain::OfficialQuotaConfig {
        alert_thresholds: vec![99],
        ..Default::default()
    };
    let mut five_hour = weekly_row(20.0, "2026-08-18T16:00:00+00:00");
    five_hour.rows[0].windows[0].kind = "session_5h".into();
    five_hour.rows[0].windows[0].label = "5 小时".into();
    let (_, short) = official_quota::notify::prepare_notifications_with(
        official_quota::notify::NotifyState::default(),
        &five_hour,
        &config,
        now,
    );
    assert!(short.is_empty());

    let high = weekly_row(80.0, "2026-08-18T16:00:00+00:00");
    let (_, skipped) = official_quota::notify::prepare_notifications_with(
        official_quota::notify::NotifyState::default(),
        &high,
        &config,
        now,
    );
    assert!(skipped.is_empty());

    let off = crate::domain::OfficialQuotaConfig {
        alert_thresholds: vec![99],
        reset_reminder_hours: 0,
        ..Default::default()
    };
    let unused = weekly_row(10.0, "2026-08-18T16:00:00+00:00");
    let (_, silent) = official_quota::notify::prepare_notifications_with(
        official_quota::notify::NotifyState::default(),
        &unused,
        &off,
        now,
    );
    assert!(silent.is_empty());
}

#[test]
fn old_config_json_keeps_default_alert_fields() {
    let parsed: crate::domain::OfficialQuotaConfig =
        serde_json::from_str(r#"{"alerts_enabled":true,"hidden_providers":["claude"]}"#).unwrap();
    assert_eq!(parsed.alert_thresholds, vec![80, 100]);
    assert_eq!(parsed.reset_reminder_hours, 12);
    assert_eq!(parsed.reset_reminder_max_used_percent, 50);
    assert_eq!(parsed.hidden_providers, vec!["claude".to_string()]);
}

#[test]
fn normalize_alert_thresholds_dedupes_and_falls_back() {
    assert_eq!(
        crate::domain::normalize_alert_thresholds(&[90, 50, 50, 0, 101]),
        vec![50, 90]
    );
    assert_eq!(
        crate::domain::normalize_alert_thresholds(&[]),
        vec![80, 100]
    );
}

#[test]
fn save_config_sanitizes_thresholds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("official_quota.json");
    official_quota::save_config(
        &path,
        &crate::domain::OfficialQuotaConfig {
            alert_thresholds: vec![0, 150, 70, 70],
            reset_reminder_hours: 200,
            reset_reminder_max_used_percent: 140,
            ..Default::default()
        },
    )
    .unwrap();
    let loaded = official_quota::load_config(&path);
    assert_eq!(loaded.alert_thresholds, vec![70]);
    assert_eq!(loaded.reset_reminder_hours, 168);
    assert_eq!(loaded.reset_reminder_max_used_percent, 100);
}
