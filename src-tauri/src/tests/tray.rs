use crate::test_support::*;

#[test]
fn local_day_filter_uses_local_midnight_and_end_as_utc_z() {
    let now = Local
        .with_ymd_and_hms(2026, 8, 17, 19, 22, 30)
        .single()
        .expect("fixed local time");
    let filter = crate::tray::local_day_filter(now);
    let from = filter.from.expect("from");
    let to = filter.to.expect("to");
    assert!(from.ends_with('Z'), "{from}");
    assert!(to.ends_with('Z'), "{to}");

    let from_local = chrono::DateTime::parse_from_rfc3339(&from)
        .unwrap()
        .with_timezone(&Local);
    let to_local = chrono::DateTime::parse_from_rfc3339(&to)
        .unwrap()
        .with_timezone(&Local);
    assert_eq!(from_local.date_naive(), now.date_naive());
    assert_eq!(
        from_local.time(),
        NaiveTime::from_hms_milli_opt(0, 0, 0, 0).unwrap()
    );
    assert_eq!(to_local.date_naive(), now.date_naive());
    assert_eq!(
        to_local.time(),
        NaiveTime::from_hms_milli_opt(23, 59, 59, 999).unwrap()
    );
}

#[test]
fn tray_format_title_marks_unpriced() {
    assert_eq!(crate::tray::format_title(Some(1.23), false), "$1.23");
    assert_eq!(crate::tray::format_title(Some(1.23), true), "$1.23*");
    assert_eq!(crate::tray::format_title(None, false), "$0.00");
    assert_eq!(crate::tray::format_title(None, true), "—");
}

#[test]
fn today_filter_overview_matches_in_memory_and_excludes_other_days() {
    let now = Local::now();
    let filter = crate::tray::local_day_filter(now);
    let mut today = rec(
        &local_noon_iso(now.date_naive()),
        Source::Claude,
        "claude",
        "anthropic",
        "/p",
        "s-today",
        100,
    );
    today.native_cost = Some(1.5);
    let mut yesterday = rec(
        &local_noon_iso(now.date_naive() - chrono::Days::new(1)),
        Source::Codex,
        "gpt",
        "official",
        "/p",
        "s-yday",
        200,
    );
    yesterday.native_cost = Some(9.0);

    let records = vec![today, yesterday];
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let prices = PriceTable::default();

    let sql = query::overview(&conn, &filter, &prices).unwrap();
    let mem = aggregate::overview(&records, &filter, &prices);
    assert_eq!(sql.total_tokens, 100);
    assert_eq!(mem.total_tokens, 100);
    assert_eq!(sql.cost, Some(1.5));
    assert_eq!(mem.cost, Some(1.5));
    assert!(!sql.unpriced);
    assert!(!mem.unpriced);
}
