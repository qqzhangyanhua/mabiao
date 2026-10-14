//! 消耗构成、明细筛选、会话行为指标（连真 PostgreSQL）。

mod common;

use axum::http::{Method, StatusCode};
use axum::Router;
use common::*;
use push_protocol::{
    usage_fingerprint, DeviceInfo, EventActor, EventKind, EventPayload, PricingSource,
    PushSessionRequest, PushUsageRequest, SessionPayload, UsageRecordPayload, UsageTokens,
    PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use sqlx::PgPool;

fn device() -> DeviceInfo {
    DeviceInfo {
        device_id: "d1".into(),
        device_name: "办公室".into(),
    }
}

fn tokens(input: i64, output: i64, cache_read: i64, reasoning: i64) -> UsageTokens {
    UsageTokens {
        input,
        output,
        cache_read,
        cache_creation: 0,
        reasoning,
        total: input + output + cache_read + reasoning,
    }
}

fn record(
    at: &str,
    source: &str,
    model: &str,
    session_id: &str,
    native: Option<f64>,
    parts: UsageTokens,
) -> UsageRecordPayload {
    UsageRecordPayload {
        fingerprint: usage_fingerprint(source, "/f.jsonl", at, model, &parts),
        occurred_at: at.into(),
        source: source.into(),
        model: model.into(),
        provider: "openai".into(),
        project: "/w/mabiao".into(),
        session_id: session_id.into(),
        source_file: "/f.jsonl".into(),
        tokens: parts,
        native_cost: native,
        cost_snapshot: Some(1.0),
        pricing_source: PricingSource::Exact,
    }
}

async fn push_usage(app: &Router, token: &str, records: Vec<UsageRecordPayload>) {
    let (status, body) = call(
        app,
        Method::POST,
        "/api/v1/push/usage",
        Some(token),
        Some(
            serde_json::to_value(PushUsageRequest {
                protocol_version: PROTOCOL_VERSION,
                device: device(),
                records,
            })
            .unwrap(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn push_session(
    app: &Router,
    token: &str,
    id: &str,
    ended_at: &str,
    events: Vec<EventPayload>,
) {
    let session = SessionPayload {
        source: "codex".into(),
        session_id: id.into(),
        title: id.into(),
        project: "/w/mabiao".into(),
        git_remote_url: Some("git@github.com:team/mabiao.git".into()),
        model: "team-model".into(),
        started_at: "2026-03-01T09:00:00Z".into(),
        ended_at: ended_at.into(),
        source_files: vec!["/s/a.jsonl".into()],
        generated_by_work_notes: false,
        redaction_count: 0,
        events,
        context_manifest: None,
    };
    let (status, body) = call(
        app,
        Method::POST,
        "/api/v1/push/session",
        Some(token),
        Some(
            serde_json::to_value(PushSessionRequest {
                protocol_version: PROTOCOL_VERSION,
                device: device(),
                session,
            })
            .unwrap(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

fn event(id: &str, kind: EventKind, actor: Option<EventActor>, name: Option<&str>) -> EventPayload {
    EventPayload {
        event_id: id.into(),
        sequence: 0,
        source_file: "/s/a.jsonl".into(),
        source_sequence: 0,
        kind,
        occurred_at: None,
        actor,
        name: name.map(str::to_owned),
        text: None,
        details: json!({}),
    }
}

async fn get(app: &Router, token: &str, path: &str) -> (StatusCode, Value) {
    call(app, Method::GET, path, Some(token), None).await
}

fn bucket<'a>(list: &'a Value, key: &str) -> &'a Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key"] == key)
        .unwrap_or_else(|| panic!("没有 {key}：{list}"))
}

async fn setup(pool: &PgPool) -> (Router, String, String, String) {
    seed_admin(pool, "root").await;
    seed_member(pool, "alice").await;
    seed_member(pool, "bob").await;
    let app = app(pool);
    let admin = token_of(&app, "root").await;
    let alice = token_of(&app, "alice").await;
    let bob = token_of(&app, "bob").await;
    let (status, body) = call(
        &app,
        Method::PUT,
        "/api/v1/admin/pricing/prices",
        Some(&admin),
        Some(json!({
            "model": "team-model", "provider": null,
            "input": 0.000002, "output": 0.0, "cache_read": 0.0, "cache_creation": 0.0,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    push_usage(
        &app,
        &alice,
        vec![
            record(
                "2026-03-02T10:00:00Z",
                "codex",
                "native-model",
                "s-1",
                Some(3.0),
                tokens(10, 4, 6, 1),
            ),
            record(
                "2026-03-02T11:00:00Z",
                "claude",
                "missing-model-zz",
                "s-2",
                None,
                tokens(100, 0, 0, 0),
            ),
        ],
    )
    .await;
    push_usage(
        &app,
        &bob,
        vec![record(
            "2026-03-02T12:00:00Z",
            "codex",
            "team-model",
            "s-bob",
            None,
            tokens(7, 0, 0, 0),
        )],
    )
    .await;
    push_session(
        &app,
        &alice,
        "s-1",
        "2026-03-02T10:30:00Z",
        vec![
            event(
                "e1",
                EventKind::ToolCall,
                Some(EventActor::Assistant),
                Some("Bash"),
            ),
            event(
                "e2",
                EventKind::ToolCall,
                Some(EventActor::Assistant),
                Some("Read"),
            ),
            event("e3", EventKind::Error, Some(EventActor::Tool), Some("Bash")),
            event("e4", EventKind::Error, Some(EventActor::Assistant), None),
            event(
                "e5",
                EventKind::ToolCall,
                Some(EventActor::Assistant),
                Some(""),
            ),
        ],
    )
    .await;
    push_session(
        &app,
        &alice,
        "old",
        "2026-02-01T10:00:00Z",
        vec![event(
            "old",
            EventKind::ToolCall,
            Some(EventActor::Assistant),
            Some("Hidden"),
        )],
    )
    .await;
    push_session(
        &app,
        &bob,
        "s-bob",
        "2026-03-02T12:00:00Z",
        vec![event(
            "bob",
            EventKind::ToolCall,
            Some(EventActor::Assistant),
            Some("BobTool"),
        )],
    )
    .await;
    (app, admin, alice, bob)
}

#[sqlx::test]
async fn composition_is_complete_and_usage_can_filter_by_cost_source(pool: PgPool) {
    let (app, admin, alice, bob) = setup(&pool).await;
    let range = "from=2026-03-01T00:00:00Z&to=2026-03-03T00:00:00Z";
    let (status, body) = get(&app, &admin, &format!("/api/v1/usage/composition?{range}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["totals"]["input"], 117);
    assert_eq!(body["totals"]["output"], 4);
    assert_eq!(body["totals"]["cache_read"], 6);
    assert_eq!(body["totals"]["reasoning"], 1);
    assert_eq!(bucket(&body["by_source"], "codex")["input"], 17);
    assert_eq!(bucket(&body["by_model"], "missing-model-zz")["input"], 100);
    assert_eq!(body["unpriced_models"][0]["model"], "missing-model-zz");
    assert_eq!(body["unpriced_models"][0]["count"], 1);

    let (status, listed) = get(
        &app,
        &admin,
        &format!("/api/v1/usage?{range}&unified_cost_source=native"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["totals"]["record_count"], 1);
    assert_eq!(listed["records"][0]["model"], "native-model");
    assert_eq!(listed["records"][0]["unified_cost_source"], "native");
    assert!(listed["records"][0]["session_pk"].is_i64(), "{listed}");

    let (status, _) = get(&app, &admin, "/api/v1/usage?unified_cost_source=bogus").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, mine) = get(&app, &alice, &format!("/api/v1/usage/composition?{range}")).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(mine["totals"]["input"], 110);

    let bob_id: i64 = sqlx::query_scalar("SELECT id FROM remote_accounts WHERE account = 'bob'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (status, _) = get(
        &app,
        &alice,
        &format!("/api/v1/usage/composition?account_id={bob_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let _ = bob;
}

#[sqlx::test]
async fn behavior_counts_tool_failures_inside_the_session_window(pool: PgPool) {
    let (app, admin, alice, _) = setup(&pool).await;
    let (status, body) = get(
        &app,
        &admin,
        "/api/v1/sessions/behavior?from=2026-03-01T00:00:00Z&to=2026-03-03T00:00:00Z",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["tool_calls"], 4);
    assert_eq!(body["tool_failures"], 1);
    assert_eq!(body["other_errors"], 1);
    assert!((body["failure_rate"].as_f64().unwrap() - 0.25).abs() < 1e-12);
    let bash = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "Bash")
        .unwrap();
    assert_eq!(bash["calls"], 1);
    assert_eq!(bash["failures"], 1);
    assert!(body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "未命名" && tool["calls"] == 1));
    assert!(body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .all(|tool| tool["name"] != "Hidden"));

    let (status, mine) = get(
        &app,
        &alice,
        "/api/v1/sessions/behavior?from=2026-03-01T00:00:00Z&to=2026-03-03T00:00:00Z",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(mine["tool_calls"], 3);
    let bob_id: i64 = sqlx::query_scalar("SELECT id FROM remote_accounts WHERE account = 'bob'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (status, _) = get(
        &app,
        &alice,
        &format!("/api/v1/sessions/behavior?account_id={bob_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
