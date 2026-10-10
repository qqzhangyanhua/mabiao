//! 团队价目与统一费用：同 token 同价、价目变更后重算、内置快照兜底、只有管理员能改（连真 PostgreSQL）。

mod common;

use axum::http::{Method, StatusCode};
use axum::Router;
use common::*;
use push_protocol::{
    usage_fingerprint, DeviceInfo, PricingSource, PushUsageRequest, UsageRecordPayload,
    UsageTokens, PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use sqlx::PgPool;

/// 内置快照里 provider 为空的条目：input 3e-6、output 7e-6、缓存为 0。
const SNAPSHOT_MODEL: &str = "DeepSeek-R1";
/// 快照里只有这一种大小写写法的模型。桌面端 merge 按大小写敏感排除快照：
/// 像 `DeepSeek-R1` / `deepseek-r1` 两种写法并存的，团队只配一种，另一种仍会兜底。
const ISOLATED_SNAPSHOT_MODEL: &str = "CodeLlama-34b-Instruct-hf";

fn device(id: &str) -> DeviceInfo {
    DeviceInfo {
        device_id: id.into(),
        device_name: format!("{id} 的电脑"),
    }
}

struct Spec<'a> {
    file: &'a str,
    model: &'a str,
    provider: &'a str,
    input: i64,
    output: i64,
    native_cost: Option<f64>,
    client_cost: Option<f64>,
    client_source: PricingSource,
}

impl<'a> Spec<'a> {
    fn new(file: &'a str, model: &'a str) -> Self {
        Self {
            file,
            model,
            provider: "",
            input: 1000,
            output: 500,
            native_cost: None,
            client_cost: Some(9.99),
            client_source: PricingSource::Exact,
        }
    }

    fn record(&self) -> UsageRecordPayload {
        let at = "2026-03-01T10:00:00Z";
        let tokens = UsageTokens {
            input: self.input,
            output: self.output,
            cache_read: 0,
            cache_creation: 0,
            reasoning: 0,
            total: self.input + self.output,
        };
        UsageRecordPayload {
            fingerprint: usage_fingerprint("codex", self.file, at, self.model, &tokens),
            occurred_at: at.into(),
            source: "codex".into(),
            model: self.model.into(),
            provider: self.provider.into(),
            project: "/w/p".into(),
            session_id: "s-1".into(),
            source_file: self.file.into(),
            tokens,
            native_cost: self.native_cost,
            cost_snapshot: self.client_cost,
            pricing_source: self.client_source,
        }
    }
}

async fn push(app: &Router, token: &str, records: Vec<UsageRecordPayload>) {
    let (status, body) = call(
        app,
        Method::POST,
        "/api/v1/push/usage",
        Some(token),
        Some(
            serde_json::to_value(PushUsageRequest {
                protocol_version: PROTOCOL_VERSION,
                device: device("d1"),
                records,
            })
            .unwrap(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn set_price(app: &Router, token: &str, price: Value) -> (StatusCode, Value) {
    call(
        app,
        Method::PUT,
        "/api/v1/admin/pricing/prices",
        Some(token),
        Some(price),
    )
    .await
}

fn team_price(model: &str, provider: Option<&str>, input: f64, output: f64) -> Value {
    json!({
        "model": model,
        "provider": provider,
        "input": input,
        "output": output,
        "cache_read": 0.0,
        "cache_creation": 0.0,
    })
}

async fn usage_list(app: &Router, token: &str, query: &str) -> (StatusCode, Value) {
    call(
        app,
        Method::GET,
        &format!("/api/v1/usage{query}"),
        Some(token),
        None,
    )
    .await
}

async fn all_records(app: &Router, token: &str) -> Vec<Value> {
    let (status, body) = usage_list(app, token, "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["records"].as_array().unwrap().clone()
}

fn close(value: &Value, expected: f64) {
    let got = value
        .as_f64()
        .unwrap_or_else(|| panic!("不是数字：{value}"));
    assert!((got - expected).abs() < 1e-12, "{got} != {expected}");
}

async fn setup(pool: &PgPool) -> (Router, String, String) {
    seed_admin(pool, "root").await;
    seed_member(pool, "alice").await;
    let app = app(pool);
    let admin = token_of(&app, "root").await;
    let alice = token_of(&app, "alice").await;
    (app, admin, alice)
}

// ---------- 统一费用 ----------

#[sqlx::test]
async fn same_tokens_with_different_client_snapshots_get_the_same_unified_cost(pool: PgPool) {
    let (app, admin, alice) = setup(&pool).await;
    seed_member(&pool, "bob").await;
    let bob = token_of(&app, "bob").await;
    set_price(&app, &admin, team_price("team-model", None, 2e-6, 4e-6)).await;

    let mut by_alice = Spec::new("/a.jsonl", "team-model");
    by_alice.client_cost = Some(0.10);
    let mut by_bob = Spec::new("/b.jsonl", "team-model");
    by_bob.client_cost = Some(7.70);
    by_bob.client_source = PricingSource::Fallback;
    push(&app, &alice, vec![by_alice.record()]).await;
    push(&app, &bob, vec![by_bob.record()]).await;

    let records = all_records(&app, &admin).await;
    assert_eq!(records.len(), 2);
    for record in &records {
        // 1000 × 2e-6 + 500 × 4e-6
        close(&record["unified_cost"], 0.004);
        assert_eq!(record["unified_cost_source"], "team");
        assert_eq!(record["unified_pricing_source"], "fallback");
    }
    let mut snapshots: Vec<f64> = records
        .iter()
        .map(|r| r["cost_snapshot"].as_f64().unwrap())
        .collect();
    snapshots.sort_by(f64::total_cmp);
    assert_eq!(snapshots, vec![0.10, 7.70]);
    let sources: Vec<&str> = records
        .iter()
        .map(|r| r["pricing_source"].as_str().unwrap())
        .collect();
    assert!(sources.contains(&"exact") && sources.contains(&"fallback"));
}

#[sqlx::test]
async fn builtin_snapshot_prices_models_the_team_never_priced(pool: PgPool) {
    let (app, _admin, alice) = setup(&pool).await;
    push(
        &app,
        &alice,
        vec![Spec::new("/a.jsonl", SNAPSHOT_MODEL).record()],
    )
    .await;

    let records = all_records(&app, &alice).await;

    // 1000 × 3e-6 + 500 × 7e-6
    close(&records[0]["unified_cost"], 0.0065);
    assert_eq!(records[0]["unified_cost_source"], "snapshot");
    assert_eq!(records[0]["unified_pricing_source"], "fallback");
}

#[sqlx::test]
async fn team_price_for_a_model_replaces_the_snapshot_for_that_model(pool: PgPool) {
    let (app, admin, alice) = setup(&pool).await;
    push(
        &app,
        &alice,
        vec![Spec::new("/a.jsonl", ISOLATED_SNAPSHOT_MODEL).record()],
    )
    .await;
    set_price(
        &app,
        &admin,
        team_price(ISOLATED_SNAPSHOT_MODEL, Some("someone-else"), 1.0, 1.0),
    )
    .await;

    let records = all_records(&app, &alice).await;

    // 团队只给该模型配了别家 provider 的价：这笔既不命中它，也不再回落到快照（与桌面端 merge 一致）。
    assert!(records[0]["unified_cost"].is_null());
    assert_eq!(records[0]["unified_pricing_source"], "unpriced");
    assert_eq!(records[0]["unified_cost_source"], "none");
}

#[sqlx::test]
async fn provider_exact_team_price_beats_model_only_team_price(pool: PgPool) {
    let (app, admin, alice) = setup(&pool).await;
    set_price(&app, &admin, team_price("gpt-5", None, 1e-6, 1e-6)).await;
    set_price(
        &app,
        &admin,
        team_price("gpt-5", Some("openai"), 5e-6, 5e-6),
    )
    .await;
    let mut openai = Spec::new("/a.jsonl", "gpt-5");
    openai.provider = "openai";
    let mut other = Spec::new("/b.jsonl", "gpt-5");
    other.provider = "azure";

    push(&app, &alice, vec![openai.record(), other.record()]).await;

    let records = all_records(&app, &alice).await;
    let by_provider = |p: &str| {
        records
            .iter()
            .find(|r| r["provider"] == p)
            .unwrap_or_else(|| panic!("没有 {p}"))
    };
    close(&by_provider("openai")["unified_cost"], 0.0075);
    assert_eq!(by_provider("openai")["unified_pricing_source"], "exact");
    close(&by_provider("azure")["unified_cost"], 0.0015);
    assert_eq!(by_provider("azure")["unified_pricing_source"], "fallback");
}

#[sqlx::test]
async fn native_cost_wins_over_team_price(pool: PgPool) {
    let (app, admin, alice) = setup(&pool).await;
    set_price(&app, &admin, team_price("team-model", None, 2e-6, 4e-6)).await;
    let mut native = Spec::new("/a.jsonl", "team-model");
    native.native_cost = Some(1.25);

    push(&app, &alice, vec![native.record()]).await;

    let records = all_records(&app, &alice).await;
    close(&records[0]["unified_cost"], 1.25);
    assert_eq!(records[0]["unified_pricing_source"], "native");
    assert_eq!(records[0]["unified_cost_source"], "native");
}

#[sqlx::test]
async fn unknown_model_is_unpriced_and_has_no_unified_cost(pool: PgPool) {
    let (app, _admin, alice) = setup(&pool).await;
    push(
        &app,
        &alice,
        vec![Spec::new("/a.jsonl", "no-such-model-xyz").record()],
    )
    .await;

    let (_, body) = usage_list(&app, &alice, "").await;

    assert!(body["records"][0]["unified_cost"].is_null());
    assert_eq!(body["records"][0]["unified_pricing_source"], "unpriced");
    assert_eq!(body["totals"]["unified_unpriced_count"], 1);
    close(&body["totals"]["unified_cost_total"], 0.0);
}

// ---------- 价目变更后重算 ----------

#[sqlx::test]
async fn changing_a_team_price_recomputes_stored_records_and_keeps_the_client_snapshot(
    pool: PgPool,
) {
    let (app, admin, alice) = setup(&pool).await;
    set_price(&app, &admin, team_price("team-model", None, 2e-6, 4e-6)).await;
    push(
        &app,
        &alice,
        vec![Spec::new("/a.jsonl", "team-model").record()],
    )
    .await;
    close(&all_records(&app, &alice).await[0]["unified_cost"], 0.004);

    let (status, body) = set_price(&app, &admin, team_price("team-model", None, 1e-5, 1e-5)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["recomputed"], 1);
    let record = &all_records(&app, &alice).await[0];
    close(&record["unified_cost"], 0.015);
    close(&record["cost_snapshot"], 9.99);
}

#[sqlx::test]
async fn deleting_a_team_price_falls_back_to_the_snapshot(pool: PgPool) {
    let (app, admin, alice) = setup(&pool).await;
    let (_, created) = set_price(&app, &admin, team_price(SNAPSHOT_MODEL, None, 1.0, 1.0)).await;
    push(
        &app,
        &alice,
        vec![Spec::new("/a.jsonl", SNAPSHOT_MODEL).record()],
    )
    .await;
    close(&all_records(&app, &alice).await[0]["unified_cost"], 1500.0);

    let (status, body) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/admin/pricing/prices/{}", created["price"]["id"]),
        Some(&admin),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["recomputed"], 1);
    let record = &all_records(&app, &alice).await[0];
    close(&record["unified_cost"], 0.0065);
    assert_eq!(record["unified_cost_source"], "snapshot");
}

#[sqlx::test]
async fn a_price_change_only_touches_records_of_that_model(pool: PgPool) {
    let (app, admin, alice) = setup(&pool).await;
    push(
        &app,
        &alice,
        vec![
            Spec::new("/a.jsonl", "team-model").record(),
            Spec::new("/b.jsonl", "other-model").record(),
        ],
    )
    .await;

    let (_, body) = set_price(&app, &admin, team_price("TEAM-MODEL", None, 2e-6, 4e-6)).await;

    assert_eq!(body["recomputed"], 1);
}

#[sqlx::test]
async fn explicit_recompute_repairs_records_without_a_unified_cost(pool: PgPool) {
    let (app, admin, alice) = setup(&pool).await;
    push(
        &app,
        &alice,
        vec![Spec::new("/a.jsonl", SNAPSHOT_MODEL).record()],
    )
    .await;
    sqlx::query(
        "UPDATE usage_records SET unified_cost = NULL, unified_pricing_source = NULL,
                                  unified_cost_source = NULL",
    )
    .execute(&pool)
    .await
    .unwrap();

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/admin/pricing/recompute",
        Some(&admin),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["recomputed"], 1);
    close(&all_records(&app, &alice).await[0]["unified_cost"], 0.0065);
}

#[sqlx::test]
async fn backfill_only_fills_records_that_were_never_computed(pool: PgPool) {
    let (app, admin, alice) = setup(&pool).await;
    set_price(&app, &admin, team_price("team-model", None, 2e-6, 4e-6)).await;
    push(
        &app,
        &alice,
        vec![
            Spec::new("/a.jsonl", "team-model").record(),
            Spec::new("/b.jsonl", SNAPSHOT_MODEL).record(),
        ],
    )
    .await;
    sqlx::query(
        "UPDATE usage_records SET unified_cost = NULL, unified_pricing_source = NULL,
                                  unified_cost_source = NULL
         WHERE model = $1",
    )
    .bind(SNAPSHOT_MODEL)
    .execute(&pool)
    .await
    .unwrap();

    let filled = mabiao_server::team_pricing::backfill_missing(&pool)
        .await
        .unwrap();

    assert_eq!(filled, 1);
    let again = mabiao_server::team_pricing::backfill_missing(&pool)
        .await
        .unwrap();
    assert_eq!(again, 0);
}

// ---------- 价目维护 ----------

#[sqlx::test]
async fn admin_lists_team_prices_and_the_builtin_snapshot_meta(pool: PgPool) {
    let (app, admin, _) = setup(&pool).await;
    set_price(&app, &admin, team_price("team-model", None, 2e-6, 4e-6)).await;

    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/admin/pricing",
        Some(&admin),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["prices"].as_array().unwrap().len(), 1);
    assert_eq!(body["prices"][0]["model"], "team-model");
    assert!(body["prices"][0]["provider"].is_null());
    assert_eq!(body["snapshot"]["source"], "litellm");
    assert!(body["snapshot"]["count"].as_u64().unwrap() > 100);
}

#[sqlx::test]
async fn model_names_match_case_insensitively_and_update_in_place(pool: PgPool) {
    let (app, admin, _) = setup(&pool).await;
    set_price(&app, &admin, team_price("GPT-5", None, 1e-6, 1e-6)).await;

    let (status, body) = set_price(&app, &admin, team_price("gpt-5", None, 2e-6, 2e-6)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, listed) = call(
        &app,
        Method::GET,
        "/api/v1/admin/pricing",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(listed["prices"].as_array().unwrap().len(), 1);
    close(&listed["prices"][0]["input"], 2e-6);
}

#[sqlx::test]
async fn empty_provider_means_model_only(pool: PgPool) {
    let (app, admin, _) = setup(&pool).await;

    let (status, body) = set_price(&app, &admin, team_price("m", Some("  "), 1e-6, 1e-6)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["price"]["provider"].is_null());
}

#[sqlx::test]
async fn invalid_prices_are_rejected(pool: PgPool) {
    let (app, admin, _) = setup(&pool).await;

    for bad in [
        team_price("m", None, -1.0, 1.0),
        team_price("   ", None, 1.0, 1.0),
        team_price("m", None, 1e300, 1.0),
    ] {
        let (status, body) = set_price(&app, &admin, bad.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} → {body}");
    }
    let (_, listed) = call(
        &app,
        Method::GET,
        "/api/v1/admin/pricing",
        Some(&admin),
        None,
    )
    .await;
    assert!(listed["prices"].as_array().unwrap().is_empty());
}

#[sqlx::test]
async fn deleting_an_unknown_team_price_is_not_found(pool: PgPool) {
    let (app, admin, _) = setup(&pool).await;

    let (status, _) = call(
        &app,
        Method::DELETE,
        "/api/v1/admin/pricing/prices/999",
        Some(&admin),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn members_cannot_read_or_change_team_prices(pool: PgPool) {
    let (app, admin, alice) = setup(&pool).await;
    let (_, created) = set_price(&app, &admin, team_price("m", None, 1e-6, 1e-6)).await;
    let id = created["price"]["id"].clone();

    let attempts = [
        (Method::GET, "/api/v1/admin/pricing".to_owned(), None),
        (
            Method::PUT,
            "/api/v1/admin/pricing/prices".to_owned(),
            Some(team_price("m", None, 0.0, 0.0)),
        ),
        (
            Method::DELETE,
            format!("/api/v1/admin/pricing/prices/{id}"),
            None,
        ),
        (
            Method::POST,
            "/api/v1/admin/pricing/recompute".to_owned(),
            None,
        ),
    ];
    for (method, uri, body) in attempts {
        let (status, _) = call(&app, method.clone(), &uri, Some(&alice), body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
        let (status, _) = call(&app, method.clone(), &uri, None, body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri} 未登录");
    }
    let (_, listed) = call(
        &app,
        Method::GET,
        "/api/v1/admin/pricing",
        Some(&admin),
        None,
    )
    .await;
    close(&listed["prices"][0]["input"], 1e-6);
}

// ---------- 查询 ----------

#[sqlx::test]
async fn member_sees_only_own_usage_and_cannot_ask_for_another_account(pool: PgPool) {
    let (app, admin, alice) = setup(&pool).await;
    let bob_id = seed_member(&pool, "bob").await;
    let bob = token_of(&app, "bob").await;
    push(
        &app,
        &alice,
        vec![Spec::new("/a.jsonl", "team-model").record()],
    )
    .await;
    push(
        &app,
        &bob,
        vec![Spec::new("/b.jsonl", "team-model").record()],
    )
    .await;

    let (status, mine) = usage_list(&app, &alice, "").await;
    let (forbidden, _) = usage_list(&app, &alice, &format!("?account_id={bob_id}")).await;
    let (_, as_admin) = usage_list(&app, &admin, &format!("?account_id={bob_id}")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(mine["records"].as_array().unwrap().len(), 1);
    assert_eq!(mine["records"][0]["account"], "alice");
    assert_eq!(forbidden, StatusCode::FORBIDDEN);
    assert_eq!(as_admin["records"].as_array().unwrap().len(), 1);
    assert_eq!(as_admin["records"][0]["account"], "bob");
}

#[sqlx::test]
async fn usage_query_filters_by_time_and_reports_totals_over_the_whole_filter(pool: PgPool) {
    let (app, admin, alice) = setup(&pool).await;
    set_price(&app, &admin, team_price("team-model", None, 2e-6, 4e-6)).await;
    let mut late = Spec::new("/b.jsonl", "team-model").record();
    late.occurred_at = "2026-04-01T10:00:00Z".into();
    late.fingerprint = "late".into();
    push(
        &app,
        &alice,
        vec![Spec::new("/a.jsonl", "team-model").record(), late],
    )
    .await;

    let (_, all) = usage_list(&app, &alice, "").await;
    let (_, march) = usage_list(
        &app,
        &alice,
        "?from=2026-03-01T00:00:00Z&to=2026-04-01T00:00:00Z",
    )
    .await;
    let (_, paged) = usage_list(&app, &alice, "?limit=1").await;

    assert_eq!(all["totals"]["record_count"], 2);
    close(&all["totals"]["unified_cost_total"], 0.008);
    close(&all["totals"]["cost_snapshot_total"], 19.98);
    assert_eq!(all["totals"]["total_tokens"], 3000);
    assert_eq!(march["records"].as_array().unwrap().len(), 1);
    assert_eq!(march["totals"]["record_count"], 1);
    assert_eq!(paged["records"].as_array().unwrap().len(), 1);
    assert_eq!(paged["totals"]["record_count"], 2);
}

#[sqlx::test]
async fn usage_query_rejects_bad_parameters(pool: PgPool) {
    let (app, _admin, alice) = setup(&pool).await;

    for query in [
        "?from=yesterday",
        "?limit=0",
        "?limit=100000",
        "?account_id=abc",
    ] {
        let (status, _) = usage_list(&app, &alice, query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
    let (status, _) = call(&app, Method::GET, "/api/v1/usage", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
