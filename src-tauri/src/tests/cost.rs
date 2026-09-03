use crate::cost::{derive_cost, snapshot_price_candidate, sum_costs};
use crate::domain::{PriceEntry, PriceOrigin, PriceTable, Source};
use crate::test_support::*;

fn entry(model: &str, input: f64, origin: PriceOrigin) -> PriceEntry {
    PriceEntry {
        model: model.into(),
        provider: None,
        input,
        output: 0.0,
        cache_read: 0.0,
        cache_creation: 0.0,
        origin,
    }
}

#[test]
fn snapshot_candidate_none_when_exact_price_exists() {
    let prices = PriceTable {
        prices: vec![entry(
            "claude-4.6-sonnet",
            3.0 / 1_000_000.0,
            PriceOrigin::Snapshot,
        )],
    };
    assert_eq!(
        snapshot_price_candidate("claude-4.6-sonnet", &prices),
        None,
        "精确命中快照时不应再给候选"
    );
    assert_eq!(
        snapshot_price_candidate("Claude-4.6-Sonnet", &prices),
        None,
        "大小写不同仍是精确价"
    );
}

#[test]
fn snapshot_candidate_from_signature_compatible_snapshot() {
    let prices = PriceTable {
        prices: vec![entry(
            "claude-sonnet-4-6",
            3.0 / 1_000_000.0,
            PriceOrigin::Snapshot,
        )],
    };
    let got =
        snapshot_price_candidate("claude-4.6-sonnet", &prices).expect("签名兼容的快照应给出候选");
    assert_eq!(got.model, "claude-sonnet-4-6");
    assert_eq!(got.origin, PriceOrigin::Snapshot);
    assert!((got.input - 3.0 / 1_000_000.0).abs() < 1e-12);
    assert_eq!(got.output, 0.0);
    assert_eq!(got.cache_read, 0.0);
    assert_eq!(got.cache_creation, 0.0);
    assert_eq!(got.provider, None);
}

#[test]
fn snapshot_candidate_prefers_user_price_over_snapshot() {
    let prices = PriceTable {
        prices: vec![
            entry("claude-sonnet-4-6", 9.0 / 1_000_000.0, PriceOrigin::User),
            entry(
                "claude-sonnet-4.6",
                3.0 / 1_000_000.0,
                PriceOrigin::Snapshot,
            ),
        ],
    };
    let got =
        snapshot_price_candidate("claude-4.6-sonnet", &prices).expect("用户价目应优先于快照候选");
    assert_eq!(got.origin, PriceOrigin::User);
    assert!((got.input - 9.0 / 1_000_000.0).abs() < 1e-12);
}

#[test]
fn snapshot_candidate_none_on_complete_miss() {
    let prices = PriceTable {
        prices: vec![entry("gpt-5", 1.25 / 1_000_000.0, PriceOrigin::Snapshot)],
    };
    assert_eq!(
        snapshot_price_candidate("composer-2", &prices),
        None,
        "家族对不上时应返回空"
    );
    assert_eq!(
        snapshot_price_candidate("", &prices),
        None,
        "空模型名不应产生候选"
    );
}

#[test]
fn usage_record_lookup_still_ignores_signature_after_candidate_helper() {
    let prices = PriceTable {
        prices: vec![entry(
            "claude-sonnet-4-6",
            3.0 / 1_000_000.0,
            PriceOrigin::Snapshot,
        )],
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
    record.total_tokens = 1_000_000;
    let derived = derive_cost(&record, &prices);
    assert!(derived.unpriced);
    assert_eq!(derived.amount, None);
    let (sum, unpriced) = sum_costs(&[&record], &prices);
    assert_eq!(sum, None);
    assert!(unpriced);
    assert!(
        snapshot_price_candidate(&record.model, &prices).is_some(),
        "诊断路径仍应给出候选，但不能改变消耗记录取价"
    );
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
