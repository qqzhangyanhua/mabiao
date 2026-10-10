//! 推送接收：整场覆盖、指纹去重、项目归并、覆盖进度、删除与越权（连真 PostgreSQL）。

mod common;

use axum::http::{Method, StatusCode};
use axum::Router;
use common::*;
use push_protocol::{
    usage_fingerprint, ApiErrorCode, ContextItemPayload, ContextKind, ContextLayer,
    ContextManifestPayload, DeviceInfo, EventActor, EventKind, EventPayload, PricingSource,
    PushSessionRequest, PushUsageRequest, SessionPayload, UsageRecordPayload, UsageTokens,
    PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use sqlx::PgPool;

fn device(id: &str) -> DeviceInfo {
    DeviceInfo {
        device_id: id.into(),
        device_name: format!("{id} 的电脑"),
    }
}

fn event(sequence: u32, text: &str) -> EventPayload {
    EventPayload {
        event_id: format!("e-{sequence}"),
        sequence,
        source_file: "/s/a.jsonl".into(),
        source_sequence: sequence,
        kind: EventKind::Message,
        occurred_at: Some("2026-03-01T10:00:00Z".into()),
        actor: Some(EventActor::User),
        name: None,
        text: Some(text.into()),
        details: json!({"n": sequence}),
    }
}

fn session(source: &str, id: &str, events: u32) -> SessionPayload {
    SessionPayload {
        source: source.into(),
        session_id: id.into(),
        title: format!("{id} 标题"),
        project: "/Users/alice/work/mabiao".into(),
        git_remote_url: Some("git@github.com:qqzhangyanhua/mabiao.git".into()),
        model: "claude-sonnet".into(),
        started_at: "2026-03-01T10:00:00Z".into(),
        ended_at: "2026-03-01T11:00:00Z".into(),
        source_files: vec!["/s/a.jsonl".into()],
        generated_by_work_notes: false,
        redaction_count: 2,
        events: (0..events)
            .map(|n| event(n, &format!("第 {n} 句")))
            .collect(),
        context_manifest: None,
    }
}

fn session_request(device: &DeviceInfo, session: SessionPayload) -> Value {
    serde_json::to_value(PushSessionRequest {
        protocol_version: PROTOCOL_VERSION,
        device: device.clone(),
        session,
    })
    .unwrap()
}

async fn push_session(
    app: &Router,
    token: &str,
    device: &DeviceInfo,
    session: SessionPayload,
) -> (StatusCode, Value) {
    call(
        app,
        Method::POST,
        "/api/v1/push/session",
        Some(token),
        Some(session_request(device, session)),
    )
    .await
}

fn usage(file: &str, at: &str, input: i64, project: &str) -> UsageRecordPayload {
    let tokens = UsageTokens {
        input,
        output: 5,
        cache_read: 0,
        cache_creation: 0,
        reasoning: 0,
        total: input + 5,
    };
    UsageRecordPayload {
        fingerprint: usage_fingerprint("codex", file, at, "gpt-5", &tokens),
        occurred_at: at.into(),
        source: "codex".into(),
        model: "gpt-5".into(),
        provider: "openai".into(),
        project: project.into(),
        session_id: "s-1".into(),
        source_file: file.into(),
        tokens,
        native_cost: None,
        cost_snapshot: Some(0.25),
        pricing_source: PricingSource::Exact,
    }
}

async fn push_usage(
    app: &Router,
    token: &str,
    device: &DeviceInfo,
    records: Vec<UsageRecordPayload>,
) -> (StatusCode, Value) {
    call(
        app,
        Method::POST,
        "/api/v1/push/usage",
        Some(token),
        Some(
            serde_json::to_value(PushUsageRequest {
                protocol_version: PROTOCOL_VERSION,
                device: device.clone(),
                records,
            })
            .unwrap(),
        ),
    )
    .await
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

// ---------- 会话：整场覆盖 ----------

#[sqlx::test]
async fn session_is_stored_with_events_as_jsonb(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;

    let (status, body) =
        push_session(&app, &token, &device("d1"), session("codex", "s-1", 3)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["replaced"], false);
    assert_eq!(body["session_id"], "s-1");
    let (events, event_count, redactions, title): (Value, i32, i32, String) =
        sqlx::query_as("SELECT events, event_count, redaction_count, title FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(event_count, 3);
    assert_eq!(redactions, 2);
    assert_eq!(title, "s-1 标题");
    assert_eq!(events.as_array().unwrap().len(), 3);
    assert_eq!(events[2]["text"], "第 2 句");
    assert_eq!(events[2]["details"], json!({"n": 2}));
}

#[sqlx::test]
async fn pushing_the_same_session_twice_is_idempotent(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");

    let (_, first) = push_session(&app, &token, &d, session("codex", "s-1", 3)).await;
    let (status, second) = push_session(&app, &token, &d, session("codex", "s-1", 3)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["replaced"], false);
    assert_eq!(second["replaced"], true);
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 1);
}

#[sqlx::test]
async fn a_session_that_grew_replaces_the_old_copy_whole(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");
    push_session(&app, &token, &d, session("codex", "s-1", 3)).await;
    let first_pushed: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT first_pushed_at FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();

    let mut longer = session("codex", "s-1", 5);
    longer.title = "改过的标题".into();
    longer.ended_at = "2026-03-02T09:00:00Z".into();
    longer.redaction_count = 7;
    let (status, body) = push_session(&app, &token, &d, longer).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["replaced"], true);
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 1);
    let (events, event_count, title, redactions, first_after): (
        Value,
        i32,
        String,
        i32,
        chrono::DateTime<chrono::Utc>,
    ) = sqlx::query_as(
        "SELECT events, event_count, title, redaction_count, first_pushed_at FROM sessions",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(event_count, 5);
    assert_eq!(events.as_array().unwrap().len(), 5);
    assert_eq!(title, "改过的标题");
    assert_eq!(redactions, 7);
    assert_eq!(first_after, first_pushed, "首次入库时间不变");
}

#[sqlx::test]
async fn a_shorter_resend_also_replaces_rather_than_merges(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");
    push_session(&app, &token, &d, session("codex", "s-1", 5)).await;
    push_session(&app, &token, &d, session("codex", "s-1", 2)).await;
    assert_eq!(
        count(&pool, "SELECT event_count::bigint FROM sessions").await,
        2
    );
}

#[sqlx::test]
async fn same_session_id_on_two_devices_does_not_overwrite(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;

    let (_, a) = push_session(&app, &token, &device("laptop"), session("codex", "s-1", 2)).await;
    let (_, b) = push_session(&app, &token, &device("desktop"), session("codex", "s-1", 4)).await;

    assert_eq!(a["replaced"], false);
    assert_eq!(b["replaced"], false, "另一台设备是新的一份，不是覆盖");
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 2);
    let counts: Vec<i32> = sqlx::query_scalar(
        "SELECT s.event_count FROM sessions s JOIN devices d ON d.id = s.device_pk
         ORDER BY d.device_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(counts, [4, 2], "desktop=4, laptop=2 各自保留");
}

#[sqlx::test]
async fn same_session_id_from_another_member_never_touches_mine(pool: PgPool) {
    seed_member(&pool, "alice").await;
    seed_member(&pool, "bob").await;
    let app = app(&pool);
    let alice = token_of(&app, "alice").await;
    let bob = token_of(&app, "bob").await;

    push_session(&app, &alice, &device("same-id"), session("codex", "s-1", 3)).await;
    let (_, body) = push_session(&app, &bob, &device("same-id"), session("codex", "s-1", 9)).await;

    assert_eq!(body["replaced"], false);
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 2);
    let alice_events: i32 = sqlx::query_scalar(
        "SELECT s.event_count FROM sessions s JOIN remote_accounts a ON a.id = s.account_id
         WHERE a.account = 'alice'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(alice_events, 3);
}

#[sqlx::test]
async fn different_sources_with_the_same_session_id_coexist(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");
    push_session(&app, &token, &d, session("codex", "s-1", 1)).await;
    push_session(&app, &token, &d, session("claude", "s-1", 1)).await;
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 2);
}

#[sqlx::test]
async fn context_manifest_is_stored_and_unparseable_times_become_null(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let mut s = session("cursor_agent", "s-1", 1);
    s.started_at = "昨天".into();
    s.context_manifest = Some(ContextManifestPayload {
        items: vec![ContextItemPayload {
            layer: ContextLayer::Injected,
            kind: ContextKind::Rule,
            id: "rule:1".into(),
            label: "rules".into(),
            path: None,
            load_mode: None,
            injection_status: None,
            char_count: Some(10),
            is_noise: false,
            is_unused_install: false,
            content: Some("注入原文".into()),
        }],
        from_cache: false,
        has_injected_snapshot: true,
        volume_is_estimate: false,
    });

    let (status, body) = push_session(&app, &token, &device("d1"), s).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let (manifest, started): (Value, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT context_manifest, started_at FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(manifest["items"][0]["content"], "注入原文");
    assert_eq!(started, None);
}

#[sqlx::test]
async fn pushing_registers_the_device_and_refreshes_its_name(pool: PgPool) {
    let alice = seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    push_session(&app, &token, &device("d1"), session("codex", "s-1", 1)).await;
    let renamed = DeviceInfo {
        device_id: "d1".into(),
        device_name: "新名字".into(),
    };
    push_session(&app, &token, &renamed, session("codex", "s-2", 1)).await;

    let (status, body) = call(
        &app,
        Method::GET,
        &format!("/api/v1/accounts/{alice}/devices"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["device_name"], "新名字");
}

// ---------- 消耗记录：指纹去重 ----------

#[sqlx::test]
async fn usage_dedups_by_fingerprint_so_repeat_pushes_do_not_double_count(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");
    let batch = vec![
        usage("/a", "2026-03-01T01:00:00Z", 10, "/p"),
        usage("/a", "2026-03-01T02:00:00Z", 20, "/p"),
        usage("/a", "2026-03-01T03:00:00Z", 30, "/p"),
    ];

    let (status, first) = push_usage(&app, &token, &d, batch.clone()).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first, json!({"inserted": 3, "duplicates": 0}));

    let mut again = batch;
    again.push(usage("/a", "2026-03-01T04:00:00Z", 40, "/p"));
    let (_, second) = push_usage(&app, &token, &d, again).await;
    assert_eq!(second, json!({"inserted": 1, "duplicates": 3}));

    assert_eq!(count(&pool, "SELECT count(*) FROM usage_records").await, 4);
    assert_eq!(
        count(&pool, "SELECT sum(input_tokens)::bigint FROM usage_records").await,
        100
    );
}

#[sqlx::test]
async fn duplicates_inside_one_request_are_counted_once(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let one = usage("/a", "2026-03-01T01:00:00Z", 10, "/p");

    let (status, body) = push_usage(&app, &token, &device("d1"), vec![one.clone(), one]).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({"inserted": 1, "duplicates": 1}));
}

#[sqlx::test]
async fn client_cost_snapshot_and_pricing_source_are_stored_verbatim(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let mut native = usage("/a", "2026-03-01T01:00:00Z", 10, "/p");
    native.native_cost = Some(0.123456789);
    native.cost_snapshot = Some(0.123456789);
    native.pricing_source = PricingSource::Native;
    let mut unpriced = usage("/a", "2026-03-01T02:00:00Z", 10, "/p");
    unpriced.cost_snapshot = None;
    unpriced.pricing_source = PricingSource::Unpriced;
    let mut fallback = usage("/a", "2026-03-01T03:00:00Z", 10, "/p");
    fallback.cost_snapshot = Some(7.5);
    fallback.pricing_source = PricingSource::Fallback;

    let (status, body) = push_usage(
        &app,
        &token,
        &device("d1"),
        vec![native, unpriced, fallback],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let rows: Vec<(Option<f64>, Option<f64>, String)> = sqlx::query_as(
        "SELECT native_cost, cost_snapshot, pricing_source FROM usage_records ORDER BY occurred_at",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        [
            (Some(0.123456789), Some(0.123456789), "native".to_string()),
            (None, None, "unpriced".to_string()),
            (None, Some(7.5), "fallback".to_string()),
        ]
    );
}

#[sqlx::test]
async fn the_same_fingerprint_from_another_device_or_member_is_a_separate_record(pool: PgPool) {
    seed_member(&pool, "alice").await;
    seed_member(&pool, "bob").await;
    let app = app(&pool);
    let alice = token_of(&app, "alice").await;
    let bob = token_of(&app, "bob").await;
    let record = usage("/a", "2026-03-01T01:00:00Z", 10, "/p");

    for (token, dev) in [(&alice, "laptop"), (&alice, "desktop"), (&bob, "laptop")] {
        let (_, body) = push_usage(&app, token, &device(dev), vec![record.clone()]).await;
        assert_eq!(body["inserted"], 1, "{dev}");
    }
    assert_eq!(count(&pool, "SELECT count(*) FROM usage_records").await, 3);
}

#[sqlx::test]
async fn an_invalid_record_rejects_the_whole_batch(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let good = usage("/a", "2026-03-01T01:00:00Z", 10, "/p");
    let mut bad_time = usage("/a", "2026-03-01T02:00:00Z", 10, "/p");
    bad_time.occurred_at = "昨天".into();
    let mut negative = usage("/a", "2026-03-01T03:00:00Z", 10, "/p");
    negative.tokens.input = -1;

    for bad in [bad_time, negative] {
        let (status, body) = push_usage(&app, &token, &device("d1"), vec![good.clone(), bad]).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error_code(&body), ApiErrorCode::InvalidPayload);
    }
    assert_eq!(
        count(&pool, "SELECT count(*) FROM usage_records").await,
        0,
        "不做半截入库"
    );
}

#[sqlx::test]
async fn oversized_usage_batches_are_refused(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let records: Vec<_> = (0..5001)
        .map(|n| usage("/a", "2026-03-01T01:00:00Z", n, "/p"))
        .collect();
    let (status, body) = push_usage(&app, &token, &device("d1"), records).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), ApiErrorCode::InvalidPayload);
}

#[sqlx::test]
async fn large_usage_batches_within_the_limit_insert_fully(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let records: Vec<_> = (0..2500)
        .map(|n| usage("/a", "2026-03-01T01:00:00Z", n, "/p"))
        .collect();
    let (status, body) = push_usage(&app, &token, &device("d1"), records).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({"inserted": 2500, "duplicates": 0}));
}

// ---------- 项目归并 ----------

async fn project_ids(pool: &PgPool) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as("SELECT key, name, git_remote FROM projects ORDER BY key")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn git_remote_merges_one_repo_across_devices_members_and_remote_spellings(pool: PgPool) {
    seed_member(&pool, "alice").await;
    seed_member(&pool, "bob").await;
    let app = app(&pool);
    let alice = token_of(&app, "alice").await;
    let bob = token_of(&app, "bob").await;

    let mut a = session("codex", "s-1", 1);
    a.project = "/Users/alice/work/mabiao".into();
    a.git_remote_url = Some("git@github.com:Owner/Repo.git".into());
    let mut b = session("codex", "s-2", 1);
    b.project = "C:\\code\\renamed-checkout".into();
    b.git_remote_url = Some("https://user:PLACEHOLDER@github.com/owner/repo.git".into());
    push_session(&app, &alice, &device("laptop"), a).await;
    push_session(&app, &bob, &device("pc"), b).await;

    assert_eq!(
        project_ids(&pool).await,
        [(
            "git:github.com/owner/repo".to_string(),
            "repo".to_string(),
            Some("github.com/owner/repo".to_string())
        )]
    );
    let distinct: i64 = count(
        &pool,
        "SELECT count(DISTINCT project_id) FROM sessions WHERE project_id IS NOT NULL",
    )
    .await;
    assert_eq!(distinct, 1);
    let paths: Vec<String> =
        sqlx::query_scalar("SELECT project_path FROM sessions ORDER BY project_path COLLATE \"C\"")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        paths,
        ["/Users/alice/work/mabiao", "C:\\code\\renamed-checkout"],
        "保留各设备的原始路径"
    );
}

#[sqlx::test]
async fn credentials_in_a_remote_url_are_never_stored(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let mut s = session("codex", "s-1", 1);
    s.git_remote_url = Some("https://oauth2:TOPSECRET@gitlab.example/team/app.git".into());
    push_session(&app, &token, &device("d1"), s).await;

    let dump: String = sqlx::query_scalar(
        "SELECT (SELECT string_agg(p::text, ' ') FROM projects p)
             || (SELECT string_agg(s::text, ' ') FROM sessions s)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!dump.contains("TOPSECRET"));
    assert!(!dump.contains("oauth2"));
}

#[sqlx::test]
async fn without_a_remote_the_directory_name_is_the_fallback(pool: PgPool) {
    seed_member(&pool, "alice").await;
    seed_member(&pool, "bob").await;
    let app = app(&pool);
    let alice = token_of(&app, "alice").await;
    let bob = token_of(&app, "bob").await;
    let mut a = session("codex", "s-1", 1);
    a.project = "/Users/alice/work/Notes".into();
    a.git_remote_url = None;
    let mut b = session("codex", "s-2", 1);
    b.project = "D:\\stuff\\notes\\".into();
    b.git_remote_url = None;
    push_session(&app, &alice, &device("d1"), a).await;
    push_session(&app, &bob, &device("d2"), b).await;

    assert_eq!(
        project_ids(&pool).await,
        [("dir:notes".to_string(), "Notes".to_string(), None)]
    );
}

#[sqlx::test]
async fn a_session_without_any_project_clue_gets_no_project(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let mut s = session("codex", "s-1", 1);
    s.project = String::new();
    s.git_remote_url = None;
    let (status, _) = push_session(&app, &token, &device("d1"), s).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count(&pool, "SELECT count(*) FROM projects").await, 0);
    let project: Option<i64> = sqlx::query_scalar("SELECT project_id FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(project, None);
}

#[sqlx::test]
async fn usage_joins_the_git_project_of_a_session_from_the_same_path(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");
    push_session(&app, &token, &d, session("codex", "s-1", 1)).await;

    push_usage(
        &app,
        &token,
        &d,
        vec![usage(
            "/a",
            "2026-03-01T01:00:00Z",
            10,
            "/Users/alice/work/mabiao",
        )],
    )
    .await;

    let key: String = sqlx::query_scalar(
        "SELECT p.key FROM usage_records u JOIN projects p ON p.id = u.project_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(key, "git:github.com/qqzhangyanhua/mabiao");
}

#[sqlx::test]
async fn pushing_usage_first_then_the_session_repoints_the_history(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");
    push_usage(
        &app,
        &token,
        &d,
        vec![usage(
            "/a",
            "2026-03-01T01:00:00Z",
            10,
            "/Users/alice/work/mabiao",
        )],
    )
    .await;
    let before: String = sqlx::query_scalar(
        "SELECT p.key FROM usage_records u JOIN projects p ON p.id = u.project_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, "dir:mabiao");

    push_session(&app, &token, &d, session("codex", "s-1", 1)).await;

    let after: String = sqlx::query_scalar(
        "SELECT p.key FROM usage_records u JOIN projects p ON p.id = u.project_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after, "git:github.com/qqzhangyanhua/mabiao");
    let path_target: String = sqlx::query_scalar(
        "SELECT p.key FROM project_paths pp JOIN projects p ON p.id = pp.project_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(path_target, "git:github.com/qqzhangyanhua/mabiao");
}

#[sqlx::test]
async fn a_later_push_without_remote_does_not_downgrade_a_git_project(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");
    push_session(&app, &token, &d, session("codex", "s-1", 1)).await;
    let mut no_remote = session("codex", "s-2", 1);
    no_remote.git_remote_url = None;
    push_session(&app, &token, &d, no_remote).await;

    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT p.key FROM sessions s JOIN projects p ON p.id = s.project_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(keys, ["git:github.com/qqzhangyanhua/mabiao"]);
}

#[sqlx::test]
async fn changing_a_paths_remote_keeps_old_history_on_the_old_git_project(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");
    let old = session("codex", "s-old", 1);
    let path = old.project.clone();
    push_session(&app, &token, &d, old).await;
    let mut moved = session("codex", "s-new", 1);
    moved.project = path;
    moved.git_remote_url = Some("https://github.com/someone/else.git".into());
    push_session(&app, &token, &d, moved).await;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT s.session_id, p.key FROM sessions s JOIN projects p ON p.id = s.project_id
         ORDER BY s.session_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        [
            (
                "s-new".to_string(),
                "git:github.com/someone/else".to_string()
            ),
            (
                "s-old".to_string(),
                "git:github.com/qqzhangyanhua/mabiao".to_string()
            ),
        ]
    );
}

// ---------- 覆盖进度 ----------

#[sqlx::test]
async fn coverage_shows_last_push_and_how_far_the_data_reaches(pool: PgPool) {
    seed_admin(&pool, "root").await;
    let alice = seed_member(&pool, "alice").await;
    let bob = seed_member(&pool, "bob").await;
    let app = app(&pool);
    let admin = token_of(&app, "root").await;
    let alice_token = token_of(&app, "alice").await;

    let mut s = session("codex", "s-1", 1);
    s.ended_at = "2026-03-05T23:30:00Z".into();
    push_session(&app, &alice_token, &device("d1"), s).await;
    push_usage(
        &app,
        &alice_token,
        &device("d2"),
        vec![usage("/a", "2026-03-07T00:10:00Z", 1, "/p")],
    )
    .await;

    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/admin/coverage",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let row = |id: i64| {
        body.as_array()
            .unwrap()
            .iter()
            .find(|r| r["account_id"] == id)
            .unwrap()
            .clone()
    };
    let a = row(alice);
    assert_eq!(a["covered_through"], "2026-03-07");
    assert_eq!(a["device_count"], 2);
    assert!(a["last_push_at"].is_string());
    let b = row(bob);
    assert_eq!(
        b["covered_through"],
        Value::Null,
        "没推过的成员缺口一眼可见"
    );
    assert_eq!(b["last_push_at"], Value::Null);
    assert_eq!(b["device_count"], 0);
}

#[sqlx::test]
async fn coverage_follows_the_data_after_a_delete(pool: PgPool) {
    seed_admin(&pool, "root").await;
    let alice = seed_member(&pool, "alice").await;
    let app = app(&pool);
    let admin = token_of(&app, "root").await;
    let token = token_of(&app, "alice").await;
    let mut late = session("codex", "late", 1);
    late.ended_at = "2026-03-09T10:00:00Z".into();
    let mut early = session("codex", "early", 1);
    early.ended_at = "2026-03-02T10:00:00Z".into();
    push_session(&app, &token, &device("d1"), early).await;
    push_session(&app, &token, &device("d1"), late).await;
    let late_id: i64 = sqlx::query_scalar("SELECT id FROM sessions WHERE session_id = 'late'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/sessions/{late_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, body) = call(
        &app,
        Method::GET,
        "/api/v1/admin/coverage",
        Some(&admin),
        None,
    )
    .await;
    let row = body
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["account_id"] == alice)
        .unwrap();
    assert_eq!(row["covered_through"], "2026-03-02");
}

#[sqlx::test]
async fn members_cannot_read_the_coverage_list(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/admin/coverage",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), ApiErrorCode::Forbidden);
}

// ---------- 删除与越权 ----------

async fn session_row_id(pool: &PgPool, account: &str, session_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT s.id FROM sessions s JOIN remote_accounts a ON a.id = s.account_id
         WHERE a.account = $1 AND s.session_id = $2",
    )
    .bind(account)
    .bind(session_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn a_member_deletes_their_own_session_but_not_anyone_elses(pool: PgPool) {
    seed_member(&pool, "alice").await;
    seed_member(&pool, "bob").await;
    let app = app(&pool);
    let alice = token_of(&app, "alice").await;
    let bob = token_of(&app, "bob").await;
    push_session(&app, &alice, &device("d1"), session("codex", "a-1", 1)).await;
    push_session(&app, &bob, &device("d2"), session("codex", "b-1", 1)).await;
    let alice_row = session_row_id(&pool, "alice", "a-1").await;
    let bob_row = session_row_id(&pool, "bob", "b-1").await;

    let (status, body) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/sessions/{bob_row}"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), ApiErrorCode::Forbidden);
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 2);

    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/sessions/{alice_row}"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 1);
    assert_eq!(session_row_id(&pool, "bob", "b-1").await, bob_row);
}

#[sqlx::test]
async fn an_admin_deletes_any_session_and_unknown_ids_are_404(pool: PgPool) {
    seed_admin(&pool, "root").await;
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let admin = token_of(&app, "root").await;
    let alice = token_of(&app, "alice").await;
    push_session(&app, &alice, &device("d1"), session("codex", "a-1", 1)).await;
    let row = session_row_id(&pool, "alice", "a-1").await;

    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/sessions/{row}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);

    let (status, body) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/sessions/{row}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), ApiErrorCode::NotFound);
}

#[sqlx::test]
async fn deleting_a_session_leaves_usage_and_projects_and_a_resend_is_new(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");
    push_session(&app, &token, &d, session("codex", "s-1", 2)).await;
    push_usage(
        &app,
        &token,
        &d,
        vec![usage(
            "/a",
            "2026-03-01T01:00:00Z",
            10,
            "/Users/alice/work/mabiao",
        )],
    )
    .await;
    let row = session_row_id(&pool, "alice", "s-1").await;
    call(
        &app,
        Method::DELETE,
        &format!("/api/v1/sessions/{row}"),
        Some(&token),
        None,
    )
    .await;

    assert_eq!(count(&pool, "SELECT count(*) FROM usage_records").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM projects").await, 1);
    let (_, again) = push_session(&app, &token, &d, session("codex", "s-1", 2)).await;
    assert_eq!(again["replaced"], false, "删掉之后再推是新的一份");
}

#[sqlx::test]
async fn pushing_never_deletes_other_data(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");
    for n in 0..3 {
        push_session(&app, &token, &d, session("codex", &format!("s-{n}"), 1)).await;
    }
    // 之后一次只推一场、区间更窄的会话，不会让其它会话消失。
    push_session(&app, &token, &d, session("codex", "s-0", 1)).await;
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 3);
}

// ---------- 鉴权与校验 ----------

#[sqlx::test]
async fn push_and_delete_routes_need_a_token(pool: PgPool) {
    let app = app(&pool);
    let d = device("d1");
    let cases = [
        (
            Method::POST,
            "/api/v1/push/session",
            Some(session_request(&d, session("codex", "s-1", 1))),
        ),
        (
            Method::POST,
            "/api/v1/push/usage",
            Some(json!({"protocol_version": PROTOCOL_VERSION, "device": d, "records": []})),
        ),
        (Method::DELETE, "/api/v1/sessions/1", None),
    ];
    for (method, uri, body) in cases {
        let (status, _) = call(&app, method, uri, None, body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
    }
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);
}

#[sqlx::test]
async fn incompatible_protocol_versions_are_rejected_on_both_push_routes(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let d = device("d1");

    for version in [0, PROTOCOL_VERSION + 1] {
        let mut session_body = session_request(&d, session("codex", "s-1", 1));
        session_body["protocol_version"] = json!(version);
        let (status, body) = call(
            &app,
            Method::POST,
            "/api/v1/push/session",
            Some(&token),
            Some(session_body),
        )
        .await;
        assert_eq!(status, StatusCode::UPGRADE_REQUIRED, "session v{version}");
        assert_eq!(error_code(&body), ApiErrorCode::UnsupportedProtocolVersion);

        let (status, body) = call(
            &app,
            Method::POST,
            "/api/v1/push/usage",
            Some(&token),
            Some(json!({"protocol_version": version, "device": d, "records": []})),
        )
        .await;
        assert_eq!(status, StatusCode::UPGRADE_REQUIRED, "usage v{version}");
        assert_eq!(error_code(&body), ApiErrorCode::UnsupportedProtocolVersion);
    }
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);
}

#[sqlx::test]
async fn disk_content_in_a_manifest_is_refused_not_stored(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let mut s = session("cursor_agent", "s-1", 1);
    s.context_manifest = Some(ContextManifestPayload {
        items: vec![ContextItemPayload {
            layer: ContextLayer::OnDiskPossible,
            kind: ContextKind::Instruction,
            id: "disk:AGENTS.md".into(),
            label: "AGENTS.md".into(),
            path: Some("/repo/AGENTS.md".into()),
            load_mode: None,
            injection_status: None,
            char_count: Some(5),
            is_noise: false,
            is_unused_install: false,
            content: Some("磁盘文件内容".into()),
        }],
        ..Default::default()
    });

    let (status, body) = push_session(&app, &token, &device("d1"), s).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), ApiErrorCode::InvalidPayload);
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);
}

#[sqlx::test]
async fn blank_or_oversized_identifiers_are_refused(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;

    let blank_session = push_session(&app, &token, &device("d1"), session("codex", "", 1)).await;
    let blank_source = push_session(&app, &token, &device("d1"), session("", "s-1", 1)).await;
    let blank_device = push_session(&app, &token, &device(""), session("codex", "s-1", 1)).await;
    let long_id = push_session(
        &app,
        &token,
        &device("d1"),
        session("codex", &"x".repeat(513), 1),
    )
    .await;
    for (status, body) in [blank_session, blank_source, blank_device, long_id] {
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(error_code(&body), ApiErrorCode::InvalidPayload);
    }
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);
    assert_eq!(count(&pool, "SELECT count(*) FROM devices").await, 0);
}

#[sqlx::test]
async fn a_deactivated_member_can_no_longer_push(pool: PgPool) {
    seed_admin(&pool, "root").await;
    let alice = seed_member(&pool, "alice").await;
    let app = app(&pool);
    let admin = token_of(&app, "root").await;
    let token = token_of(&app, "alice").await;
    call(
        &app,
        Method::POST,
        &format!("/api/v1/admin/accounts/{alice}/deactivate"),
        Some(&admin),
        None,
    )
    .await;

    let (status, _) = push_session(&app, &token, &device("d1"), session("codex", "s-1", 1)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);
}

#[sqlx::test]
async fn a_big_session_body_is_accepted_beyond_the_default_limit(pool: PgPool) {
    seed_member(&pool, "alice").await;
    let app = app(&pool);
    let token = token_of(&app, "alice").await;
    let mut big = session("codex", "big", 1);
    big.events[0].text = Some("x".repeat(5 * 1024 * 1024));

    let (status, body) = push_session(&app, &token, &device("d1"), big).await;

    assert_eq!(status, StatusCode::OK, "{body}");
}
