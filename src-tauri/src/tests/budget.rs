use crate::test_support::*;

#[test]
fn local_month_filter_starts_at_first_of_month_local_midnight() {
    let now = Local
        .with_ymd_and_hms(2026, 8, 17, 19, 22, 30)
        .single()
        .expect("fixed local time");
    let filter = budget::local_month_filter(now);
    let from = filter.from.expect("from");
    let to = filter.to.expect("to");
    assert!(from.ends_with('Z'), "{from}");
    assert!(to.ends_with('Z'), "{to}");

    let from_local = chrono::DateTime::parse_from_rfc3339(&from)
        .unwrap()
        .with_timezone(&Local);
    assert_eq!(
        from_local.date_naive(),
        chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap()
    );
    assert_eq!(
        from_local.time(),
        NaiveTime::from_hms_milli_opt(0, 0, 0, 0).unwrap()
    );

    let to_local = chrono::DateTime::parse_from_rfc3339(&to)
        .unwrap()
        .with_timezone(&Local);
    assert_eq!(to_local.date_naive(), now.date_naive());
    assert_eq!(
        to_local.time().num_seconds_from_midnight(),
        19 * 3600 + 22 * 60 + 30
    );
}

#[test]
fn budget_status_scopes_cost_to_the_current_calendar_month() {
    let now = Local::now();
    let conn = store::open_memory().unwrap();

    let mut this_month = rec(
        &now.with_timezone(&Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/proj",
        "s1",
        1000,
    );
    this_month.native_cost = Some(30.0);

    // 40 天前无论如何都落在上个自然月之前，不应计入本月预算。
    let mut last_month = rec(
        &(now - chrono::Duration::days(40))
            .with_timezone(&Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj",
        "s2",
        2000,
    );
    last_month.native_cost = Some(999.0);

    store::insert_records(&conn, &[this_month, last_month]).unwrap();

    let config = BudgetConfig {
        monthly_usd: Some(100.0),
    };
    let dto = budget::status(&conn, &PriceTable::default(), &config, now).unwrap();

    assert_eq!(dto.month, now.format("%Y-%m").to_string());
    assert_eq!(dto.days_elapsed, now.day() as i64);
    assert!(dto.days_in_month >= 28 && dto.days_in_month <= 31);
    assert!((dto.month_to_date_cost - 30.0).abs() < 1e-9);
    assert!(!dto.unpriced);
    assert_eq!(dto.monthly_budget, Some(100.0));
    assert!((dto.percent_used.unwrap() - 30.0).abs() < 1e-9);
    // 预测费用按日均线性外推到月末，应不小于已产生的费用。
    assert!(dto.projected_month_cost.unwrap() >= dto.month_to_date_cost);
    assert_eq!(dto.thresholds, vec![50, 80, 100]);
}

#[test]
fn budget_status_without_a_configured_budget_has_no_percentages() {
    let now = Local::now();
    let conn = store::open_memory().unwrap();
    let config = BudgetConfig { monthly_usd: None };
    let dto = budget::status(&conn, &PriceTable::default(), &config, now).unwrap();
    assert_eq!(dto.monthly_budget, None);
    assert_eq!(dto.percent_used, None);
    assert_eq!(dto.percent_projected, None);
}

#[test]
fn thresholds_to_notify_only_returns_reached_and_unreported_ones() {
    assert_eq!(budget::thresholds_to_notify(45.0, &[]), Vec::<u32>::new());
    assert_eq!(budget::thresholds_to_notify(55.0, &[]), vec![50]);
    assert_eq!(budget::thresholds_to_notify(85.0, &[50]), vec![80]);
    assert_eq!(budget::thresholds_to_notify(120.0, &[50, 80]), vec![100]);
    assert_eq!(
        budget::thresholds_to_notify(120.0, &[50, 80, 100]),
        Vec::<u32>::new()
    );
}

#[test]
fn budget_config_round_trips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budget.json");
    assert_eq!(budget::load_config(&path), BudgetConfig::default());

    let config = BudgetConfig {
        monthly_usd: Some(42.5),
    };
    budget::save_config(&path, &config).unwrap();
    assert_eq!(budget::load_config(&path), config);
}

#[test]
fn should_check_budget_skips_missing_or_non_positive_limits() {
    assert!(!budget::should_check_budget(&BudgetConfig {
        monthly_usd: None
    }));
    assert!(!budget::should_check_budget(&BudgetConfig {
        monthly_usd: Some(0.0),
    }));
    assert!(!budget::should_check_budget(&BudgetConfig {
        monthly_usd: Some(-10.0),
    }));
    assert!(budget::should_check_budget(&BudgetConfig {
        monthly_usd: Some(20.0),
    }));
}

#[test]
fn prepare_notifications_emits_each_threshold_once_in_the_same_month() {
    let empty = budget::NotifyState::default();
    let (after_50, crossed) = budget::prepare_notifications(empty, "2026-08", 50.0);
    assert_eq!(crossed, vec![50]);
    assert_eq!(after_50.month, "2026-08");
    assert_eq!(after_50.notified, vec![50]);

    let (after_repeat, crossed) = budget::prepare_notifications(after_50.clone(), "2026-08", 55.0);
    assert!(crossed.is_empty());
    assert_eq!(after_repeat, after_50);

    let (after_80, crossed) = budget::prepare_notifications(after_50, "2026-08", 80.0);
    assert_eq!(crossed, vec![80]);
    assert_eq!(after_80.notified, vec![50, 80]);

    let (after_100, crossed) = budget::prepare_notifications(after_80, "2026-08", 120.0);
    assert_eq!(crossed, vec![100]);
    assert_eq!(after_100.notified, vec![50, 80, 100]);

    let (after_all, crossed) = budget::prepare_notifications(after_100.clone(), "2026-08", 150.0);
    assert!(crossed.is_empty());
    assert_eq!(after_all, after_100);
}

#[test]
fn prepare_notifications_resets_notified_thresholds_on_month_change() {
    let last_month = budget::NotifyState {
        month: "2026-07".into(),
        notified: vec![50, 80, 100],
    };
    let (next, crossed) = budget::prepare_notifications(last_month, "2026-08", 52.0);
    assert_eq!(crossed, vec![50]);
    assert_eq!(next.month, "2026-08");
    assert_eq!(next.notified, vec![50]);
}

#[test]
fn notify_state_round_trips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budget-notify.json");
    assert_eq!(
        budget::load_notify_state(&path),
        budget::NotifyState::default()
    );

    let state = budget::NotifyState {
        month: "2026-08".into(),
        notified: vec![50, 80],
    };
    budget::save_notify_state(&path, &state).unwrap();
    assert_eq!(budget::load_notify_state(&path), state);
}
