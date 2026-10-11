//! 工作时间线：重叠裁剪、按日消耗、越权（连真 PostgreSQL）。

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

async fn get(app: &Router, token: &str, path: &str) -> (StatusCode, Value) {
    call(app, Method::GET, path, Some(token), None).await
}

async fn push_session(
    app: &Router,
    token: &str,
    id: &str,
    source: &str,
    started_at: &str,
    ended_at: &str,
    project: &str,
) {
    let session = SessionPayload {
        source: source.into(),
        session_id: id.into(),
        title: format!("{id} 标题"),
        project: project.into(),
        git_remote_url: Some("git@github.com:team/mabiao.git".into()),
        model: "team-model".into(),
        started_at: started_at.into(),
        ended_at: ended_at.into(),
        source_files: vec!["/s/a.jsonl".into()],
        generated_by_work_notes: false,
        redaction_count: 0,
        events: vec![EventPayload {
            event_id: "e-0".into(),
            sequence: 0,
            source_file: "/s/a.jsonl".into(),
            source_sequence: 0,
            kind: EventKind::Message,
            occurred_at: None,
            actor: Some(EventActor::User),
            name: None,
            text: Some("正文不该出现在时间线里".into()),
            details: json!({}),
        }],
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

async fn push_usage(
    app: &Router,
    token: &str,
    session_id: &str,
    source: &str,
    at: &str,
    tokens: i64,
    cost: f64,
) {
    let usage_tokens = UsageTokens {
        input: tokens,
        output: 0,
        cache_read: 0,
        cache_creation: 0,
        reasoning: 0,
        total: tokens,
    };
    let record = UsageRecordPayload {
        fingerprint: usage_fingerprint(
            source,
            &format!("/{session_id}.jsonl"),
            at,
            "team-model",
            &usage_tokens,
        ),
        occurred_at: at.into(),
        source: source.into(),
        model: "team-model".into(),
        provider: "".into(),
        project: "/w/mabiao".into(),
        session_id: session_id.into(),
        source_file: format!("/{session_id}.jsonl"),
        tokens: usage_tokens,
        native_cost: None,
        cost_snapshot: Some(cost),
        pricing_source: PricingSource::Exact,
    };
    let (status, body) = call(
        app,
        Method::POST,
        "/api/v1/push/usage",
        Some(token),
        Some(
            serde_json::to_value(PushUsageRequest {
                protocol_version: PROTOCOL_VERSION,
                device: device(),
                records: vec![record],
            })
            .unwrap(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn setup(pool: &PgPool) -> (Router, String, String, String) {
    seed_admin(pool, "root").await;
    seed_member(pool, "alice").await;
    seed_member(pool, "bob").await;
    let app = app(pool);
    let admin = token_of(&app, "root").await;
    let alice = token_of(&app, "alice").await;
    let bob = token_of(&app, "bob").await;
    (app, admin, alice, bob)
}

fn first_seg<'a>(body: &'a Value, day: &str) -> &'a Value {
    body["days"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["day"] == day)
        .unwrap_or_else(|| panic!("没有 {day}：{body}"))["segments"]
        .as_array()
        .unwrap()
        .first()
        .unwrap()
}

#[sqlx::test]
async fn overlapping_session_is_clipped_and_tokens_stay_on_their_day(pool: PgPool) {
    let (app, admin, alice, _) = setup(&pool).await;
    push_session(
        &app,
        &alice,
        "s-night",
        "codex",
        "2026-03-01T22:00:00Z",
        "2026-03-02T02:00:00Z",
        "/w/mabiao",
    )
    .await;
    push_usage(
        &app,
        &alice,
        "s-night",
        "codex",
        "2026-03-01T22:30:00Z",
        100,
        1.0,
    )
    .await;
    push_usage(
        &app,
        &alice,
        "s-night",
        "codex",
        "2026-03-02T01:00:00Z",
        40,
        0.4,
    )
    .await;

    let (status, body) = get(
        &app,
        &admin,
        "/api/v1/work-timeline?from=2026-03-01T00:00:00Z&to=2026-03-03T00:00:00Z",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["segment_count"], 2);
    assert_eq!(body["total_tokens"], 140);
    assert_eq!(body["truncated"], false);
    assert!(!body.to_string().contains("正文不该出现在时间线里"));

    let days = body["days"].as_array().unwrap();
    assert_eq!(days.len(), 2);
    assert_eq!(days[0]["day"], "2026-03-02", "新的一天在上");
    let d2 = first_seg(&body, "2026-03-02");
    assert_eq!(d2["session_id"], "s-night");
    assert_eq!(d2["title"], "s-night 标题");
    assert_eq!(d2["account"], "alice");
    assert_eq!(d2["source"], "codex");
    assert_eq!(d2["total_tokens"], 40);
    assert_eq!(d2["start"], "2026-03-02T00:00:00Z");
    assert_eq!(d2["end"], "2026-03-02T02:00:00Z");
    assert!(d2["id"].is_i64());
    assert!(d2.get("events").is_none());

    let d1 = first_seg(&body, "2026-03-01");
    assert_eq!(d1["total_tokens"], 100);
    assert_eq!(d1["start"], "2026-03-01T22:00:00Z");
}

#[sqlx::test]
async fn session_started_before_from_still_appears_when_it_overlaps(pool: PgPool) {
    let (app, admin, alice, _) = setup(&pool).await;
    push_session(
        &app,
        &alice,
        "s-early",
        "codex",
        "2026-02-28T20:00:00Z",
        "2026-03-01T08:00:00Z",
        "/w/mabiao",
    )
    .await;

    let (_, body) = get(
        &app,
        &admin,
        "/api/v1/work-timeline?from=2026-03-01T00:00:00Z&to=2026-03-02T00:00:00Z",
    )
    .await;
    assert_eq!(body["segment_count"], 1);
    let seg = first_seg(&body, "2026-03-01");
    assert_eq!(seg["start"], "2026-03-01T00:00:00Z");
    assert_eq!(seg["end"], "2026-03-01T08:00:00Z");
    assert_eq!(seg["total_tokens"], 0);
}

#[sqlx::test]
async fn member_only_sees_own_sessions_and_cannot_ask_for_another_account(pool: PgPool) {
    let (app, admin, alice, bob) = setup(&pool).await;
    push_session(
        &app,
        &alice,
        "s-a",
        "codex",
        "2026-03-01T09:00:00Z",
        "2026-03-01T10:00:00Z",
        "/w/mabiao",
    )
    .await;
    push_session(
        &app,
        &bob,
        "s-b",
        "claude",
        "2026-03-01T11:00:00Z",
        "2026-03-01T12:00:00Z",
        "/w/other",
    )
    .await;

    let (_, mine) = get(&app, &alice, "/api/v1/work-timeline").await;
    assert_eq!(mine["segment_count"], 1);
    assert_eq!(mine["days"][0]["segments"][0]["session_id"], "s-a");

    let (_, all) = get(&app, &admin, "/api/v1/work-timeline").await;
    assert_eq!(all["segment_count"], 2);

    let bob_id: i64 = sqlx::query_scalar("SELECT id FROM remote_accounts WHERE account = 'bob'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (status, body) = get(
        &app,
        &alice,
        &format!("/api/v1/work-timeline?account_id={bob_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[sqlx::test]
async fn source_filter_and_timezone_day_boundary(pool: PgPool) {
    let (app, admin, alice, _) = setup(&pool).await;
    push_session(
        &app,
        &alice,
        "s-codex",
        "codex",
        "2026-03-01T10:00:00Z",
        "2026-03-01T11:00:00Z",
        "/w/mabiao",
    )
    .await;
    push_session(
        &app,
        &alice,
        "s-claude",
        "claude",
        "2026-03-01T12:00:00Z",
        "2026-03-01T13:00:00Z",
        "/w/mabiao",
    )
    .await;
    push_usage(
        &app,
        &alice,
        "s-codex",
        "codex",
        "2026-03-01T10:30:00Z",
        50,
        0.5,
    )
    .await;

    let (_, only) = get(&app, &admin, "/api/v1/work-timeline?source=codex").await;
    assert_eq!(only["segment_count"], 1);
    assert_eq!(only["days"][0]["segments"][0]["source"], "codex");
    assert_eq!(only["total_tokens"], 50);

    // UTC+14：3/1 10:00Z 是 3/2 00:00。
    let (_, east) = get(
        &app,
        &admin,
        "/api/v1/work-timeline?source=codex&tz_offset_minutes=840",
    )
    .await;
    assert_eq!(east["days"][0]["day"], "2026-03-02");
}

#[sqlx::test]
async fn empty_team_and_bad_parameters(pool: PgPool) {
    seed_admin(&pool, "root").await;
    let app = app(&pool);
    let admin = token_of(&app, "root").await;
    let (status, body) = get(&app, &admin, "/api/v1/work-timeline").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["segment_count"], 0);
    assert_eq!(body["days"].as_array().unwrap().len(), 0);

    for query in [
        "?from=yesterday",
        "?tz_offset_minutes=900",
        "?account_id=abc",
        "?from=2020-01-01T00:00:00Z&to=2022-01-01T00:00:00Z",
    ] {
        let (status, _) = get(&app, &admin, &format!("/api/v1/work-timeline{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
    let (status, _) = call(&app, Method::GET, "/api/v1/work-timeline", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
