use crate::domain::{
    CursorUsageEvent, ReportDto, ReportInsight, ReportPeriod, ReportPeriodKind, ReportTopSessionBy,
};
use crate::test_support::*;
use chrono::{DateTime, Local, NaiveDate};

fn day(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
}

fn now_on(date: NaiveDate) -> DateTime<Local> {
    let naive = date.and_hms_opt(15, 0, 0).expect("valid time");
    naive
        .and_local_timezone(Local)
        .earliest()
        .or_else(|| naive.and_local_timezone(Local).latest())
        .expect("local time")
}

fn week(offset: u32) -> ReportPeriod {
    ReportPeriod {
        kind: ReportPeriodKind::Week,
        offset,
    }
}

fn month(offset: u32) -> ReportPeriod {
    ReportPeriod {
        kind: ReportPeriodKind::Month,
        offset,
    }
}

fn usage(
    date: NaiveDate,
    hour: u32,
    min: u32,
    sec: u32,
    session_id: &str,
    total: i64,
) -> crate::domain::UsageRecord {
    rec(
        &local_time_iso(date, hour, min, sec),
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/proj/a",
        session_id,
        total,
    )
}

/// 固定「现在」为 2026-08-19（周三）：当前周从 8/17 一开始，最近结束的完整周是 8/10–8/16。
fn now() -> DateTime<Local> {
    now_on(day(2026, 8, 19))
}

fn build_with(
    records: &[crate::domain::UsageRecord],
    period: ReportPeriod,
) -> crate::domain::ReportDto {
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, records).unwrap();
    crate::report::build(&conn, &PriceTable::default(), period, now()).unwrap()
}

fn night_share(dto: &ReportDto) -> (i64, i64, i64) {
    for insight in &dto.insights {
        if let ReportInsight::NightShare {
            night_tokens,
            total_tokens,
            pct,
        } = insight
        {
            return (*night_tokens, *total_tokens, *pct);
        }
    }
    panic!("expected night_share, got {:?}", dto.insights);
}

fn peak_hours(dto: &ReportDto) -> (u8, u8) {
    for insight in &dto.insights {
        if let ReportInsight::PeakHours {
            start_hour,
            end_hour,
        } = insight
        {
            return (*start_hour, *end_hour);
        }
    }
    panic!("expected peak_hours, got {:?}", dto.insights);
}

fn busiest_day(dto: &ReportDto) -> u8 {
    for insight in &dto.insights {
        if let ReportInsight::BusiestDay { weekday } = insight {
            return *weekday;
        }
    }
    panic!("expected busiest_day, got {:?}", dto.insights);
}

fn day_tokens(dto: &ReportDto) -> Vec<(&str, i64)> {
    dto.days
        .iter()
        .map(|point| (point.date.as_str(), point.total_tokens))
        .collect()
}

#[test]
fn offset_zero_is_last_complete_local_week() {
    let dto = build_with(&[], week(0));
    assert_eq!(dto.period_kind, ReportPeriodKind::Week);
    assert_eq!(dto.offset, 0);
    assert_eq!(dto.start_date, "2026-08-10");
    assert_eq!(dto.end_date, "2026-08-16");
    assert!(!dto.has_data);
    assert!(dto.insights.is_empty());
}

#[test]
fn offset_one_is_the_week_before_last_complete() {
    let dto = build_with(&[], week(1));
    assert_eq!(dto.start_date, "2026-08-03");
    assert_eq!(dto.end_date, "2026-08-09");
}

#[test]
fn month_offset_zero_is_last_complete_calendar_month() {
    let dto = build_with(&[], month(0));
    assert_eq!(dto.period_kind, ReportPeriodKind::Month);
    assert_eq!(dto.start_date, "2026-07-01");
    assert_eq!(dto.end_date, "2026-07-31");
}

#[test]
fn week_bounds_include_monday_midnight_and_exclude_next_monday() {
    let monday = day(2026, 8, 10);
    let sunday = day(2026, 8, 16);
    let next_monday = day(2026, 8, 17);
    let prev_sunday = day(2026, 8, 9);
    let records = vec![
        usage(monday, 0, 0, 0, "in-start", 10),
        usage(monday, 1, 0, 0, "in-morning", 20),
        usage(sunday, 23, 0, 0, "in-evening", 40),
        usage(sunday, 23, 59, 59, "in-end", 80),
        usage(next_monday, 0, 0, 0, "out-next", 700),
        usage(prev_sunday, 23, 59, 59, "out-prev", 800),
    ];
    let dto = build_with(&records, week(0));
    assert!(dto.has_data);
    assert_eq!(dto.totals.total_tokens, 150);
    assert_eq!(dto.totals.session_count, 4);
}

#[test]
fn in_progress_current_week_is_not_addressable_at_offset_zero() {
    let current_monday = day(2026, 8, 17);
    let records = vec![
        usage(current_monday, 10, 0, 0, "current", 999),
        usage(day(2026, 8, 19), 12, 0, 0, "today", 50),
    ];
    let dto = build_with(&records, week(0));
    assert!(!dto.has_data);
    assert_eq!(dto.totals.total_tokens, 0);
}

#[test]
fn empty_period_returns_has_data_false_with_zero_totals() {
    let records = vec![usage(day(2026, 7, 1), 12, 0, 0, "july", 100)];
    let dto = build_with(&records, week(0));
    assert!(!dto.has_data);
    assert_eq!(dto.totals.total_tokens, 0);
    assert_eq!(dto.totals.input_tokens, 0);
    assert_eq!(dto.totals.session_count, 0);
    assert_eq!(dto.totals.cost, None);
    assert!(dto.insights.is_empty());
    assert_eq!(
        day_tokens(&dto),
        vec![
            ("2026-08-10", 0),
            ("2026-08-11", 0),
            ("2026-08-12", 0),
            ("2026-08-13", 0),
            ("2026-08-14", 0),
            ("2026-08-15", 0),
            ("2026-08-16", 0),
        ]
    );
}

#[test]
fn zero_token_record_still_sets_has_data() {
    let record = usage(day(2026, 8, 12), 12, 0, 0, "zero", 0);
    let dto = build_with(&[record], week(0));
    assert!(dto.has_data);
    assert_eq!(dto.totals.total_tokens, 0);
}

#[test]
fn totals_sum_token_dimensions_and_native_cost() {
    let mut record = usage(day(2026, 8, 12), 14, 0, 0, "dims", 0);
    record.input_tokens = 10;
    record.output_tokens = 20;
    record.cache_read_tokens = 30;
    record.cache_creation_tokens = 40;
    record.reasoning_tokens = 5;
    record.total_tokens = 105;
    record.native_cost = Some(1.25);
    let dto = build_with(&[record], week(0));
    assert!(dto.has_data);
    assert_eq!(dto.totals.input_tokens, 10);
    assert_eq!(dto.totals.output_tokens, 20);
    assert_eq!(dto.totals.cache_read_tokens, 30);
    assert_eq!(dto.totals.cache_creation_tokens, 40);
    assert_eq!(dto.totals.reasoning_tokens, 5);
    assert_eq!(dto.totals.total_tokens, 105);
    assert_eq!(dto.totals.cost, Some(1.25));
}

#[test]
fn cursor_account_usage_does_not_change_report_totals() {
    let records = vec![usage(day(2026, 8, 12), 12, 0, 0, "local", 100)];
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    store::upsert_cursor_account_events(
        &conn,
        &[CursorUsageEvent {
            occurred_at: local_time_iso(day(2026, 8, 12), 12, 0, 0),
            model: "gpt-5".into(),
            input_tokens: 999_999,
            output_tokens: 999_999,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            is_headless: false,
        }],
    )
    .unwrap();
    let dto = crate::report::build(&conn, &PriceTable::default(), week(0), now()).unwrap();
    assert_eq!(dto.totals.total_tokens, 100);
    assert_eq!(dto.totals.session_count, 1);
    let (night_tokens, total_tokens, _) = night_share(&dto);
    assert_eq!(night_tokens, 0);
    assert_eq!(total_tokens, 100);
    assert_eq!(
        day_tokens(&dto),
        vec![
            ("2026-08-10", 0),
            ("2026-08-11", 0),
            ("2026-08-12", 100),
            ("2026-08-13", 0),
            ("2026-08-14", 0),
            ("2026-08-15", 0),
            ("2026-08-16", 0),
        ]
    );
}

#[test]
fn insight_payload_has_no_natural_language_fields() {
    let insights = [
        ReportInsight::NightShare {
            night_tokens: 1,
            total_tokens: 10,
            pct: 10,
        },
        ReportInsight::PeakHours {
            start_hour: 22,
            end_hour: 2,
        },
        ReportInsight::BusiestDay { weekday: 2 },
        ReportInsight::TopSession {
            by: ReportTopSessionBy::Cost,
            source: "claude".into(),
            session_id: "s1".into(),
            project: Some("/proj/a".into()),
            cost: Some(1.5),
            total_tokens: 20,
        },
    ];
    for insight in insights {
        let value = serde_json::to_value(&insight).unwrap();
        let obj = value.as_object().expect("object");
        for banned in ["headline", "comment", "label", "text", "copy"] {
            assert!(!obj.contains_key(banned), "{value} 不得含 {banned}");
        }
        assert!(obj.get("kind").and_then(|v| v.as_str()).is_some());
    }
}

#[test]
fn same_local_hour_across_days_is_merged_into_night_share() {
    let monday = day(2026, 8, 10);
    let tuesday = day(2026, 8, 11);
    let records = vec![
        usage(monday, 3, 0, 0, "night-1", 10),
        usage(tuesday, 3, 15, 0, "night-2", 20),
        usage(tuesday, 14, 0, 0, "afternoon", 70),
    ];
    let dto = build_with(&records, week(0));
    let (night_tokens, total_tokens, pct) = night_share(&dto);
    assert_eq!(night_tokens, 30);
    assert_eq!(total_tokens, 100);
    assert_eq!(pct, 30);
}

#[test]
fn night_share_is_zero_when_no_tokens_land_before_six() {
    let records = vec![
        usage(day(2026, 8, 12), 6, 0, 0, "six", 40),
        usage(day(2026, 8, 12), 22, 0, 0, "evening", 60),
    ];
    let dto = build_with(&records, week(0));
    let (night_tokens, total_tokens, pct) = night_share(&dto);
    assert_eq!(night_tokens, 0);
    assert_eq!(total_tokens, 100);
    assert_eq!(pct, 0);
}

#[test]
fn night_share_is_all_when_every_token_is_before_six() {
    let records = vec![
        usage(day(2026, 8, 12), 0, 0, 0, "midnight", 25),
        usage(day(2026, 8, 13), 5, 59, 0, "almost-six", 75),
    ];
    let dto = build_with(&records, week(0));
    let (night_tokens, total_tokens, pct) = night_share(&dto);
    assert_eq!(night_tokens, 100);
    assert_eq!(total_tokens, 100);
    assert_eq!(pct, 100);
}

#[test]
fn night_share_pct_clamps_sub_one_percent_up_to_one() {
    let records = vec![
        usage(day(2026, 8, 12), 2, 0, 0, "night", 1),
        usage(day(2026, 8, 12), 14, 0, 0, "day", 999),
    ];
    let dto = build_with(&records, week(0));
    let (night_tokens, total_tokens, pct) = night_share(&dto);
    assert_eq!(night_tokens, 1);
    assert_eq!(total_tokens, 1000);
    assert_eq!(pct, 1);
}

#[test]
fn night_share_pct_clamps_over_ninety_nine_down_to_ninety_nine() {
    let records = vec![
        usage(day(2026, 8, 12), 2, 0, 0, "night", 996),
        usage(day(2026, 8, 12), 14, 0, 0, "day", 4),
    ];
    let dto = build_with(&records, week(0));
    let (night_tokens, total_tokens, pct) = night_share(&dto);
    assert_eq!(night_tokens, 996);
    assert_eq!(total_tokens, 1000);
    assert_eq!(pct, 99);
}

#[test]
fn peak_hours_uses_four_hour_window_wrapping_midnight() {
    let records = vec![
        usage(day(2026, 8, 12), 22, 0, 0, "h22", 10),
        usage(day(2026, 8, 12), 23, 0, 0, "h23", 10),
        usage(day(2026, 8, 13), 0, 0, 0, "h0", 10),
        usage(day(2026, 8, 13), 1, 0, 0, "h1", 10),
    ];
    let dto = build_with(&records, week(0));
    assert_eq!(peak_hours(&dto), (22, 2));
}

#[test]
fn peak_hours_tie_takes_the_earlier_start_hour() {
    let records = vec![usage(day(2026, 8, 12), 14, 0, 0, "only", 80)];
    let dto = build_with(&records, week(0));
    assert_eq!(peak_hours(&dto), (11, 15));
}

#[test]
fn records_outside_the_period_do_not_enter_hour_of_day() {
    let records = vec![
        usage(day(2026, 8, 9), 3, 0, 0, "prev-night", 500),
        usage(day(2026, 8, 12), 3, 0, 0, "in-night", 10),
        usage(day(2026, 8, 12), 14, 0, 0, "in-day", 90),
        usage(day(2026, 8, 17), 3, 0, 0, "next-night", 500),
    ];
    let dto = build_with(&records, week(0));
    let (night_tokens, total_tokens, pct) = night_share(&dto);
    assert_eq!(night_tokens, 10);
    assert_eq!(total_tokens, 100);
    assert_eq!(pct, 10);
}

#[test]
fn hour_of_day_uses_local_timezone_not_utc() {
    // 本地 03:00。UTC+8 机器上对应前一天 19:00Z；若误按 UTC 小时归桶就不会进深夜。
    let records = vec![
        usage(day(2026, 8, 12), 3, 0, 0, "local-night", 40),
        usage(day(2026, 8, 12), 15, 0, 0, "local-day", 60),
    ];
    let dto = build_with(&records, week(0));
    let (night_tokens, total_tokens, pct) = night_share(&dto);
    assert_eq!(night_tokens, 40);
    assert_eq!(total_tokens, 100);
    assert_eq!(pct, 40);
}

#[test]
fn week_days_keep_zero_bars_for_days_without_usage() {
    let records = vec![
        usage(day(2026, 8, 10), 10, 0, 0, "mon", 10),
        usage(day(2026, 8, 12), 11, 0, 0, "wed", 50),
        usage(day(2026, 8, 14), 9, 0, 0, "fri", 20),
    ];
    let dto = build_with(&records, week(0));
    assert_eq!(
        day_tokens(&dto),
        vec![
            ("2026-08-10", 10),
            ("2026-08-11", 0),
            ("2026-08-12", 50),
            ("2026-08-13", 0),
            ("2026-08-14", 20),
            ("2026-08-15", 0),
            ("2026-08-16", 0),
        ]
    );
}

#[test]
fn busiest_day_is_the_local_calendar_day_with_most_tokens() {
    let records = vec![
        usage(day(2026, 8, 10), 10, 0, 0, "mon", 10),
        usage(day(2026, 8, 12), 11, 0, 0, "wed-a", 30),
        usage(day(2026, 8, 12), 16, 0, 0, "wed-b", 20),
        usage(day(2026, 8, 14), 9, 0, 0, "fri", 20),
    ];
    let dto = build_with(&records, week(0));
    assert_eq!(busiest_day(&dto), 2);
}

#[test]
fn busiest_day_tie_takes_the_earlier_day() {
    let records = vec![
        usage(day(2026, 8, 10), 10, 0, 0, "mon", 40),
        usage(day(2026, 8, 14), 9, 0, 0, "fri", 40),
    ];
    let dto = build_with(&records, week(0));
    assert_eq!(busiest_day(&dto), 0);
}

#[test]
fn single_day_with_data_keeps_seven_bars_and_that_weekday() {
    let records = vec![usage(day(2026, 8, 13), 14, 0, 0, "thu", 80)];
    let dto = build_with(&records, week(0));
    assert_eq!(
        day_tokens(&dto),
        vec![
            ("2026-08-10", 0),
            ("2026-08-11", 0),
            ("2026-08-12", 0),
            ("2026-08-13", 80),
            ("2026-08-14", 0),
            ("2026-08-15", 0),
            ("2026-08-16", 0),
        ]
    );
    assert_eq!(busiest_day(&dto), 3);
}

#[test]
fn daily_series_uses_local_calendar_day_not_utc_date_prefix() {
    // 本地周一 00:30。UTC+8 上 occurred_at 前缀是周日；误按 UTC 日切会错位。
    let records = vec![usage(day(2026, 8, 10), 0, 30, 0, "monday-early", 25)];
    let dto = build_with(&records, week(0));
    assert_eq!(
        day_tokens(&dto),
        vec![
            ("2026-08-10", 25),
            ("2026-08-11", 0),
            ("2026-08-12", 0),
            ("2026-08-13", 0),
            ("2026-08-14", 0),
            ("2026-08-15", 0),
            ("2026-08-16", 0),
        ]
    );
    assert_eq!(busiest_day(&dto), 0);
}
