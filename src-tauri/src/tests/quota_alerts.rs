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
            }],
            freshness: crate::domain::OfficialQuotaFreshness::Official,
            captured_at: Some("2026-08-18T12:00:00+00:00".into()),
            error: None,
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
            }],
            freshness: crate::domain::OfficialQuotaFreshness::Official,
            captured_at: Some("2026-08-18T12:00:00+00:00".into()),
            error: None,
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
