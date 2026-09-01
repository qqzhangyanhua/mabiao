use crate::test_support::*;

#[test]
fn billing_window_keeps_activity_within_five_hours() {
    let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    let records = vec![
        window_rec("2026-08-17T08:10:00Z", Source::Claude, "s1", 100),
        window_rec("2026-08-17T09:10:00Z", Source::Claude, "s1", 50),
    ];
    let dto = billing_window::summarize(&records, &PriceTable::default(), now);
    assert_eq!(dto.current.len(), 1);
    assert!(dto.recent.is_empty());
    let window = &dto.current[0];
    assert_eq!(window.source, "claude");
    assert_eq!(window.start, "2026-08-17T08:00:00Z");
    assert_eq!(window.end, "2026-08-17T13:00:00Z");
    assert_eq!(window.total_tokens, 150);
    assert_eq!(window.session_count, 1);
    assert_eq!(window.remaining_minutes, Some(60));
    let burn = window.burn.as_ref().expect("应有燃烧速率");
    assert!((burn.tokens_per_minute - 2.5).abs() < 1e-9);
    let projection = window.projection.as_ref().expect("应有预测");
    assert_eq!(projection.total_tokens, 300);
}

#[test]
fn billing_window_opens_after_five_hour_gap() {
    let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    let records = vec![
        window_rec("2026-08-17T02:00:00Z", Source::Claude, "s1", 80),
        window_rec("2026-08-17T08:00:00Z", Source::Claude, "s1", 40),
    ];
    let dto = billing_window::summarize(&records, &PriceTable::default(), now);
    assert_eq!(dto.current.len(), 1);
    assert_eq!(dto.recent.len(), 1);
    assert_eq!(dto.recent[0].start, "2026-08-17T02:00:00Z");
    assert_eq!(dto.recent[0].end, "2026-08-17T07:00:00Z");
    assert!(!dto.recent[0].is_active);
    assert_eq!(dto.current[0].start, "2026-08-17T08:00:00Z");
    assert_eq!(dto.current[0].total_tokens, 40);
}

#[test]
fn billing_window_floors_start_to_utc_hour() {
    let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    let records = vec![window_rec("2026-08-17T08:37:12Z", Source::Claude, "s1", 10)];
    let dto = billing_window::summarize(&records, &PriceTable::default(), now);
    assert_eq!(dto.current[0].start, "2026-08-17T08:00:00Z");
    assert_eq!(dto.current[0].end, "2026-08-17T13:00:00Z");
    assert!(dto.current[0].burn.is_none());
    assert!(dto.current[0].projection.is_none());
}

#[test]
fn billing_window_expires_after_end_or_idle() {
    let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    let expired = vec![window_rec("2026-08-17T06:00:00Z", Source::Claude, "s1", 20)];
    let dto = billing_window::summarize(&expired, &PriceTable::default(), now);
    assert!(dto.current.is_empty());
    assert_eq!(dto.recent.len(), 1);
    assert!(!dto.recent[0].is_active);
    assert_eq!(dto.recent[0].remaining_minutes, None);
}

#[test]
fn billing_windows_do_not_mix_sources() {
    let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    let records = vec![
        window_rec("2026-08-17T10:00:00Z", Source::Claude, "c1", 30),
        window_rec("2026-08-17T10:05:00Z", Source::Codex, "x1", 90),
    ];
    let dto = billing_window::summarize(&records, &PriceTable::default(), now);
    assert_eq!(dto.current.len(), 2);
    let claude = dto
        .current
        .iter()
        .find(|window| window.source == "claude")
        .expect("claude");
    let codex = dto
        .current
        .iter()
        .find(|window| window.source == "codex")
        .expect("codex");
    assert_eq!(claude.total_tokens, 30);
    assert_eq!(codex.total_tokens, 90);
}

#[test]
fn weekly_window_sums_last_seven_days_per_source() {
    let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    let records = vec![
        window_rec("2026-08-11T12:00:00Z", Source::Claude, "s1", 100),
        window_rec("2026-08-16T09:00:00Z", Source::Claude, "s1", 50),
        // 8 天前，超出 7 天滚动窗口，不应计入。
        window_rec("2026-08-09T12:00:00Z", Source::Claude, "s2", 999),
        window_rec("2026-08-15T00:00:00Z", Source::Codex, "x1", 70),
    ];
    let dto = billing_window::summarize(&records, &PriceTable::default(), now);
    assert_eq!(dto.weekly_window_days, 7);
    assert_eq!(dto.weekly.len(), 2);

    let claude = dto
        .weekly
        .iter()
        .find(|window| window.source == "claude")
        .expect("claude weekly window");
    assert_eq!(claude.total_tokens, 150);
    assert_eq!(claude.session_count, 1);
    assert_eq!(claude.end, "2026-08-17T12:00:00Z");
    assert_eq!(claude.start, "2026-08-10T12:00:00Z");
    assert!((claude.daily_average_tokens - 150.0 / 7.0).abs() < 1e-9);
    let claude_cost = claude.cost.expect("claude weekly cost");
    assert!((claude_cost - 0.15).abs() < 1e-9);
    let claude_daily_cost = claude.daily_average_cost.expect("claude daily cost");
    assert!((claude_daily_cost - claude_cost / 7.0).abs() < 1e-9);

    let codex = dto
        .weekly
        .iter()
        .find(|window| window.source == "codex")
        .expect("codex weekly window");
    assert_eq!(codex.total_tokens, 70);

    // 按 total_tokens 降序排列。
    assert_eq!(dto.weekly[0].source, "claude");
}

#[test]
fn weekly_window_excludes_activity_older_than_seven_days_but_within_the_lookback() {
    let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    // 10 天前：仍落在 14 天摄取回看窗内，但超出 7 天滚动窗口，不应计入 weekly。
    let records = vec![window_rec("2026-08-07T12:00:00Z", Source::Claude, "s1", 40)];
    let dto = billing_window::summarize(&records, &PriceTable::default(), now);
    assert!(dto.weekly.is_empty());
    // 仍应出现在 recent（5 小时窗）里，证明记录本身被正常摄取，只是不满足 weekly 的时间范围。
    assert_eq!(dto.recent.len(), 1);
}

#[test]
fn weekly_window_includes_cursor_account_usage_priced_by_snapshot() {
    use crate::domain::{CursorUsageEvent, PriceEntry, PriceOrigin};

    let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    let records = vec![window_rec("2026-08-16T09:00:00Z", Source::Claude, "s1", 50)];
    let events = vec![
        CursorUsageEvent {
            occurred_at: "2026-08-16T10:00:00Z".into(),
            model: "claude-4.5-sonnet".into(),
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            is_headless: false,
        },
        // 8 天前：超出 7 天滚动窗口。
        CursorUsageEvent {
            occurred_at: "2026-08-09T10:00:00Z".into(),
            model: "claude-4.5-sonnet".into(),
            input_tokens: 9_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            is_headless: false,
        },
        // 无 LiteLLM / 用户单价的模型：计入 token，费用标记 unpriced。
        CursorUsageEvent {
            occurred_at: "2026-08-15T08:00:00Z".into(),
            model: "composer-2".into(),
            input_tokens: 200,
            output_tokens: 80,
            cache_read_tokens: 0,
            cache_creation_tokens: 5,
            is_headless: true,
        },
    ];
    let prices = PriceTable {
        prices: vec![PriceEntry {
            model: "claude-4.5-sonnet".into(),
            provider: None,
            input: 3.0 / 1_000_000.0,
            output: 15.0 / 1_000_000.0,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::Snapshot,
        }],
    };

    let dto = billing_window::attach_cursor_weekly(
        billing_window::summarize(&records, &prices, now),
        &events,
        &prices,
        now,
    );
    assert!(dto.current.iter().all(|window| window.source != "cursor"));
    assert!(dto.recent.iter().all(|window| window.source != "cursor"));

    let cursor = dto
        .weekly
        .iter()
        .find(|window| window.source == "cursor")
        .expect("cursor weekly window");
    assert_eq!(cursor.application, "Cursor");
    assert_eq!(cursor.total_tokens, 1_500_285);
    assert_eq!(cursor.input_tokens, 1_000_200);
    assert_eq!(cursor.output_tokens, 500_080);
    assert_eq!(cursor.cache_creation_tokens, 5);
    assert_eq!(cursor.session_count, 2);
    assert!(cursor.unpriced);
    let cost = cursor.cost.expect("priced cursor events should contribute");
    assert!((cost - 10.5).abs() < 1e-9);
    assert!((cursor.daily_average_tokens - 1_500_285.0 / 7.0).abs() < 1e-9);
    let daily_cost = cursor.daily_average_cost.expect("daily cost");
    assert!((daily_cost - cost / 7.0).abs() < 1e-9);
    assert_eq!(dto.weekly[0].source, "cursor");
}

#[test]
fn weekly_window_omits_cursor_when_account_events_are_outside_window() {
    use crate::domain::CursorUsageEvent;

    let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    let events = vec![CursorUsageEvent {
        occurred_at: "2026-08-09T12:00:00Z".into(),
        model: "claude-4.5-sonnet".into(),
        input_tokens: 100,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        is_headless: false,
    }];
    let dto = billing_window::attach_cursor_weekly(
        billing_window::summarize(&[] as &[UsageRecord], &PriceTable::default(), now),
        &events,
        &PriceTable::default(),
        now,
    );
    assert!(dto.weekly.is_empty());
}

#[test]
fn weekly_window_prices_cursor_models_by_litellm_signature() {
    use crate::domain::CursorUsageEvent;

    let now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    let events = vec![
        CursorUsageEvent {
            occurred_at: "2026-08-16T10:00:00Z".into(),
            model: "claude-4.6-sonnet".into(),
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            is_headless: false,
        },
        CursorUsageEvent {
            occurred_at: "2026-08-16T11:00:00Z".into(),
            model: "claude-4.5-sonnet-thinking".into(),
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            is_headless: false,
        },
        CursorUsageEvent {
            occurred_at: "2026-08-16T12:00:00Z".into(),
            model: "gpt-5-high".into(),
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            is_headless: false,
        },
        CursorUsageEvent {
            occurred_at: "2026-08-16T13:00:00Z".into(),
            model: "composer-2".into(),
            input_tokens: 200,
            output_tokens: 80,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            is_headless: true,
        },
    ];
    let prices = crate::litellm::merge(&PriceTable::default(), &crate::litellm::bundled_snapshot());

    let dto = billing_window::attach_cursor_weekly(
        billing_window::summarize(&[] as &[UsageRecord], &prices, now),
        &events,
        &prices,
        now,
    );
    let cursor = dto
        .weekly
        .iter()
        .find(|window| window.source == "cursor")
        .expect("cursor weekly window");
    let cost = cursor.cost.expect("Cursor 模型应按 LiteLLM 签名匹配到单价");
    assert!(cost > 0.0, "匹配到的费用应为正数，实际 {cost}");
    assert!(
        cursor.unpriced,
        "composer-2 仍无公开价目，整行应保留部分未定价"
    );
    assert_eq!(cursor.total_tokens, 3_000_280);
}

#[test]
fn billing_windows_lookback_predicate_uses_occurred_at_index() {
    let conn = store::open_memory().unwrap();
    let plan: Vec<String> = conn
        .prepare(
            "EXPLAIN QUERY PLAN SELECT r.occurred_at FROM usage_records r \
             WHERE r.occurred_at >= ?1",
        )
        .unwrap()
        .query_map(["2026-08-03"], |row| row.get(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|detail| detail.contains("USING") && detail.contains("INDEX")),
        "billing_windows lookback must use an occurred_at index, query plan: {plan:?}"
    );
}
