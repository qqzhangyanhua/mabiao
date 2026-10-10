//! 会话详情、项目页、项目改名与合并（连真 PostgreSQL）。

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

fn device() -> DeviceInfo {
    DeviceInfo {
        device_id: "d1".into(),
        device_name: "办公室".into(),
    }
}

fn item(layer: ContextLayer, id: &str, content: Option<&str>) -> ContextItemPayload {
    ContextItemPayload {
        layer,
        kind: ContextKind::Instruction,
        id: id.into(),
        label: id.into(),
        path: None,
        load_mode: None,
        injection_status: None,
        char_count: Some(10),
        is_noise: false,
        is_unused_install: false,
        content: content.map(str::to_owned),
    }
}

fn session(id: &str, project: &str, remote: Option<&str>) -> SessionPayload {
    SessionPayload {
        source: "codex".into(),
        session_id: id.into(),
        title: format!("{id} 标题"),
        project: project.into(),
        git_remote_url: remote.map(str::to_owned),
        model: "team-model".into(),
        started_at: "2026-03-01T09:00:00Z".into(),
        ended_at: "2026-03-01T10:00:00Z".into(),
        source_files: vec!["/s/a.jsonl".into()],
        generated_by_work_notes: false,
        redaction_count: 3,
        events: vec![
            EventPayload {
                event_id: "e-0".into(),
                sequence: 0,
                source_file: "/s/a.jsonl".into(),
                source_sequence: 0,
                kind: EventKind::Message,
                occurred_at: None,
                actor: Some(EventActor::User),
                name: None,
                text: Some("请帮我改一下".into()),
                details: json!({}),
            },
            EventPayload {
                event_id: "e-1".into(),
                sequence: 1,
                source_file: "/s/a.jsonl".into(),
                source_sequence: 1,
                kind: EventKind::ToolCall,
                occurred_at: None,
                actor: Some(EventActor::Assistant),
                name: Some("shell".into()),
                text: Some("ls -la".into()),
                details: json!({}),
            },
        ],
        context_manifest: Some(ContextManifestPayload {
            items: vec![
                item(ContextLayer::Injected, "inj:AGENTS.md", Some("注入原文")),
                item(ContextLayer::OnDiskPossible, "disk:CLAUDE.md", None),
            ],
            ..Default::default()
        }),
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

fn usage(session_id: &str, project: &str, at: &str, input: i64, cost: f64) -> UsageRecordPayload {
    let tokens = UsageTokens {
        input,
        output: 0,
        cache_read: 0,
        cache_creation: 0,
        reasoning: 0,
        total: input,
    };
    UsageRecordPayload {
        fingerprint: usage_fingerprint("codex", "/s/a.jsonl", at, "team-model", &tokens),
        occurred_at: at.into(),
        source: "codex".into(),
        model: "team-model".into(),
        provider: "".into(),
        project: project.into(),
        session_id: session_id.into(),
        source_file: "/s/a.jsonl".into(),
        tokens,
        native_cost: None,
        cost_snapshot: Some(cost),
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

async fn get(app: &Router, token: &str, path: &str) -> (StatusCode, Value) {
    call(app, Method::GET, path, Some(token), None).await
}

async fn admin_post(app: &Router, token: &str, path: &str, body: Value) -> (StatusCode, Value) {
    call(app, Method::POST, path, Some(token), Some(body)).await
}

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
            "input": 2e-6, "output": 0.0, "cache_read": 0.0, "cache_creation": 0.0,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    Team {
        app,
        admin,
        alice,
        bob,
        alice_id,
        bob_id,
    }
}

async fn session_row_id(app: &Router, token: &str, session_id: &str) -> i64 {
    let (_, body) = get(app, token, "/api/v1/sessions").await;
    body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == session_id)
        .unwrap_or_else(|| panic!("没有会话 {session_id}：{body}"))["id"]
        .as_i64()
        .unwrap()
}

async fn project_id_named(app: &Router, admin: &str, name: &str) -> i64 {
    let (status, body) = get(app, admin, "/api/v1/admin/projects").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("没有项目 {name}：{body}"))["id"]
        .as_i64()
        .unwrap()
}

const REMOTE_A: &str = "git@github.com:team/alpha.git";
const REMOTE_B: &str = "git@github.com:team/beta.git";
const REMOTE_C: &str = "git@github.com:team/gamma.git";

// ---- 会话详情 ----

#[sqlx::test]
async fn detail_has_events_manifest_tiers_and_only_this_sessions_usage(pool: PgPool) {
    let t = team(&pool).await;
    push_session(&t.app, &t.alice, session("s-1", "/w/alpha", Some(REMOTE_A))).await;
    push_session(&t.app, &t.alice, session("s-2", "/w/alpha", Some(REMOTE_A))).await;
    push_usage(
        &t.app,
        &t.alice,
        vec![
            usage("s-1", "/w/alpha", "2026-03-01T09:10:00Z", 1000, 5.0),
            usage("s-1", "/w/alpha", "2026-03-01T09:20:00Z", 3000, 7.0),
            usage("s-2", "/w/alpha", "2026-03-01T09:30:00Z", 500, 1.0),
        ],
    )
    .await;
    let id = session_row_id(&t.app, &t.alice, "s-1").await;

    let (status, body) = get(&t.app, &t.alice, &format!("/api/v1/sessions/{id}")).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["session"]["session_id"], "s-1");
    assert_eq!(body["session"]["project_name"], "alpha");
    assert_eq!(body["redaction_count"], 3);
    assert_eq!(body["source_files"], json!(["/s/a.jsonl"]));
    let events = body["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["text"], "请帮我改一下");
    assert_eq!(events[1]["kind"], "tool_call");
    assert_eq!(events[1]["name"], "shell");

    let items = body["context_manifest"]["items"].as_array().unwrap();
    assert_eq!(items[0]["layer"], "injected");
    assert_eq!(items[0]["content"], "注入原文");
    assert_eq!(items[1]["layer"], "on_disk_possible");
    assert!(items[1].get("content").is_none_or(Value::is_null));
    assert_eq!(body["context_manifest"]["has_injected_snapshot"], true);

    let usage = &body["usage"];
    assert_eq!(usage["totals"]["record_count"], 2);
    assert_eq!(usage["totals"]["total_tokens"], 4000);
    assert!((usage["totals"]["cost_snapshot_total"].as_f64().unwrap() - 12.0).abs() < 1e-9);
    assert!((usage["totals"]["unified_cost_total"].as_f64().unwrap() - 0.008).abs() < 1e-9);
    assert_eq!(usage["by_model"][0]["key"], "team-model");
}

#[sqlx::test]
async fn detail_keeps_the_cached_manifest_flag_and_has_no_manifest_when_none_was_pushed(
    pool: PgPool,
) {
    let t = team(&pool).await;
    let mut cached = session("cached", "/w/alpha", None);
    cached.context_manifest = Some(ContextManifestPayload {
        items: vec![item(ContextLayer::Injected, "inj:x", None)],
        from_cache: true,
        ..Default::default()
    });
    let mut bare = session("bare", "/w/alpha", None);
    bare.context_manifest = None;
    push_session(&t.app, &t.alice, cached).await;
    push_session(&t.app, &t.alice, bare).await;

    let id = session_row_id(&t.app, &t.alice, "cached").await;
    let (_, body) = get(&t.app, &t.alice, &format!("/api/v1/sessions/{id}")).await;
    assert_eq!(body["context_manifest"]["from_cache"], true);
    assert!(body["context_manifest"]["items"][0]
        .get("content")
        .is_none_or(Value::is_null));

    let id = session_row_id(&t.app, &t.alice, "bare").await;
    let (_, body) = get(&t.app, &t.alice, &format!("/api/v1/sessions/{id}")).await;
    assert!(body["context_manifest"].is_null());
    assert_eq!(body["usage"]["totals"]["record_count"], 0);
}

#[sqlx::test]
async fn member_cannot_read_someone_elses_session_but_admin_can(pool: PgPool) {
    let t = team(&pool).await;
    push_session(&t.app, &t.alice, session("s-1", "/w/alpha", None)).await;
    let id = session_row_id(&t.app, &t.alice, "s-1").await;
    let path = format!("/api/v1/sessions/{id}");

    let (status, body) = get(&t.app, &t.bob, &path).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), ApiErrorCode::Forbidden);
    assert!(
        !body.to_string().contains("请帮我改一下"),
        "被拒绝的响应不能带正文"
    );

    assert_eq!(get(&t.app, &t.admin, &path).await.0, StatusCode::OK);
    assert_eq!(get(&t.app, &t.alice, &path).await.0, StatusCode::OK);
}

#[sqlx::test]
async fn detail_needs_a_token_and_unknown_ids_are_404(pool: PgPool) {
    let t = team(&pool).await;
    let (status, body) = call(&t.app, Method::GET, "/api/v1/sessions/1", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), ApiErrorCode::TokenExpired);

    let (status, body) = get(&t.app, &t.admin, "/api/v1/sessions/999999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), ApiErrorCode::NotFound);
}

#[sqlx::test]
async fn detail_usage_does_not_mix_in_the_same_session_id_from_another_member(pool: PgPool) {
    let t = team(&pool).await;
    push_session(&t.app, &t.alice, session("same", "/w/alpha", None)).await;
    push_session(&t.app, &t.bob, session("same", "/w/alpha", None)).await;
    push_usage(
        &t.app,
        &t.alice,
        vec![usage("same", "/w/alpha", "2026-03-01T09:10:00Z", 1000, 5.0)],
    )
    .await;
    push_usage(
        &t.app,
        &t.bob,
        vec![usage("same", "/w/alpha", "2026-03-01T09:11:00Z", 9000, 9.0)],
    )
    .await;
    let (_, list) = get(&t.app, &t.admin, "/api/v1/sessions").await;
    let alice_row = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["account_id"] == t.alice_id)
        .unwrap()["id"]
        .as_i64()
        .unwrap();

    let (_, body) = get(&t.app, &t.admin, &format!("/api/v1/sessions/{alice_row}")).await;

    assert_eq!(body["usage"]["totals"]["total_tokens"], 1000);
}

#[sqlx::test]
async fn member_can_delete_own_session_from_the_detail_and_not_others(pool: PgPool) {
    let t = team(&pool).await;
    push_session(&t.app, &t.alice, session("s-1", "/w/alpha", None)).await;
    let id = session_row_id(&t.app, &t.alice, "s-1").await;
    let path = format!("/api/v1/sessions/{id}");

    let denied = call(&t.app, Method::DELETE, &path, Some(&t.bob), None).await;
    assert_eq!(denied.0, StatusCode::FORBIDDEN);
    let ok = call(&t.app, Method::DELETE, &path, Some(&t.alice), None).await;
    assert_eq!(ok.0, StatusCode::NO_CONTENT);
    assert_eq!(get(&t.app, &t.alice, &path).await.0, StatusCode::NOT_FOUND);
}

// ---- 会话列表过滤 ----

#[sqlx::test]
async fn session_list_filters_by_work_notes_mark_and_by_project(pool: PgPool) {
    let t = team(&pool).await;
    let mut generated = session("gen", "/w/beta", Some(REMOTE_B));
    generated.generated_by_work_notes = true;
    push_session(&t.app, &t.alice, session("own", "/w/alpha", Some(REMOTE_A))).await;
    push_session(&t.app, &t.alice, generated).await;
    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;

    let ids = |body: &Value| -> Vec<String> {
        body["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["session_id"].as_str().unwrap().to_owned())
            .collect()
    };
    let (_, only) = get(
        &t.app,
        &t.alice,
        "/api/v1/sessions?generated_by_work_notes=true",
    )
    .await;
    assert_eq!(ids(&only), ["gen"]);
    assert_eq!(only["total"], 1);
    let (_, without) = get(
        &t.app,
        &t.alice,
        "/api/v1/sessions?generated_by_work_notes=false",
    )
    .await;
    assert_eq!(ids(&without), ["own"]);
    let (_, all) = get(&t.app, &t.alice, "/api/v1/sessions").await;
    assert_eq!(all["total"], 2);
    let (_, in_alpha) = get(
        &t.app,
        &t.alice,
        &format!("/api/v1/sessions?project_id={alpha}"),
    )
    .await;
    assert_eq!(ids(&in_alpha), ["own"]);
    assert_eq!(in_alpha["total"], 1);
}

// ---- 项目页 ----

/// alice 与 bob 都在 alpha 上花了钱，alice 另在 beta 上也花了。
async fn spend_on_projects(t: &Team) {
    push_session(&t.app, &t.alice, session("a1", "/w/alpha", Some(REMOTE_A))).await;
    push_session(&t.app, &t.alice, session("a2", "/w/beta", Some(REMOTE_B))).await;
    push_session(
        &t.app,
        &t.bob,
        session("b1", "/home/bob/alpha", Some(REMOTE_A)),
    )
    .await;
    push_usage(
        &t.app,
        &t.alice,
        vec![
            usage("a1", "/w/alpha", "2026-03-01T09:10:00Z", 1000, 1.0),
            usage("a2", "/w/beta", "2026-03-01T09:20:00Z", 7000, 1.0),
        ],
    )
    .await;
    push_usage(
        &t.app,
        &t.bob,
        vec![usage(
            "b1",
            "/home/bob/alpha",
            "2026-03-01T09:30:00Z",
            4000,
            1.0,
        )],
    )
    .await;
}

#[sqlx::test]
async fn project_page_shows_who_spent_what_for_admin(pool: PgPool) {
    let t = team(&pool).await;
    spend_on_projects(&t).await;
    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;

    let (status, body) = get(&t.app, &t.admin, &format!("/api/v1/projects/{alpha}")).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["project"]["name"], "alpha");
    assert_eq!(body["project"]["git_remote"], "github.com/team/alpha");
    assert_eq!(body["summary"]["totals"]["total_tokens"], 5000);
    let by_account = body["summary"]["by_account"].as_array().unwrap();
    let tokens = |name: &str| {
        by_account.iter().find(|a| a["key"] == name).unwrap()["total_tokens"]
            .as_i64()
            .unwrap()
    };
    assert_eq!(by_account.len(), 2);
    assert_eq!(tokens("alice"), 1000);
    assert_eq!(tokens("bob"), 4000);
    assert!(
        body["summary"]["totals"]["unified_cost_total"]
            .as_f64()
            .unwrap()
            > 0.0
    );
}

#[sqlx::test]
async fn project_page_for_a_member_only_counts_their_own_usage(pool: PgPool) {
    let t = team(&pool).await;
    spend_on_projects(&t).await;
    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;
    let path = format!("/api/v1/projects/{alpha}");

    let (status, body) = get(&t.app, &t.alice, &path).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["summary"]["totals"]["total_tokens"], 1000);
    let accounts = body["summary"]["by_account"].as_array().unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["key"], "alice");

    let (status, body) = get(&t.app, &t.alice, &format!("{path}?account_id={}", t.bob_id)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[sqlx::test]
async fn member_cannot_open_a_project_they_have_no_data_in(pool: PgPool) {
    let t = team(&pool).await;
    spend_on_projects(&t).await;
    let beta = project_id_named(&t.app, &t.admin, "beta").await;

    // beta 只有 alice 的数据；bob 不该靠 id 看到仓库名与 remote。
    let (status, body) = get(&t.app, &t.bob, &format!("/api/v1/projects/{beta}")).await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(!body.to_string().contains("github.com/team/beta"));
    assert_eq!(
        get(&t.app, &t.alice, &format!("/api/v1/projects/{beta}"))
            .await
            .0,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn project_page_unknown_or_merged_project_is_404(pool: PgPool) {
    let t = team(&pool).await;
    spend_on_projects(&t).await;
    assert_eq!(
        get(&t.app, &t.admin, "/api/v1/projects/999999").await.0,
        StatusCode::NOT_FOUND
    );

    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;
    let beta = project_id_named(&t.app, &t.admin, "beta").await;
    let merged = admin_post(
        &t.app,
        &t.admin,
        &format!("/api/v1/admin/projects/{beta}/merge"),
        json!({ "into_project_id": alpha }),
    )
    .await;
    assert_eq!(merged.0, StatusCode::OK, "{}", merged.1);

    assert_eq!(
        get(&t.app, &t.admin, &format!("/api/v1/projects/{beta}"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn project_page_needs_a_token(pool: PgPool) {
    let t = team(&pool).await;
    let (status, _) = call(&t.app, Method::GET, "/api/v1/projects/1", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---- 管理员：列表、改名、合并 ----

#[sqlx::test]
async fn members_cannot_list_rename_or_merge_projects(pool: PgPool) {
    let t = team(&pool).await;
    spend_on_projects(&t).await;
    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;
    let beta = project_id_named(&t.app, &t.admin, "beta").await;

    let list = get(&t.app, &t.alice, "/api/v1/admin/projects").await;
    let rename = call(
        &t.app,
        Method::PUT,
        &format!("/api/v1/admin/projects/{alpha}"),
        Some(&t.alice),
        Some(json!({ "name": "hijack" })),
    )
    .await;
    let merge = admin_post(
        &t.app,
        &t.alice,
        &format!("/api/v1/admin/projects/{beta}/merge"),
        json!({ "into_project_id": alpha }),
    )
    .await;

    for (status, body) in [list, rename, merge] {
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(error_code(&body), ApiErrorCode::Forbidden);
    }
    let (_, projects) = get(&t.app, &t.admin, "/api/v1/admin/projects").await;
    assert_eq!(projects.as_array().unwrap().len(), 2, "没有项目被改动");
    assert_eq!(project_id_named(&t.app, &t.admin, "alpha").await, alpha);
}

#[sqlx::test]
async fn admin_project_list_counts_data_and_hides_merged_projects(pool: PgPool) {
    let t = team(&pool).await;
    spend_on_projects(&t).await;
    let (_, body) = get(&t.app, &t.admin, "/api/v1/admin/projects").await;
    let alpha = body
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "alpha")
        .unwrap();
    assert_eq!(alpha["session_count"], 2);
    assert_eq!(alpha["usage_record_count"], 2);
    assert_eq!(alpha["merged_count"], 0);
    assert_eq!(alpha["key"], "git:github.com/team/alpha");
}

#[sqlx::test]
async fn rename_sticks_even_when_the_same_project_is_pushed_again(pool: PgPool) {
    let t = team(&pool).await;
    spend_on_projects(&t).await;
    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;

    let (status, body) = call(
        &t.app,
        Method::PUT,
        &format!("/api/v1/admin/projects/{alpha}"),
        Some(&t.admin),
        Some(json!({ "name": "  阿尔法  " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "阿尔法");
    push_session(&t.app, &t.alice, session("a3", "/w/alpha", Some(REMOTE_A))).await;

    let (_, page) = get(&t.app, &t.admin, &format!("/api/v1/projects/{alpha}")).await;
    assert_eq!(page["project"]["name"], "阿尔法");
}

#[sqlx::test]
async fn rename_rejects_blank_names_unknown_projects_and_merged_ones(pool: PgPool) {
    let t = team(&pool).await;
    spend_on_projects(&t).await;
    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;
    let beta = project_id_named(&t.app, &t.admin, "beta").await;
    let rename = |id: i64, name: String| {
        let app = t.app.clone();
        let admin = t.admin.clone();
        async move {
            call(
                &app,
                Method::PUT,
                &format!("/api/v1/admin/projects/{id}"),
                Some(&admin),
                Some(json!({ "name": name })),
            )
            .await
        }
    };

    assert_eq!(rename(alpha, "   ".into()).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        rename(alpha, "x".repeat(201)).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(rename(999_999, "x".into()).await.0, StatusCode::NOT_FOUND);
    admin_post(
        &t.app,
        &t.admin,
        &format!("/api/v1/admin/projects/{beta}/merge"),
        json!({ "into_project_id": alpha }),
    )
    .await;
    assert_eq!(rename(beta, "x".into()).await.0, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn merge_moves_history_and_later_pushes_land_in_the_target(pool: PgPool) {
    let t = team(&pool).await;
    spend_on_projects(&t).await;
    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;
    let beta = project_id_named(&t.app, &t.admin, "beta").await;

    let (status, body) = admin_post(
        &t.app,
        &t.admin,
        &format!("/api/v1/admin/projects/{beta}/merge"),
        json!({ "into_project_id": alpha }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["project"]["id"], alpha);
    assert_eq!(body["sessions_moved"], 1);
    assert_eq!(body["usage_records_moved"], 1);

    let (_, page) = get(&t.app, &t.admin, &format!("/api/v1/projects/{alpha}")).await;
    assert_eq!(page["summary"]["totals"]["total_tokens"], 12000);

    // 被合并项目的归并键还在：同一个 remote 再推送，仍然归到目标，不会长出第二个 beta。
    push_session(&t.app, &t.alice, session("a4", "/w/beta", Some(REMOTE_B))).await;
    push_usage(
        &t.app,
        &t.alice,
        vec![usage("a4", "/w/beta", "2026-03-02T09:00:00Z", 100, 1.0)],
    )
    .await;
    let (_, projects) = get(&t.app, &t.admin, "/api/v1/admin/projects").await;
    let names: Vec<&str> = projects
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["alpha"]);
    let alpha_row = &projects[0];
    assert_eq!(alpha_row["session_count"], 4);
    assert_eq!(alpha_row["usage_record_count"], 4);
    assert_eq!(alpha_row["merged_count"], 1);
}

#[sqlx::test]
async fn merge_also_catches_directory_fallback_pushes_from_a_new_machine(pool: PgPool) {
    let t = team(&pool).await;
    // 没有 remote，只有目录名：key 是 dir:scratch。
    push_session(&t.app, &t.alice, session("d1", "/w/scratch", None)).await;
    push_session(&t.app, &t.alice, session("a1", "/w/alpha", Some(REMOTE_A))).await;
    let scratch = project_id_named(&t.app, &t.admin, "scratch").await;
    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;
    let merged = admin_post(
        &t.app,
        &t.admin,
        &format!("/api/v1/admin/projects/{scratch}/merge"),
        json!({ "into_project_id": alpha }),
    )
    .await;
    assert_eq!(merged.0, StatusCode::OK, "{}", merged.1);

    push_session(&t.app, &t.bob, session("d2", "/home/bob/scratch", None)).await;
    push_usage(
        &t.app,
        &t.alice,
        vec![usage("d1", "/w/scratch", "2026-03-02T09:00:00Z", 100, 1.0)],
    )
    .await;

    let (_, projects) = get(&t.app, &t.admin, "/api/v1/admin/projects").await;
    assert_eq!(projects.as_array().unwrap().len(), 1);
    assert_eq!(projects[0]["session_count"], 3);
    assert_eq!(projects[0]["usage_record_count"], 1);
}

#[sqlx::test]
async fn merging_again_keeps_every_alias_pointing_at_the_final_target(pool: PgPool) {
    let t = team(&pool).await;
    push_session(&t.app, &t.alice, session("a1", "/w/alpha", Some(REMOTE_A))).await;
    push_session(&t.app, &t.alice, session("b1", "/w/beta", Some(REMOTE_B))).await;
    push_session(&t.app, &t.alice, session("c1", "/w/gamma", Some(REMOTE_C))).await;
    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;
    let beta = project_id_named(&t.app, &t.admin, "beta").await;
    let gamma = project_id_named(&t.app, &t.admin, "gamma").await;
    let merge = |from: i64, into: i64| {
        let app = t.app.clone();
        let admin = t.admin.clone();
        async move {
            admin_post(
                &app,
                &admin,
                &format!("/api/v1/admin/projects/{from}/merge"),
                json!({ "into_project_id": into }),
            )
            .await
        }
    };

    assert_eq!(merge(alpha, beta).await.0, StatusCode::OK);
    assert_eq!(merge(beta, gamma).await.0, StatusCode::OK);
    // alpha 先被并进 beta，beta 又并进 gamma：alpha 的 remote 此后也直接归到 gamma。
    push_session(&t.app, &t.alice, session("a2", "/w/alpha", Some(REMOTE_A))).await;

    let (_, projects) = get(&t.app, &t.admin, "/api/v1/admin/projects").await;
    assert_eq!(projects.as_array().unwrap().len(), 1);
    assert_eq!(projects[0]["id"], gamma);
    assert_eq!(projects[0]["session_count"], 4);
    assert_eq!(projects[0]["merged_count"], 2);
}

#[sqlx::test]
async fn merge_refuses_self_unknown_and_already_merged_projects(pool: PgPool) {
    let t = team(&pool).await;
    spend_on_projects(&t).await;
    let alpha = project_id_named(&t.app, &t.admin, "alpha").await;
    let beta = project_id_named(&t.app, &t.admin, "beta").await;
    let merge = |from: i64, into: i64| {
        let app = t.app.clone();
        let admin = t.admin.clone();
        async move {
            admin_post(
                &app,
                &admin,
                &format!("/api/v1/admin/projects/{from}/merge"),
                json!({ "into_project_id": into }),
            )
            .await
        }
    };

    assert_eq!(merge(alpha, alpha).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(merge(alpha, 999_999).await.0, StatusCode::NOT_FOUND);
    assert_eq!(merge(999_999, alpha).await.0, StatusCode::NOT_FOUND);
    assert_eq!(merge(beta, alpha).await.0, StatusCode::OK);
    // 被合并的不能再当来源，也不能再当目标。
    assert_eq!(merge(beta, alpha).await.0, StatusCode::CONFLICT);
    assert_eq!(merge(alpha, beta).await.0, StatusCode::CONFLICT);

    let (_, projects) = get(&t.app, &t.admin, "/api/v1/admin/projects").await;
    assert_eq!(projects.as_array().unwrap().len(), 1, "失败的合并不留痕");
}
