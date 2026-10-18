use crate::test_support::*;

#[test]
fn cursor_account_parser_maps_token_dimensions_and_keeps_duplicates() {
    let page = cursor_account::parse_cursor_usage_page(&fixture("cursor_account_usage.json"))
        .expect("parse cursor account fixture");
    assert_eq!(page.total_count, 3);
    let events = page.events;
    assert_eq!(events.len(), 3);

    assert_eq!(events[0].occurred_at, "2024-01-01T00:00:00+00:00");
    assert_eq!(events[0].model, "claude-4.5-sonnet");
    assert_eq!(events[0].input_tokens, 100);
    assert_eq!(events[0].output_tokens, 50);
    assert_eq!(events[0].cache_read_tokens, 20);
    assert_eq!(events[0].cache_creation_tokens, 10);
    assert!(!events[0].is_headless);

    assert_eq!(events[1].occurred_at, "2024-01-02T00:00:00+00:00");
    assert_eq!(events[1].model, "composer-2");
    assert_eq!(events[1].input_tokens, 200);
    assert_eq!(events[1].output_tokens, 80);
    assert_eq!(events[1].cache_read_tokens, 0);
    assert_eq!(events[1].cache_creation_tokens, 5);
    assert!(events[1].is_headless);

    assert_eq!(events[2], events[0]);
}

#[test]
fn cursor_account_parser_rejects_bad_json_and_skips_empty_payload() {
    let err =
        cursor_account::parse_cursor_usage_events("{not-json").expect_err("bad json should fail");
    assert!(err.contains("解析失败"), "错误应可读：{err}");

    let empty = cursor_account::parse_cursor_usage_events(r#"{"usageEventsDisplay":[]}"#)
        .expect("empty list is valid");
    assert!(empty.is_empty());

    let missing = cursor_account::parse_cursor_usage_events(r#"{"totalUsageEventsCount":0}"#)
        .expect_err("missing list is a structure change");
    assert!(missing.contains("结构已变更"), "{missing}");
}

#[test]
fn cursor_account_summary_adds_token_dimensions_without_dedup() {
    let events = cursor_account::parse_cursor_usage_events(&fixture("cursor_account_usage.json"))
        .expect("parse");
    let dto = cursor_account::summarize_cursor_usage(&events);
    assert_eq!(dto.event_count, 3);
    assert_eq!(dto.input_tokens, 400);
    assert_eq!(dto.output_tokens, 180);
    assert_eq!(dto.cache_read_tokens, 40);
    assert_eq!(dto.cache_creation_tokens, 25);
    assert_eq!(dto.total_tokens, 645);
    assert_eq!(dto.as_of, None);

    let empty = cursor_account::summarize_cursor_usage(&[]);
    assert_eq!(empty, crate::domain::CursorAccountUsageDto::empty());
}

#[test]
fn cursor_account_summary_buckets_tokens_by_local_day() {
    use crate::domain::CursorUsageEvent;

    fn ev(occurred_at: &str, input: i64, output: i64) -> CursorUsageEvent {
        CursorUsageEvent {
            occurred_at: occurred_at.to_string(),
            model: "m".into(),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            is_headless: false,
        }
    }

    let day_a = chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    let day_b = chrono::NaiveDate::from_ymd_opt(2024, 1, 16).unwrap();
    let dto = cursor_account::summarize_cursor_usage(&[
        ev(&local_noon_iso(day_a), 100, 10),
        ev(&local_noon_iso(day_a), 50, 5),
        ev(&local_noon_iso(day_b), 20, 2),
    ]);
    assert_eq!(dto.daily.len(), 2);
    assert_eq!(dto.daily[0].bucket, "2024-01-15");
    assert_eq!(dto.daily[0].input_tokens, 150);
    assert_eq!(dto.daily[0].output_tokens, 15);
    assert_eq!(dto.daily[0].total_tokens, 165);
    assert_eq!(dto.daily[0].cost, None);
    assert_eq!(dto.daily[1].bucket, "2024-01-16");
    assert_eq!(dto.daily[1].total_tokens, 22);

    let single = cursor_account::summarize_cursor_usage(&[ev(&local_noon_iso(day_a), 7, 3)]);
    assert_eq!(single.daily.len(), 1);
    assert_eq!(single.daily[0].bucket, "2024-01-15");
    assert_eq!(single.daily[0].total_tokens, 10);

    let empty = cursor_account::summarize_cursor_usage(&[]);
    assert!(empty.daily.is_empty());
}

#[test]
fn cursor_account_summary_splits_by_model_and_headless() {
    use crate::domain::CursorUsageEvent;

    fn ev(model: &str, input: i64, headless: bool) -> CursorUsageEvent {
        CursorUsageEvent {
            occurred_at: "2024-01-15T12:00:00+00:00".into(),
            model: model.into(),
            input_tokens: input,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            is_headless: headless,
        }
    }

    let dto = cursor_account::summarize_cursor_usage(&[
        ev("claude-4.5-sonnet", 300, false),
        ev("composer-2", 100, true),
        ev("claude-4.5-sonnet", 100, true),
    ]);
    assert_eq!(dto.by_model.len(), 2);
    assert_eq!(dto.by_model[0].name, "claude-4.5-sonnet");
    assert_eq!(dto.by_model[0].total_tokens, 400);
    assert!((dto.by_model[0].share - 0.8).abs() < 1e-9);
    assert_eq!(dto.by_model[1].name, "composer-2");
    assert_eq!(dto.by_model[1].total_tokens, 100);
    assert!((dto.by_model[1].share - 0.2).abs() < 1e-9);
    assert_eq!(dto.headless_tokens, 200);
    assert_eq!(dto.interactive_tokens, 300);
    assert!((dto.headless_share.unwrap() - 0.4).abs() < 1e-9);

    let interactive_only = cursor_account::summarize_cursor_usage(&[ev("gpt-5", 50, false)]);
    assert_eq!(interactive_only.by_model.len(), 1);
    assert_eq!(interactive_only.headless_tokens, 0);
    assert_eq!(interactive_only.interactive_tokens, 50);
    assert_eq!(interactive_only.headless_share, Some(0.0));

    let empty = cursor_account::summarize_cursor_usage(&[]);
    assert!(empty.by_model.is_empty());
    assert_eq!(empty.headless_tokens, 0);
    assert_eq!(empty.interactive_tokens, 0);
    assert_eq!(empty.headless_share, None);
}

#[test]
fn cursor_account_filter_keeps_time_and_model_only() {
    use crate::domain::CursorUsageEvent;

    fn ev(occurred_at: &str, model: &str, input: i64) -> CursorUsageEvent {
        CursorUsageEvent {
            occurred_at: occurred_at.into(),
            model: model.into(),
            input_tokens: input,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            is_headless: false,
        }
    }

    let early = ev("2024-01-15T12:00:00+00:00", "composer-2", 10);
    let mid = ev("2024-01-16T12:00:00+00:00", "gpt-5", 20);
    let late = ev("2024-01-17T12:00:00+00:00", "gpt-5", 40);
    let conn = store::open_memory().unwrap();
    store::upsert_cursor_account_events(&conn, &[early, mid, late]).unwrap();
    store::set_cursor_account_as_of(&conn, "2024-01-17T18:00:00+00:00").unwrap();

    let range = Filter {
        from: Some("2024-01-16T00:00:00.000Z".into()),
        to: Some("2024-01-16T23:59:59.000Z".into()),
        ..Filter::default()
    };
    let by_day = crate::cursor_account::load_summary_filtered(&conn, Some(&range)).unwrap();
    assert_eq!(by_day.event_count, 1);
    assert_eq!(by_day.total_tokens, 20);
    assert_eq!(by_day.as_of.as_deref(), Some("2024-01-17T18:00:00+00:00"));

    let by_model = crate::cursor_account::load_summary_filtered(
        &conn,
        Some(&Filter {
            models: vec!["gpt-5".into()],
            ..Filter::default()
        }),
    )
    .unwrap();
    assert_eq!(by_model.event_count, 2);
    assert_eq!(by_model.total_tokens, 60);

    let sources_ignored = crate::cursor_account::event_matches_filter(
        &ev("2024-01-16T12:00:00+00:00", "gpt-5", 1),
        &Filter {
            sources: vec!["codex".into()],
            projects: vec!["/tmp".into()],
            providers: vec!["openai".into()],
            ..Filter::default()
        },
    );
    assert!(sources_ignored);
}

#[test]
fn cursor_account_store_dedups_by_fingerprint() {
    let events = cursor_account::parse_cursor_usage_events(&fixture("cursor_account_usage.json"))
        .expect("parse");
    assert_eq!(events.len(), 3);

    let conn = store::open_memory().unwrap();
    let first = store::upsert_cursor_account_events(&conn, &events).unwrap();
    let second = store::upsert_cursor_account_events(&conn, &events).unwrap();
    assert_eq!(first, 2);
    assert_eq!(second, 0);

    let stored = store::load_cursor_account_events(&conn).unwrap();
    assert_eq!(stored.len(), 2);
    let dto = cursor_account::summarize_cursor_usage(&stored);
    assert_eq!(dto.event_count, 2);
    assert_eq!(dto.input_tokens, 300);
    assert_eq!(dto.output_tokens, 130);
    assert_eq!(dto.cache_read_tokens, 20);
    assert_eq!(dto.cache_creation_tokens, 15);
    assert_eq!(dto.total_tokens, 465);

    store::set_cursor_account_as_of(&conn, "2026-08-17T12:00:00+00:00").unwrap();
    assert_eq!(
        store::cursor_account_as_of(&conn).unwrap().as_deref(),
        Some("2026-08-17T12:00:00+00:00")
    );

    store::clear_cursor_account_usage(&conn).unwrap();
    assert!(store::load_cursor_account_events(&conn).unwrap().is_empty());
    assert_eq!(store::cursor_account_as_of(&conn).unwrap(), None);
}

#[test]
fn cursor_account_clear_resets_watermark_without_touching_usage_records() {
    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-01-01T00:00:00+00:00",
            Source::Codex,
            "gpt-5",
            "openai",
            "/tmp/demo",
            "sess-keep",
            42,
        )],
    )
    .unwrap();
    crate::cursor_account::ingest_raw_pages(&conn, &[CURSOR_PAGE_ONE]).unwrap();
    store::set_cursor_account_as_of(&conn, "2026-08-17T12:00:00+00:00").unwrap();
    assert_eq!(
        crate::cursor_account::incremental_start_ms(&conn).unwrap(),
        1_704_153_600_000
    );

    let cleared = crate::cursor_account::clear_cache(&conn).unwrap();
    assert_eq!(cleared.event_count, 0);
    assert_eq!(cleared.total_tokens, 0);
    assert_eq!(cleared.as_of, None);
    assert_eq!(
        crate::cursor_account::incremental_start_ms(&conn).unwrap(),
        0
    );
    assert!(store::load_cursor_account_events(&conn).unwrap().is_empty());

    let kept = store::load_all(&conn).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].session_id, "sess-keep");
    assert_eq!(kept[0].total_tokens, 42);
}

const CURSOR_PAGE_ONE: &str = r#"{
    "totalUsageEventsCount": 3,
    "usageEventsDisplay": [
        {
            "timestamp": "1704067200000",
            "model": "claude-4.5-sonnet",
            "isHeadless": false,
            "tokenUsage": {
                "inputTokens": 100,
                "outputTokens": 50,
                "cacheReadTokens": 20,
                "cacheWriteTokens": 10
            }
        },
        {
            "timestamp": "1704153600000",
            "model": "composer-2",
            "isHeadless": true,
            "tokenUsage": {
                "inputTokens": 200,
                "outputTokens": 80,
                "cacheReadTokens": 0,
                "cacheWriteTokens": 5
            }
        }
    ]
}"#;

const CURSOR_PAGE_TWO: &str = r#"{
    "totalUsageEventsCount": 3,
    "usageEventsDisplay": [
        {
            "timestamp": "1704153600000",
            "model": "composer-2",
            "isHeadless": true,
            "tokenUsage": {
                "inputTokens": 200,
                "outputTokens": 80,
                "cacheReadTokens": 0,
                "cacheWriteTokens": 5
            }
        },
        {
            "timestamp": "1704240000000",
            "model": "gpt-5",
            "isHeadless": false,
            "tokenUsage": {
                "inputTokens": 30,
                "outputTokens": 10,
                "cacheReadTokens": 0,
                "cacheWriteTokens": 0
            }
        }
    ]
}"#;

#[test]
fn cursor_account_ingest_dedups_overlapping_pages() {
    let conn = store::open_memory().unwrap();
    let written =
        crate::cursor_account::ingest_raw_pages(&conn, &[CURSOR_PAGE_ONE, CURSOR_PAGE_TWO])
            .unwrap();
    assert_eq!(written, 3);

    let dto = crate::cursor_account::load_summary(&conn).unwrap();
    assert_eq!(dto.event_count, 3);
    assert_eq!(dto.input_tokens, 330);
    assert_eq!(dto.output_tokens, 140);
    assert_eq!(dto.cache_read_tokens, 20);
    assert_eq!(dto.cache_creation_tokens, 15);
    assert_eq!(dto.total_tokens, 505);
}

#[test]
fn cursor_account_incremental_ingest_only_adds_new_events() {
    let conn = store::open_memory().unwrap();
    assert_eq!(
        crate::cursor_account::incremental_start_ms(&conn).unwrap(),
        0
    );

    let first = crate::cursor_account::ingest_raw_pages(&conn, &[CURSOR_PAGE_ONE]).unwrap();
    assert_eq!(first, 2);
    assert_eq!(
        crate::cursor_account::incremental_start_ms(&conn).unwrap(),
        1_704_153_600_000
    );

    let second = crate::cursor_account::ingest_raw_pages(&conn, &[CURSOR_PAGE_TWO]).unwrap();
    assert_eq!(second, 1);
    assert_eq!(
        crate::cursor_account::incremental_start_ms(&conn).unwrap(),
        1_704_240_000_000
    );

    let again = crate::cursor_account::ingest_raw_pages(&conn, &[CURSOR_PAGE_TWO]).unwrap();
    assert_eq!(again, 0);

    let dto = crate::cursor_account::load_summary(&conn).unwrap();
    assert_eq!(dto.event_count, 3);
    assert_eq!(dto.total_tokens, 505);
}

#[test]
fn cursor_account_failed_refresh_keeps_last_good_cache() {
    let conn = store::open_memory().unwrap();
    crate::cursor_account::ingest_raw_pages(&conn, &[CURSOR_PAGE_ONE]).unwrap();
    store::set_cursor_account_as_of(&conn, "2026-08-17T12:00:00+00:00").unwrap();
    let before = crate::cursor_account::load_summary(&conn).unwrap();
    assert_eq!(before.event_count, 2);
    assert_eq!(before.total_tokens, 465);

    let auth = crate::cursor_account::apply_fetched_pages(
        &conn,
        Err(crate::cursor_account::auth_expired_error()),
    );
    assert!(
        auth.as_ref().unwrap_err().contains("过期"),
        "{}",
        auth.unwrap_err()
    );
    let after_auth = crate::cursor_account::load_summary(&conn).unwrap();
    assert_eq!(after_auth.event_count, 2);
    assert_eq!(after_auth.total_tokens, 465);
    assert_eq!(
        after_auth.as_of.as_deref(),
        Some("2026-08-17T12:00:00+00:00")
    );

    let structure = crate::cursor_account::apply_fetched_pages(
        &conn,
        Ok(vec![r#"{"totalUsageEventsCount":9}"#.to_string()]),
    );
    assert!(
        structure.as_ref().unwrap_err().contains("结构已变更"),
        "{}",
        structure.unwrap_err()
    );
    assert_eq!(
        crate::cursor_account::load_summary(&conn)
            .unwrap()
            .event_count,
        2
    );

    let parse = crate::cursor_account::apply_fetched_pages(
        &conn,
        Ok(vec![CURSOR_PAGE_TWO.to_string(), "{not-json".to_string()]),
    );
    assert!(
        parse.as_ref().unwrap_err().contains("解析失败"),
        "{}",
        parse.unwrap_err()
    );
    let after_parse = crate::cursor_account::load_summary(&conn).unwrap();
    assert_eq!(after_parse.event_count, 2);
    assert_eq!(after_parse.total_tokens, 465);
    assert_eq!(
        after_parse.as_of.as_deref(),
        Some("2026-08-17T12:00:00+00:00")
    );

    let network = crate::cursor_account::apply_fetched_pages(
        &conn,
        Err(crate::cursor_account::network_failure_error()),
    );
    assert!(
        network.as_ref().unwrap_err().contains("网络"),
        "{}",
        network.unwrap_err()
    );
    assert_eq!(
        crate::cursor_account::load_summary(&conn)
            .unwrap()
            .event_count,
        2
    );

    let empty = crate::cursor_account::apply_fetched_pages(
        &conn,
        Ok(vec![r#"{"usageEventsDisplay":[]}"#.to_string()]),
    )
    .unwrap();
    assert_eq!(empty.event_count, 2);
    assert_eq!(empty.total_tokens, 465);
}

#[test]
fn billing_windows_query_appends_cursor_weekly_from_account_cache() {
    use crate::domain::{CursorUsageEvent, PriceEntry, PriceOrigin};

    let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[window_rec("2026-08-16T09:00:00Z", Source::Claude, "s1", 50)],
    )
    .unwrap();
    store::upsert_cursor_account_events(
        &conn,
        &[CursorUsageEvent {
            occurred_at: "2026-08-16T10:00:00Z".into(),
            model: "claude-4.5-sonnet".into(),
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            is_headless: false,
        }],
    )
    .unwrap();
    let prices = PriceTable {
        prices: vec![PriceEntry {
            model: "claude-4.5-sonnet".into(),
            provider: None,
            input: 0.01,
            output: 0.02,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::Snapshot,
            ..Default::default()
        }],
    };

    let dto = query::billing_windows(&conn, &Filter::default(), &prices, now).unwrap();
    let cursor = dto
        .weekly
        .iter()
        .find(|window| window.source == "cursor")
        .expect("cursor weekly from query");
    assert_eq!(cursor.total_tokens, 120);
    assert_eq!(cursor.session_count, 1);
    let cost = cursor.cost.expect("LiteLLM fallback cost");
    assert!((cost - 1.4).abs() < 1e-9);
    assert!(!cursor.unpriced);

    let only_claude = Filter {
        sources: vec!["claude".into()],
        ..Filter::default()
    };
    let filtered = query::billing_windows(&conn, &only_claude, &prices, now).unwrap();
    assert!(filtered
        .weekly
        .iter()
        .all(|window| window.source != "cursor"));

    let only_cursor = Filter {
        sources: vec!["cursor".into()],
        ..Filter::default()
    };
    let cursor_only = query::billing_windows(&conn, &only_cursor, &prices, now).unwrap();
    assert_eq!(cursor_only.weekly.len(), 1);
    assert_eq!(cursor_only.weekly[0].source, "cursor");
    assert!(cursor_only.current.is_empty());

    let other_model = Filter {
        models: vec!["composer-2".into()],
        ..Filter::default()
    };
    let by_model = query::billing_windows(&conn, &other_model, &prices, now).unwrap();
    assert!(by_model
        .weekly
        .iter()
        .all(|window| window.source != "cursor"));
}

#[test]
fn application_analytics_overlays_cursor_account_usage_without_touching_summary() {
    use crate::domain::CursorUsageEvent;

    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-16T09:00:00Z",
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            100,
        )],
    )
    .unwrap();
    store::upsert_cursor_account_events(
        &conn,
        &[
            CursorUsageEvent {
                occurred_at: "2026-08-16T10:00:00Z".into(),
                model: "claude-4.5-sonnet".into(),
                input_tokens: 80,
                output_tokens: 20,
                cache_read_tokens: 10,
                cache_creation_tokens: 0,
                is_headless: false,
            },
            CursorUsageEvent {
                occurred_at: "2026-07-01T10:00:00Z".into(),
                model: "claude-4.5-sonnet".into(),
                input_tokens: 9_000,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                is_headless: false,
            },
        ],
    )
    .unwrap();

    let dto = query::application_analytics(
        &conn,
        &Filter {
            from: Some("2026-08-01T00:00:00Z".into()),
            to: Some("2026-08-31T23:59:59Z".into()),
            sources: vec!["codex".into()],
            ..Filter::default()
        },
        "day",
    )
    .unwrap();

    assert_eq!(dto.summary.total_tokens, 100);
    let cursor = dto
        .by_application
        .iter()
        .find(|row| row.source == "cursor")
        .expect("cursor application row");
    assert_eq!(cursor.application, "Cursor");
    assert_eq!(cursor.metrics.total_tokens, 110);
    assert_eq!(cursor.metrics.session_count, 1);
    let day = dto
        .trend
        .iter()
        .find(|point| point.bucket == "2026-08-16")
        .expect("cursor trend bucket");
    assert_eq!(day.values["cursor"], 110);
    let unlabeled = dto
        .projects
        .iter()
        .find(|row| row.project == "（未标注）")
        .expect("cursor unlabeled project");
    assert_eq!(unlabeled.values["cursor"], 110);
}

#[test]
fn trend_overlays_cursor_account_usage_into_matching_buckets() {
    use crate::domain::CursorUsageEvent;

    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-16T09:00:00Z",
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            100,
        )],
    )
    .unwrap();
    store::upsert_cursor_account_events(
        &conn,
        &[
            CursorUsageEvent {
                occurred_at: "2026-08-16T10:00:00Z".into(),
                model: "claude-4.5-sonnet".into(),
                input_tokens: 80,
                output_tokens: 20,
                cache_read_tokens: 10,
                cache_creation_tokens: 0,
                is_headless: false,
            },
            CursorUsageEvent {
                occurred_at: "2026-07-01T10:00:00Z".into(),
                model: "claude-4.5-sonnet".into(),
                input_tokens: 9_000,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                is_headless: false,
            },
        ],
    )
    .unwrap();

    let points = query::trend(
        &conn,
        &Filter {
            from: Some("2026-08-01T00:00:00Z".into()),
            to: Some("2026-08-31T23:59:59Z".into()),
            ..Filter::default()
        },
        &PriceTable::default(),
        "day",
    )
    .unwrap();
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].bucket, "2026-08-16");
    assert_eq!(points[0].total_tokens, 210);
    assert_eq!(points[0].input_tokens, 180);
    assert_eq!(points[0].output_tokens, 20);
    assert_eq!(points[0].cache_read_tokens, 10);
}

#[test]
fn project_breakdown_adds_cursor_account_usage_as_its_own_row() {
    use crate::domain::CursorUsageEvent;

    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-16T09:00:00Z",
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            100,
        )],
    )
    .unwrap();
    store::upsert_cursor_account_events(
        &conn,
        &[
            CursorUsageEvent {
                occurred_at: "2026-08-16T10:00:00Z".into(),
                model: "claude-4.5-sonnet".into(),
                input_tokens: 80,
                output_tokens: 20,
                cache_read_tokens: 10,
                cache_creation_tokens: 0,
                is_headless: false,
            },
            CursorUsageEvent {
                occurred_at: "2026-07-01T10:00:00Z".into(),
                model: "claude-4.5-sonnet".into(),
                input_tokens: 9_000,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                is_headless: false,
            },
        ],
    )
    .unwrap();

    let rows = query::breakdown(
        &conn,
        &Filter {
            from: Some("2026-08-01T00:00:00Z".into()),
            to: Some("2026-08-31T23:59:59Z".into()),
            ..Filter::default()
        },
        &PriceTable::default(),
        "project",
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name, "Cursor");
    assert_eq!(rows[0].total_tokens, 110);
    assert!((rows[0].share - 110.0 / 210.0).abs() < 1e-9);
    assert_eq!(rows[1].name, "/proj/a");
    assert_eq!(rows[1].total_tokens, 100);

    let models = query::breakdown(
        &conn,
        &Filter {
            from: Some("2026-08-01T00:00:00Z".into()),
            to: Some("2026-08-31T23:59:59Z".into()),
            ..Filter::default()
        },
        &PriceTable::default(),
        "model",
    )
    .unwrap();
    assert!(models.iter().all(|row| row.name != "Cursor"));
}
