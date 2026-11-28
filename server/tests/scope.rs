//! 统一 Scope 校验（跨账号 403、未知/已合并/无权项目 404）、Work Notes 聚合口径与直出 JSON 兼容性。

mod common;

use axum::http::{Method, StatusCode};
use axum::Router;
use common::*;
use push_protocol::{
    usage_fingerprint, ApiErrorCode, DeviceInfo, EventActor, EventKind, EventPayload,
    PricingSource, PushSessionRequest, PushUsageRequest, SessionPayload, UsageRecordPayload,
    UsageTokens, PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use sqlx::PgPool;

const REMOTE_A: &str = "git@github.com:team/alpha.git";
const REMOTE_B: &str = "git@github.com:team/beta.git";

struct Team {
    app: Router,
    admin: String,
    alice: String,
    bob: String,
    alice_id: i64,
    bob_id: i64,
}

async fn team(pool: &PgPool) -> Team {
    seed_admin(pool, "root").await;
    let alice_id = seed_member(pool, "alice").await;
    let bob_id = seed_member(pool, "bob").await;
    let app = app(pool);
    Team {
        admin: token_of(&app, "root").await,
        alice: token_of(&app, "alice").await,
        bob: token_of(&app, "bob").await,
        app,
        alice_id,
        bob_id,
    }
}

fn device() -> DeviceInfo {
    DeviceInfo {
        device_id: "dev-1".into(),
        device_name: "工作机".into(),
    }
}

fn tokens(input: i64, output: i64, cache_read: i64) -> UsageTokens {
    UsageTokens {
        input,
        output,
        cache_read,
        cache_creation: 0,
        reasoning: 0,
        total: input + output + cache_read,
    }
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
        text: Some("ok".into()),
        details: json!({}),
    }
}

async fn push_session(app: &Router, token: &str, session: SessionPayload) -> Value {
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
    body
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

fn make_session(
    id: &str,
    project: &str,
    remote: Option<&str>,
    generated_by_work_notes: bool,
    events: Vec<EventPayload>,
) -> SessionPayload {
    SessionPayload {
        source: "codex".into(),
        session_id: id.into(),
        title: format!("会话 {id}"),
        project: project.into(),
        git_remote_url: remote.map(str::to_owned),
        model: "gpt-5".into(),
        started_at: "2026-03-02T09:00:00Z".into(),
        ended_at: "2026-03-02T10:00:00Z".into(),
        source_files: vec!["/s/a.jsonl".into()],
        generated_by_work_notes,
        redaction_count: 0,
        events,
        context_manifest: None,
    }
}

fn make_usage(session_id: &str, project: &str, at: &str, input: i64) -> UsageRecordPayload {
    let parts = tokens(input, 20, 10);
    UsageRecordPayload {
        fingerprint: usage_fingerprint("codex", "/s/a.jsonl", at, "gpt-5", &parts),
        occurred_at: at.into(),
        source: "codex".into(),
        model: "gpt-5".into(),
        provider: "openai".into(),
        project: project.into(),
        session_id: session_id.into(),
        source_file: "/s/a.jsonl".into(),
        tokens: parts,
        native_cost: Some(1.0),
        cost_snapshot: Some(1.0),
        pricing_source: PricingSource::Native,
    }
}

async fn get(app: &Router, token: &str, uri: &str) -> (StatusCode, Value) {
    call(app, Method::GET, uri, Some(token), None).await
}

async fn project_id_named(app: &Router, admin: &str, name: &str) -> i64 {
    let (status, body) = get(app, admin, "/api/v1/admin/projects").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("没有项目 {name}: {body}"))["id"]
        .as_i64()
        .unwrap()
}

/// 全部 7 个支持项目范围的只读接口。
fn project_scoped_uris(project_id: i64, extra_query: &str) -> Vec<String> {
    let suffix = if extra_query.is_empty() {
        String::new()
    } else {
        format!("&{extra_query}")
    };
    let detail_query = if extra_query.is_empty() {
        String::new()
    } else {
        format!("?{extra_query}")
    };
    vec![
        format!("/api/v1/projects/{project_id}{detail_query}"),
        format!("/api/v1/usage?project_id={project_id}{suffix}"),
        format!("/api/v1/usage/summary?project_id={project_id}{suffix}"),
        format!("/api/v1/usage/composition?project_id={project_id}{suffix}"),
        format!("/api/v1/sessions?project_id={project_id}{suffix}"),
        format!("/api/v1/sessions/behavior?project_id={project_id}{suffix}"),
        format!("/api/v1/work-timeline?project_id={project_id}{suffix}"),
    ]
}

async fn seed_alpha_and_beta(t: &Team) -> (i64, i64) {
    // alpha：alice 和 bob 都有数据；beta：只有 alice 有数据。
    push_session(
        &t.app,
        &t.alice,
        make_session(
            "a-alice",
            "/w/alpha",
            Some(REMOTE_A),
            false,
            vec![event(
                "e1",
                EventKind::ToolCall,
                Some(EventActor::Assistant),
                Some("Bash"),
            )],
        ),
    )
    .await;
    push_session(
        &t.app,
        &t.bob,
        make_session("a-bob", "/w/alpha", Some(REMOTE_A), false, vec![]),
    )
    .await;
    push_session(
        &t.app,
        &t.alice,
        make_session("b-alice", "/w/beta", Some(REMOTE_B), false, vec![]),
    )
    .await;
    push_usage(
        &t.app,
        &t.alice,
        vec![
            make_usage("a-alice", "/w/alpha", "2026-03-02T09:30:00Z", 100),
            make_usage("b-alice", "/w/beta", "2026-03-02T09:40:00Z", 200),
        ],
    )
    .await;
    push_usage(
        &t.app,
        &t.bob,
        vec![make_usage("a-bob", "/w/alpha", "2026-03-02T09:50:00Z", 300)],
    )
    .await;
    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;
    let beta = project_id_named(&t.app, &t.admin, "beta").await;
    (alpha, beta)
}

#[sqlx::test]
async fn every_project_scoped_endpoint_enforces_403_before_project_lookup(pool: PgPool) {
    let t = team(&pool).await;
    let (alpha, beta) = seed_alpha_and_beta(&t).await;

    // 成员指定别人的 account_id：无论不带 project_id、带合法项目、带无权项目还是带不存在项目，一律 403。
    let other = format!("account_id={}", t.bob_id);
    for uri in [
        format!("/api/v1/usage?{other}"),
        format!("/api/v1/usage/summary?{other}"),
        format!("/api/v1/usage/composition?{other}"),
        format!("/api/v1/sessions?{other}"),
        format!("/api/v1/sessions/behavior?{other}"),
        format!("/api/v1/work-timeline?{other}"),
    ]
    .into_iter()
    .chain(project_scoped_uris(alpha, &other))
    .chain(project_scoped_uris(999_999, &other))
    {
        let (status, body) = get(&t.app, &t.alice, &uri).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri} -> {body}");
        assert_eq!(error_code(&body), ApiErrorCode::Forbidden, "{uri}");
    }

    // bob 查 alice 的账号同时带自己无权的 beta：同样必须是 403 而不是 404。
    let alice_param = format!("account_id={}", t.alice_id);
    for uri in project_scoped_uris(beta, &alice_param) {
        let (status, body) = get(&t.app, &t.bob, &uri).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri} -> {body}");
        assert_eq!(error_code(&body), ApiErrorCode::Forbidden, "{uri}");
    }
}

#[sqlx::test]
async fn every_project_scoped_endpoint_returns_404_for_unknown_unauthorized_and_merged_project(
    pool: PgPool,
) {
    let t = team(&pool).await;
    let (alpha, beta) = seed_alpha_and_beta(&t).await;

    // 1. 不存在的 project_id：管理员和成员一律 404。
    for uri in project_scoped_uris(999_999, "") {
        for (who, token) in [("admin", &t.admin), ("alice", &t.alice)] {
            let (status, body) = get(&t.app, token, &uri).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{who} {uri} -> {body}");
            assert_eq!(error_code(&body), ApiErrorCode::NotFound, "{who} {uri}");
        }
    }

    // 2. beta 只有 alice 的数据：bob 一律 404；alice 和 admin 一律 200。
    for uri in project_scoped_uris(beta, "") {
        let (status, body) = get(&t.app, &t.bob, &uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "bob {uri} -> {body}");
        assert_eq!(error_code(&body), ApiErrorCode::NotFound, "bob {uri}");

        let (status, body) = get(&t.app, &t.alice, &uri).await;
        assert_eq!(status, StatusCode::OK, "alice {uri} -> {body}");

        let (status, body) = get(&t.app, &t.admin, &uri).await;
        assert_eq!(status, StatusCode::OK, "admin {uri} -> {body}");
    }

    // 3. has_data_of 不受时间窗口限制：alice 在 3 月有 beta 数据，查空的 1 月区间仍得 200。
    let empty_window = "from=2026-01-01T00:00:00Z&to=2026-01-02T00:00:00Z";
    for uri in project_scoped_uris(beta, empty_window) {
        let (status, body) = get(&t.app, &t.alice, &uri).await;
        assert_eq!(status, StatusCode::OK, "alice empty window {uri} -> {body}");
    }

    // 4. 合并 beta -> alpha 后，beta 在所有 7 个接口上对管理员和成员都返回 404，alpha 仍 200。
    let (status, merged) = call(
        &t.app,
        Method::POST,
        &format!("/api/v1/admin/projects/{beta}/merge"),
        Some(&t.admin),
        Some(json!({ "into_project_id": alpha })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{merged}");

    for uri in project_scoped_uris(beta, "") {
        for (who, token) in [("admin", &t.admin), ("alice", &t.alice)] {
            let (status, body) = get(&t.app, token, &uri).await;
            assert_eq!(
                status,
                StatusCode::NOT_FOUND,
                "merged {who} {uri} -> {body}"
            );
            assert_eq!(
                error_code(&body),
                ApiErrorCode::NotFound,
                "merged {who} {uri}"
            );
        }
    }
    for uri in project_scoped_uris(alpha, "") {
        let (status, body) = get(&t.app, &t.admin, &uri).await;
        assert_eq!(status, StatusCode::OK, "target alpha {uri} -> {body}");
    }
}

#[sqlx::test]
async fn work_notes_sessions_stay_counted_in_summary_behavior_and_timeline(pool: PgPool) {
    let t = team(&pool).await;
    push_session(
        &t.app,
        &t.alice,
        make_session(
            "wn-1",
            "/w/alpha",
            Some(REMOTE_A),
            true,
            vec![
                event(
                    "e1",
                    EventKind::ToolCall,
                    Some(EventActor::Assistant),
                    Some("Read"),
                ),
                event(
                    "e2",
                    EventKind::ToolCall,
                    Some(EventActor::Assistant),
                    Some("Bash"),
                ),
                event("e3", EventKind::Error, Some(EventActor::Tool), Some("Bash")),
            ],
        ),
    )
    .await;
    push_usage(
        &t.app,
        &t.alice,
        vec![make_usage("wn-1", "/w/alpha", "2026-03-02T09:30:00Z", 500)],
    )
    .await;

    let (status, summary) = get(&t.app, &t.admin, "/api/v1/usage/summary").await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(summary["totals"]["record_count"], 1);
    assert_eq!(summary["totals"]["total_tokens"], 530);
    assert_eq!(summary["by_source"][0]["session_count"], 1);

    let (status, behavior) = get(&t.app, &t.admin, "/api/v1/sessions/behavior").await;
    assert_eq!(status, StatusCode::OK, "{behavior}");
    assert_eq!(behavior["tool_calls"], 2);
    assert_eq!(behavior["tool_failures"], 1);

    let (status, timeline) = get(&t.app, &t.admin, "/api/v1/work-timeline").await;
    assert_eq!(status, StatusCode::OK, "{timeline}");
    assert_eq!(timeline["segment_count"], 1);
    assert_eq!(timeline["total_tokens"], 530);

    let (_, all_sessions) = get(&t.app, &t.admin, "/api/v1/sessions").await;
    assert_eq!(all_sessions["total"], 1);
    let (_, only_wn) = get(
        &t.app,
        &t.admin,
        "/api/v1/sessions?generated_by_work_notes=true",
    )
    .await;
    assert_eq!(only_wn["total"], 1);
    let (_, without_wn) = get(
        &t.app,
        &t.admin,
        "/api/v1/sessions?generated_by_work_notes=false",
    )
    .await;
    assert_eq!(without_wn["total"], 0);
}

#[sqlx::test]
async fn direct_serialized_endpoints_preserve_wire_shapes_and_hide_internal_fields(pool: PgPool) {
    let t = team(&pool).await;
    let (alpha, _) = seed_alpha_and_beta(&t).await;

    let (status, me) = get(&t.app, &t.alice, "/api/v1/me").await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert!(me.get("password_hash").is_none());

    let (status, detail) = get(&t.app, &t.admin, &format!("/api/v1/projects/{alpha}")).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert!(detail["project"].get("merged_into").is_none());
    assert_eq!(detail["project"]["id"], alpha);
    assert!(detail["summary"]["totals"]["total_tokens"].is_i64());

    let (status, admin_projects) = get(&t.app, &t.admin, "/api/v1/admin/projects").await;
    assert_eq!(status, StatusCode::OK, "{admin_projects}");
    let first_proj = &admin_projects[0];
    assert!(first_proj.get("merged_into").is_none());
    assert!(first_proj.get("project").is_none());
    assert!(first_proj["session_count"].is_i64());
    assert!(first_proj["merged_count"].is_i64());

    let (status, sessions) = get(&t.app, &t.admin, "/api/v1/sessions").await;
    assert_eq!(status, StatusCode::OK, "{sessions}");
    let first_session = &sessions["sessions"][0];
    assert!(first_session["project"].is_string());
    assert!(first_session.get("project_path").is_none());

    let session_pk = first_session["id"].as_i64().unwrap();
    let (status, session_detail) =
        get(&t.app, &t.admin, &format!("/api/v1/sessions/{session_pk}")).await;
    assert_eq!(status, StatusCode::OK, "{session_detail}");
    assert!(session_detail.get("device_pk").is_none());
    assert!(session_detail["session"]["project"].is_string());
    assert!(session_detail["session"].get("project_path").is_none());

    let (status, comp) = get(&t.app, &t.admin, "/api/v1/usage/composition").await;
    assert_eq!(status, StatusCode::OK, "{comp}");
    let codex_bucket = &comp["by_source"][0];
    assert_eq!(codex_bucket["key"], "codex");
    assert!(codex_bucket["input"].is_i64());
    assert!(codex_bucket.get("tokens").is_none());

    let (status, devices) = get(
        &t.app,
        &t.alice,
        &format!("/api/v1/accounts/{}/devices", t.alice_id),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{devices}");
    assert_eq!(devices[0]["device_name"], "工作机");

    let (status, coverage) = get(&t.app, &t.admin, "/api/v1/admin/coverage").await;
    assert_eq!(status, StatusCode::OK, "{coverage}");
    assert!(coverage.as_array().unwrap().len() >= 3);
}
