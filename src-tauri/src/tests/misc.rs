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

// ---------- LiteLLM 价目快照 ----------

const LITELLM_RAW_SAMPLE: &str = r#"{
    "sample_spec": {"note": "占位，应被跳过"},
    "gpt-4o": {
        "litellm_provider": "openai",
        "mode": "chat",
        "input_cost_per_token": 2.5e-06,
        "output_cost_per_token": 1e-05,
        "cache_read_input_token_cost": 1.25e-06
    },
    "anthropic/claude-3-5-sonnet": {
        "litellm_provider": "anthropic",
        "mode": "chat",
        "input_cost_per_token": 3e-06,
        "output_cost_per_token": 1.5e-05,
        "cache_creation_input_token_cost": 3.75e-06
    },
    "claude-3-5-sonnet": {
        "litellm_provider": "anthropic",
        "mode": "chat",
        "input_cost_per_token": 3e-06,
        "output_cost_per_token": 1.5e-05
    },
    "text-embedding-3-small": {
        "litellm_provider": "openai",
        "mode": "embedding",
        "input_cost_per_token": 2e-08
    },
    "free-local-model": {
        "litellm_provider": "ollama",
        "mode": "chat",
        "input_cost_per_token": 0,
        "output_cost_per_token": 0
    }
}"#;

#[test]
fn litellm_snapshot_normalizes_upstream_and_skips_noise() {
    let snapshot = crate::litellm::parse_litellm_raw(LITELLM_RAW_SAMPLE, "2026-08-17")
        .expect("parse litellm sample");
    assert_eq!(snapshot.as_of, "2026-08-17");
    assert_eq!(snapshot.source, "litellm");

    let by_model: std::collections::HashMap<&str, &PriceEntry> = snapshot
        .entries
        .iter()
        .map(|e| (e.model.as_str(), e))
        .collect();

    // sample_spec、embedding 模式、纯零价条目都应被跳过。
    assert!(!by_model.contains_key("sample_spec"));
    assert!(!by_model.contains_key("text-embedding-3-small"));
    assert!(!by_model.contains_key("free-local-model"));

    // 归一后 provider 一律为空，充当按模型兜底。
    let gpt = by_model.get("gpt-4o").expect("gpt-4o present");
    assert_eq!(gpt.provider, None);
    assert_eq!(gpt.input, 2.5e-06);
    assert_eq!(gpt.output, 1e-05);
    assert_eq!(gpt.cache_read, 1.25e-06);

    // 同一模型同时有裸键与带前缀键时，只保留裸键那条（无 cache_creation）。
    let claude = by_model.get("claude-3-5-sonnet").expect("claude present");
    assert_eq!(claude.provider, None);
    assert_eq!(claude.cache_creation, 0.0);
    // 去重后每个模型只有一条。
    assert_eq!(
        snapshot
            .entries
            .iter()
            .filter(|e| e.model == "claude-3-5-sonnet")
            .count(),
        1
    );
}

#[test]
fn litellm_merge_lets_user_prices_win_and_fills_the_rest() {
    let snapshot = crate::litellm::parse_litellm_raw(LITELLM_RAW_SAMPLE, "2026-08-17").unwrap();
    let user = PriceTable {
        prices: vec![PriceEntry {
            model: "gpt-4o".into(),
            provider: None,
            input: 9.9,
            output: 9.9,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }],
    };
    let merged = crate::litellm::merge(&user, &snapshot);

    // 用户配置过的 gpt-4o 不被快照覆盖，只保留用户那条。
    let gpt: Vec<&PriceEntry> = merged
        .prices
        .iter()
        .filter(|e| e.model == "gpt-4o")
        .collect();
    assert_eq!(gpt.len(), 1);
    assert_eq!(gpt[0].input, 9.9);
    assert_eq!(gpt[0].origin, PriceOrigin::User);
    // 用户没配的模型由快照补齐，并打上 snapshot 来源。
    let claude = merged
        .prices
        .iter()
        .find(|e| e.model == "claude-3-5-sonnet")
        .expect("snapshot fills missing model");
    assert_eq!(claude.origin, PriceOrigin::Snapshot);
}

#[test]
fn litellm_snapshot_fills_cost_for_models_without_native_or_user_price() {
    let snapshot = crate::litellm::parse_litellm_raw(LITELLM_RAW_SAMPLE, "2026-08-17").unwrap();
    // 空的用户单价表：完全依赖快照兜底。
    let effective = crate::litellm::merge(&PriceTable::default(), &snapshot);

    // Codex 类记录：无 native_cost、provider 为空，模型名与快照一致。
    let mut record = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "gpt-4o",
        "",
        "/proj/a",
        "s1",
        0,
    );
    record.input_tokens = 1_000_000;
    record.output_tokens = 1_000_000;

    let derived = derive_cost(&record, &effective);
    assert!(!derived.unpriced, "快照应把该模型标记为已定价");
    assert!(!derived.source_native, "快照兜底不是来源自带费用");
    assert_eq!(derived.cost_source, CostSource::Snapshot);
    assert_eq!(derived.amount, Some(2.5 + 10.0));

    // 有来源自带费用时优先 native。
    let native = UsageRecord {
        native_cost: Some(4.2),
        ..record.clone()
    };
    let native_derived = derive_cost(&native, &effective);
    assert_eq!(native_derived.amount, Some(4.2));
    assert_eq!(native_derived.cost_source, CostSource::Native);

    // 快照没有的模型仍然是未定价。
    let unknown = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "totally-unknown-model",
        "",
        "/proj/a",
        "s2",
        100,
    );
    let unknown_derived = derive_cost(&unknown, &effective);
    assert!(unknown_derived.unpriced);
    assert_eq!(unknown_derived.cost_source, CostSource::None);
}

#[test]
fn cost_source_labels_native_user_snapshot_and_none_on_sql_and_memory() {
    let snapshot = crate::litellm::parse_litellm_raw(LITELLM_RAW_SAMPLE, "2026-08-17").unwrap();
    let user = PriceTable {
        prices: vec![PriceEntry {
            model: "user-only-model".into(),
            provider: None,
            input: 0.001,
            output: 0.0,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }],
    };
    let prices = crate::litellm::merge(&user, &snapshot);

    let mut native = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "gpt-4o",
        "",
        "/proj/a",
        "s-native",
        0,
    );
    native.native_cost = Some(1.25);
    native.input_tokens = 10;

    let mut user_priced = rec(
        "2026-08-01T10:01:00Z",
        Source::Codex,
        "user-only-model",
        "",
        "/proj/a",
        "s-user",
        0,
    );
    user_priced.input_tokens = 1000;

    let mut snapshot_priced = rec(
        "2026-08-01T10:02:00Z",
        Source::Codex,
        "gpt-4o",
        "",
        "/proj/a",
        "s-snapshot",
        0,
    );
    snapshot_priced.input_tokens = 1_000_000;

    let unpriced = rec(
        "2026-08-01T10:03:00Z",
        Source::Codex,
        "totally-unknown-model",
        "",
        "/proj/a",
        "s-none",
        0,
    );

    let records = vec![
        native.clone(),
        user_priced.clone(),
        snapshot_priced.clone(),
        unpriced.clone(),
    ];
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();

    let cases = [
        ("s-native", CostSource::Native, "来源自带", Some(1.25)),
        ("s-user", CostSource::User, "用户单价", Some(1.0)),
        (
            "s-snapshot",
            CostSource::Snapshot,
            "LiteLLM 快照",
            Some(2.5),
        ),
        ("s-none", CostSource::None, "单价未配置", None),
    ];
    for (session_id, source, note, cost) in cases {
        let mem = aggregate::session_turns(
            &records,
            session_id,
            Some("codex"),
            &Filter::default(),
            &prices,
        );
        let sql = query::session_turns(
            &conn,
            session_id,
            Some("codex"),
            &Filter::default(),
            &prices,
        )
        .unwrap();
        assert_eq!(mem, sql, "session_turns cost_source 不一致：{session_id}");
        assert_eq!(mem[0].cost_source, source);
        assert_eq!(mem[0].cost_note.as_deref(), Some(note));
        assert_eq!(mem[0].cost, cost);
    }
}

#[test]
fn price_entry_origin_defaults_to_user_for_legacy_json() {
    let table: PriceTable = serde_json::from_str(
        r#"{"prices":[{"model":"gpt-4o","provider":null,"input":1.0,"output":2.0,"cache_read":0.0,"cache_creation":0.0}]}"#,
    )
    .unwrap();
    assert_eq!(table.prices[0].origin, PriceOrigin::User);
    let encoded = serde_json::to_string(&table).unwrap();
    assert!(
        !encoded.contains("origin"),
        "用户单价序列化不应写出默认 origin：{encoded}"
    );
}

#[test]
fn bundled_litellm_snapshot_is_valid_and_covers_common_models() {
    let bundled = crate::litellm::bundled_snapshot();
    assert!(
        bundled.entries.len() > 200,
        "内置快照应包含大量模型，实际 {}",
        bundled.entries.len()
    );
    assert_eq!(bundled.source, "litellm");
    let models: std::collections::HashSet<&str> =
        bundled.entries.iter().map(|e| e.model.as_str()).collect();
    for expected in ["gpt-4o", "claude-3-5-sonnet-20241022", "gemini-2.5-pro"] {
        assert!(models.contains(expected), "内置快照缺少常见模型 {expected}");
    }
    // 所有条目都应有非零单价（生成阶段已过滤零价）。
    assert!(bundled
        .entries
        .iter()
        .all(|e| e.input > 0.0 || e.output > 0.0));
}

fn cursor_event(model: &str, input: i64, output: i64) -> crate::domain::CursorUsageEvent {
    crate::domain::CursorUsageEvent {
        occurred_at: "2026-08-16T10:00:00Z".into(),
        model: model.into(),
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        is_headless: false,
    }
}

#[test]
fn cursor_costs_match_reordered_and_suffixed_litellm_names() {
    let prices = PriceTable {
        prices: vec![
            PriceEntry {
                model: "claude-sonnet-4-6".into(),
                provider: None,
                input: 3.0 / 1_000_000.0,
                output: 15.0 / 1_000_000.0,
                cache_read: 0.0,
                cache_creation: 0.0,
                origin: PriceOrigin::Snapshot,
            },
            PriceEntry {
                model: "claude-4.5-sonnet".into(),
                provider: None,
                input: 3.0 / 1_000_000.0,
                output: 15.0 / 1_000_000.0,
                cache_read: 0.0,
                cache_creation: 0.0,
                origin: PriceOrigin::Snapshot,
            },
            PriceEntry {
                model: "gpt-5".into(),
                provider: None,
                input: 1.25 / 1_000_000.0,
                output: 10.0 / 1_000_000.0,
                cache_read: 0.0,
                cache_creation: 0.0,
                origin: PriceOrigin::Snapshot,
            },
            PriceEntry {
                model: "gpt-5-mini".into(),
                provider: None,
                input: 0.25 / 1_000_000.0,
                output: 2.0 / 1_000_000.0,
                cache_read: 0.0,
                cache_creation: 0.0,
                origin: PriceOrigin::Snapshot,
            },
        ],
    };

    let reordered = cursor_event("claude-4.6-sonnet", 1_000_000, 0);
    let (cost, unpriced) = crate::cost::sum_cursor_event_costs(&[&reordered], &prices);
    assert!(!unpriced);
    assert!((cost.expect("reordered") - 3.0).abs() < 1e-9);

    let thinking = cursor_event("claude-4.5-sonnet-thinking", 1_000_000, 0);
    let (cost, unpriced) = crate::cost::sum_cursor_event_costs(&[&thinking], &prices);
    assert!(!unpriced);
    assert!((cost.expect("thinking suffix") - 3.0).abs() < 1e-9);

    let gpt_high = cursor_event("gpt-5-high", 1_000_000, 0);
    let (cost, unpriced) = crate::cost::sum_cursor_event_costs(&[&gpt_high], &prices);
    assert!(!unpriced);
    assert!((cost.expect("gpt-5-high should use gpt-5, not mini") - 1.25).abs() < 1e-9);

    let composer = cursor_event("composer-2", 1_000_000, 0);
    let (cost, unpriced) = crate::cost::sum_cursor_event_costs(&[&composer], &prices);
    assert!(unpriced);
    assert_eq!(cost, None);
}

#[test]
fn cursor_signature_match_prefers_user_price_over_snapshot() {
    let prices = PriceTable {
        prices: vec![
            PriceEntry {
                model: "claude-sonnet-4-6".into(),
                provider: None,
                input: 9.0 / 1_000_000.0,
                output: 0.0,
                cache_read: 0.0,
                cache_creation: 0.0,
                origin: PriceOrigin::User,
            },
            PriceEntry {
                model: "claude-sonnet-4.6".into(),
                provider: None,
                input: 3.0 / 1_000_000.0,
                output: 0.0,
                cache_read: 0.0,
                cache_creation: 0.0,
                origin: PriceOrigin::Snapshot,
            },
        ],
    };
    let event = cursor_event("claude-4.6-sonnet", 1_000_000, 0);
    let (cost, unpriced) = crate::cost::sum_cursor_event_costs(&[&event], &prices);
    assert!(!unpriced);
    assert!((cost.expect("user price wins") - 9.0).abs() < 1e-9);
}

#[test]
fn usage_record_costs_do_not_use_signature_fallback() {
    let prices = PriceTable {
        prices: vec![PriceEntry {
            model: "claude-sonnet-4-6".into(),
            provider: None,
            input: 3.0 / 1_000_000.0,
            output: 0.0,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::Snapshot,
        }],
    };
    let mut record = rec(
        "2026-08-16T10:00:00Z",
        Source::Claude,
        "claude-4.6-sonnet",
        "",
        "/proj/a",
        "s1",
        0,
    );
    record.input_tokens = 1_000_000;
    let derived = derive_cost(&record, &prices);
    assert!(derived.unpriced);
    assert_eq!(derived.amount, None);
}

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
fn backup_and_restore_round_trips_records_and_user_config() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();

    let db_path = live.join("usage.sqlite");
    let prices_path = live.join("prices.json");
    let snapshot_path = live.join("litellm_prices.json");
    let budget_path = live.join("budget.json");
    let budget_notify_path = live.join("budget_notify_state.json");
    let paths = backup::AppDataPaths {
        db_path: db_path.clone(),
        prices_path: prices_path.clone(),
        snapshot_path: snapshot_path.clone(),
        budget_path: budget_path.clone(),
        budget_notify_path: budget_notify_path.clone(),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    };

    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-18T00:00:00.000Z",
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj",
            "s1",
            42,
        )],
    )
    .unwrap();

    let prices = PriceTable {
        prices: vec![PriceEntry {
            model: "claude-sonnet-5".into(),
            provider: Some("anthropic".into()),
            input: 0.003,
            output: 0.015,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }],
    };
    std::fs::write(&prices_path, serde_json::to_string_pretty(&prices).unwrap()).unwrap();
    budget::save_config(
        &budget_path,
        &BudgetConfig {
            monthly_usd: Some(20.0),
        },
    )
    .unwrap();
    budget::save_notify_state(
        &budget_notify_path,
        &budget::NotifyState {
            month: "2026-08".into(),
            notified: vec![50, 80],
        },
    )
    .unwrap();
    std::fs::write(
        &snapshot_path,
        r#"{"as_of":"2026-01-01","source":"test","entries":[]}"#,
    )
    .unwrap();

    let manifest = backup::backup_to(&conn, &dest, &paths).unwrap();
    assert!(manifest.files.contains(&"usage.sqlite".to_string()));
    assert!(manifest.files.contains(&"prices.json".to_string()));
    assert!(manifest.files.contains(&"budget.json".to_string()));
    assert!(manifest
        .files
        .contains(&"budget_notify_state.json".to_string()));
    assert!(manifest.note.contains("钥匙串"));
    drop(conn);

    std::fs::write(&prices_path, "{\"prices\":[]}").unwrap();
    budget::save_config(&budget_path, &BudgetConfig { monthly_usd: None }).unwrap();
    budget::save_notify_state(&budget_notify_path, &budget::NotifyState::default()).unwrap();
    std::fs::remove_file(&db_path).unwrap();
    let _ = std::fs::remove_file(live.join("usage.sqlite-wal"));
    let _ = std::fs::remove_file(live.join("usage.sqlite-shm"));

    backup::restore_from(&dest, &paths).unwrap();
    let restored = store::open_db(db_path.to_str().unwrap()).unwrap();
    let rows = store::load_all(&restored).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].total_tokens, 42);
    assert_eq!(budget::load_config(&budget_path).monthly_usd, Some(20.0));
    assert_eq!(
        budget::load_notify_state(&budget_notify_path),
        budget::NotifyState {
            month: "2026-08".into(),
            notified: vec![50, 80],
        }
    );
    let restored_prices: PriceTable =
        serde_json::from_str(&std::fs::read_to_string(&prices_path).unwrap()).unwrap();
    assert_eq!(restored_prices.prices[0].model, "claude-sonnet-5");
}

#[test]
fn restore_rejects_invalid_backup_without_touching_live_files() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    let db_path = live.join("usage.sqlite");
    let prices_path = live.join("prices.json");
    let snapshot_path = live.join("litellm_prices.json");
    let budget_path = live.join("budget.json");
    let paths = backup::AppDataPaths {
        db_path: db_path.clone(),
        prices_path: prices_path.clone(),
        snapshot_path,
        budget_path,
        budget_notify_path: live.join("budget_notify_state.json"),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    };

    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-18T00:00:00.000Z",
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj",
            "s1",
            42,
        )],
    )
    .unwrap();
    drop(conn);
    std::fs::write(&prices_path, "{\"prices\":[]}").unwrap();

    assert!(backup::validate_restore(&dest).is_err());
    assert!(backup::restore_from(&dest, &paths).is_err());

    std::fs::write(
        dest.join("manifest.json"),
        "{\"created_at\":\"x\",\"files\":[],\"note\":\"\"}",
    )
    .unwrap();
    assert!(
        backup::restore_from(&dest, &paths)
            .unwrap_err()
            .contains("usage.sqlite"),
        "missing sqlite should fail before overwrite"
    );

    let still = store::open_db(db_path.to_str().unwrap()).unwrap();
    assert_eq!(store::load_all(&still).unwrap()[0].total_tokens, 42);
    assert_eq!(
        std::fs::read_to_string(&prices_path).unwrap(),
        "{\"prices\":[]}"
    );
}

#[test]
fn restore_rolls_back_live_files_when_a_later_replace_fails() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();

    let db_path = live.join("usage.sqlite");
    let prices_path = live.join("prices.json");
    let snapshot_path = live.join("litellm_prices.json");
    let budget_path = live.join("budget.json");
    let paths = backup::AppDataPaths {
        db_path: db_path.clone(),
        prices_path: prices_path.clone(),
        snapshot_path,
        budget_path,
        budget_notify_path: live.join("budget_notify_state.json"),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    };

    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-18T00:00:00.000Z",
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj",
            "s1",
            42,
        )],
    )
    .unwrap();
    std::fs::write(&prices_path, "{\"prices\":[]}").unwrap();
    backup::backup_to(&conn, &dest, &paths).unwrap();
    drop(conn);

    std::fs::remove_file(&prices_path).unwrap();
    std::fs::create_dir(&prices_path).unwrap();

    let error = backup::restore_from(&dest, &paths).unwrap_err();
    assert!(error.contains("写入") || error.contains("失败"), "{error}");

    let still = store::open_db(db_path.to_str().unwrap()).unwrap();
    assert_eq!(
        store::load_all(&still).unwrap()[0].total_tokens,
        42,
        "db should roll back when a later file cannot be replaced"
    );
}

fn backup_paths(live: &std::path::Path) -> backup::AppDataPaths {
    backup::AppDataPaths {
        db_path: live.join("usage.sqlite"),
        prices_path: live.join("prices.json"),
        snapshot_path: live.join("litellm_prices.json"),
        budget_path: live.join("budget.json"),
        budget_notify_path: live.join("budget_notify_state.json"),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    }
}

#[test]
fn backup_omits_conversation_event_bodies_and_restore_reads_via_fallback() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();
    write_home_fixture(
        &home,
        ".codex/sessions/2026/08/rollout-conv-1.jsonl",
        "codex-conversation.jsonl",
    );
    let paths = backup_paths(&live);
    let conn = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    crate::conversation::refresh_codex(&conn, &home).unwrap();
    let live_events = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();
    assert!(!live_events.is_empty());
    assert!(live_events
        .iter()
        .any(|event| event.text.as_deref() == Some("我先检查现有实现。")));

    let manifest = backup::backup_to(&conn, &dest, &paths).unwrap();
    assert!(manifest.note.contains("对话"));
    drop(conn);

    let backup_db = rusqlite::Connection::open(dest.join(backup::DB_NAME)).unwrap();
    let has_events_table: bool = backup_db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'conversation_events')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!has_events_table, "备份产物不得包含事件索引表");
    let generations: i64 = backup_db
        .query_row(
            "SELECT COUNT(*) FROM conversation_sessions WHERE event_index_generation IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(generations, 0, "备份不得留下可被当成已索引的代次");
    let raw = std::fs::read(dest.join(backup::DB_NAME)).unwrap();
    let raw = String::from_utf8_lossy(&raw);
    assert!(
        !raw.contains("我先检查现有实现。"),
        "VACUUM 后备份文件不得残留对话正文"
    );

    std::fs::remove_file(&paths.db_path).unwrap();
    let _ = std::fs::remove_file(live.join("usage.sqlite-wal"));
    let _ = std::fs::remove_file(live.join("usage.sqlite-shm"));
    backup::restore_from(&dest, &paths).unwrap();

    let restored = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    assert!(
        crate::conversation::indexed_events(&restored, "codex", "conv-1")
            .unwrap()
            .is_empty()
    );
    let fallback = crate::conversation::load_detail(&restored, &home, "codex", "conv-1").unwrap();
    assert!(
        fallback
            .events
            .iter()
            .any(|event| event.text.as_deref() == Some("我先检查现有实现。")),
        "恢复后未索引会话必须经回退路径读到正确内容"
    );
    assert!(fallback
        .events
        .iter()
        .any(|event| event.text.as_deref() == Some("已完成提交。")));

    crate::conversation::backfill_event_index(&restored, &home).unwrap();
    assert_conversation_index_matches_parse(&restored, &home, "codex", "conv-1");
}

#[test]
fn restore_accepts_legacy_backup_without_conversation_events_table() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    let paths = backup_paths(&live);

    let source = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    store::insert_records(
        &source,
        &[rec(
            "2026-08-18T00:00:00.000Z",
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj",
            "s1",
            42,
        )],
    )
    .unwrap();
    backup::backup_to(&source, &dest, &paths).unwrap();
    drop(source);

    let backup_db = rusqlite::Connection::open(dest.join(backup::DB_NAME)).unwrap();
    backup_db
        .execute_batch("DROP TABLE IF EXISTS conversation_events;")
        .unwrap();
    drop(backup_db);

    backup::validate_restore(&dest).unwrap();
    std::fs::remove_file(&paths.db_path).unwrap();
    backup::restore_from(&dest, &paths).unwrap();
    let restored = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    assert_eq!(store::load_all(&restored).unwrap()[0].total_tokens, 42);
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
