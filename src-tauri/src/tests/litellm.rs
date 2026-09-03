use crate::test_support::*;

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
