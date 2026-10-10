//! 网页用的聚合接口：按天 / 人 / 来源 / 模型 / 项目拆分、会话列表、越权（连真 PostgreSQL）。

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

struct Row<'a> {
    at: &'a str,
    source: &'a str,
    model: &'a str,
    project: &'a str,
    input: i64,
    client_cost: f64,
}

impl<'a> Row<'a> {
    fn new(at: &'a str, source: &'a str, model: &'a str, project: &'a str) -> Self {
        Self {
            at,
            source,
            model,
            project,
            input: 1000,
            client_cost: 9.0,
        }
    }

    fn payload(&self, file: &str) -> UsageRecordPayload {
        let tokens = UsageTokens {
            input: self.input,
            output: 0,
            cache_read: 0,
            cache_creation: 0,
            reasoning: 0,
            total: self.input,
        };
        UsageRecordPayload {
            fingerprint: usage_fingerprint(self.source, file, self.at, self.model, &tokens),
            occurred_at: self.at.into(),
            source: self.source.into(),
            model: self.model.into(),
            provider: "".into(),
            project: self.project.into(),
            session_id: "s-1".into(),
            source_file: file.into(),
            tokens,
            native_cost: None,
            cost_snapshot: Some(self.client_cost),
            pricing_source: PricingSource::Exact,
        }
    }
}

fn device() -> DeviceInfo {
    DeviceInfo {
        device_id: "d1".into(),
        device_name: "办公室".into(),
    }
}

async fn push_usage(app: &Router, token: &str, rows: &[Row<'_>]) {
    let records = rows
        .iter()
        .enumerate()
        .map(|(n, row)| row.payload(&format!("/f{n}.jsonl")))
        .collect();
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

async fn push_session(app: &Router, token: &str, id: &str, source: &str, ended_at: &str) {
    let session = SessionPayload {
        source: source.into(),
        session_id: id.into(),
        title: format!("{id} 标题"),
        project: "/w/mabiao".into(),
        git_remote_url: Some("git@github.com:team/mabiao.git".into()),
        model: "claude-sonnet".into(),
        started_at: "2026-03-01T09:00:00Z".into(),
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
            text: Some("正文不该出现在列表里".into()),
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

async fn set_price(app: &Router, admin: &str, model: &str, input: f64) {
    let (status, body) = call(
        app,
        Method::PUT,
        "/api/v1/admin/pricing/prices",
        Some(admin),
        Some(json!({
            "model": model, "provider": null,
            "input": input, "output": 0.0, "cache_read": 0.0, "cache_creation": 0.0,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn get(app: &Router, token: &str, path: &str) -> (StatusCode, Value) {
    call(app, Method::GET, path, Some(token), None).await
}

fn close(value: &Value, expected: f64) {
    let got = value
        .as_f64()
        .unwrap_or_else(|| panic!("不是数字：{value}"));
    assert!((got - expected).abs() < 1e-12, "{got} != {expected}");
}

fn item<'a>(list: &'a Value, key: &str) -> &'a Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key"] == key)
        .unwrap_or_else(|| panic!("没有 {key}：{list}"))
}

/// admin、alice、bob；alice 与 bob 各有几条消耗记录。
async fn setup(pool: &PgPool) -> (Router, String, String, String) {
    seed_admin(pool, "root").await;
    let alice_id = seed_member(pool, "alice").await;
    let bob_id = seed_member(pool, "bob").await;
    assert!(alice_id < bob_id);
    let app = app(pool);
    let admin = token_of(&app, "root").await;
    let alice = token_of(&app, "alice").await;
    let bob = token_of(&app, "bob").await;
    set_price(&app, &admin, "team-model", 2e-6).await;

    let mut big = Row::new("2026-03-02T10:00:00Z", "codex", "team-model", "/w/mabiao");
    big.input = 3000;
    push_usage(
        &app,
        &alice,
        &[
            Row::new("2026-03-01T10:00:00Z", "codex", "team-model", "/w/mabiao"),
            big,
            Row::new("2026-03-02T11:00:00Z", "claude", "other-model", "/w/other"),
        ],
    )
    .await;
    push_usage(
        &app,
        &bob,
        &[Row::new(
            "2026-03-02T12:00:00Z",
            "codex",
            "team-model",
            "/w/mabiao",
        )],
    )
    .await;
    (app, admin, alice, bob)
}

#[sqlx::test]
async fn admin_summary_splits_the_team_by_day_member_source_model_and_project(pool: PgPool) {
    let (app, admin, _, _) = setup(&pool).await;
    let (status, body) = get(&app, &admin, "/api/v1/usage/summary").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(body["totals"]["record_count"], 4);
    assert_eq!(body["totals"]["total_tokens"], 6000);
    close(&body["totals"]["cost_snapshot_total"], 36.0);
    // team-model 共 5000 token × 2e-6；other-model 无价目，未定价。
    close(&body["totals"]["unified_cost_total"], 0.01);

    let days = body["by_day"].as_array().unwrap();
    assert_eq!(days.len(), 2);
    assert_eq!(days[0]["key"], "2026-03-01");
    assert_eq!(days[1]["key"], "2026-03-02", "按日期升序");
    assert_eq!(days[0]["record_count"], 1);
    assert_eq!(days[1]["record_count"], 3);
    assert_eq!(days[1]["total_tokens"], 5000);
    close(&days[1]["cost_snapshot"], 27.0);
    close(&days[1]["unified_cost"], 0.008);

    let alice = item(&body["by_account"], "alice");
    assert_eq!(alice["record_count"], 3);
    assert_eq!(alice["total_tokens"], 5000);
    assert_eq!(item(&body["by_account"], "bob")["total_tokens"], 1000);
    assert_eq!(item(&body["by_source"], "codex")["record_count"], 3);
    assert_eq!(item(&body["by_source"], "claude")["record_count"], 1);
    let team_model = item(&body["by_model"], "team-model");
    assert_eq!(team_model["total_tokens"], 5000);
    close(&team_model["unified_cost"], 0.01);
    assert_eq!(item(&body["by_model"], "other-model")["unpriced_count"], 1);
}

#[sqlx::test]
async fn breakdowns_are_sorted_by_unified_cost_then_tokens(pool: PgPool) {
    let (app, admin, _, _) = setup(&pool).await;
    let (_, body) = get(&app, &admin, "/api/v1/usage/summary").await;
    let models: Vec<&str> = body["by_model"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["key"].as_str().unwrap())
        .collect();
    assert_eq!(models, ["team-model", "other-model"]);
}

#[sqlx::test]
async fn a_git_remote_moves_that_devices_records_into_the_git_project(pool: PgPool) {
    let (app, admin, alice, _) = setup(&pool).await;
    push_session(&app, &alice, "s-a", "codex", "2026-03-02T10:00:00Z").await;
    let (_, body) = get(&app, &admin, "/api/v1/usage/summary").await;
    let projects = body["by_project"].as_array().unwrap();
    // alice 的会话把她设备上的 /w/mabiao 指到 git 项目，她的两条历史消耗记录一并改挂；
    // bob 另一台设备上的同名目录没有 remote 线索，仍留在目录兜底项目里。
    let git = projects
        .iter()
        .find(|p| p["key"].as_str().unwrap().starts_with("git:"))
        .unwrap_or_else(|| panic!("{projects:?}"));
    assert_eq!(git["record_count"], 2);
    assert_eq!(git["label"], "mabiao");
    let bob_dir = item(&body["by_project"], "dir:mabiao");
    assert_eq!(bob_dir["record_count"], 1);
    assert!(projects.iter().any(|p| p["label"] == "other"));
}

#[sqlx::test]
async fn member_summary_only_covers_their_own_records(pool: PgPool) {
    let (app, _, alice, bob) = setup(&pool).await;
    let (_, mine) = get(&app, &alice, "/api/v1/usage/summary").await;
    assert_eq!(mine["totals"]["record_count"], 3);
    let accounts = mine["by_account"].as_array().unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["key"], "alice");
    let (_, theirs) = get(&app, &bob, "/api/v1/usage/summary").await;
    assert_eq!(theirs["totals"]["record_count"], 1);
}

#[sqlx::test]
async fn a_member_cannot_ask_for_another_accounts_summary(pool: PgPool) {
    let (app, _, alice, _) = setup(&pool).await;
    let (_, me) = get(&app, &alice, "/api/v1/me").await;
    let bob_id = {
        let id: i64 = sqlx::query_scalar("SELECT id FROM remote_accounts WHERE account = 'bob'")
            .fetch_one(&pool)
            .await
            .unwrap();
        id
    };
    let (status, body) = get(
        &app,
        &alice,
        &format!("/api/v1/usage/summary?account_id={bob_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let own = me["id"].as_i64().unwrap();
    let (status, _) = get(
        &app,
        &alice,
        &format!("/api/v1/usage/summary?account_id={own}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[sqlx::test]
async fn admin_can_narrow_the_summary_to_one_member(pool: PgPool) {
    let (app, admin, _, _) = setup(&pool).await;
    let bob_id: i64 = sqlx::query_scalar("SELECT id FROM remote_accounts WHERE account = 'bob'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (_, body) = get(
        &app,
        &admin,
        &format!("/api/v1/usage/summary?account_id={bob_id}"),
    )
    .await;
    assert_eq!(body["totals"]["record_count"], 1);
    assert_eq!(body["by_day"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn summary_honours_from_and_to(pool: PgPool) {
    let (app, admin, _, _) = setup(&pool).await;
    let (_, body) = get(
        &app,
        &admin,
        "/api/v1/usage/summary?from=2026-03-02T00:00:00Z&to=2026-03-02T11:30:00Z",
    )
    .await;
    assert_eq!(body["totals"]["record_count"], 2, "to 不含：12:00 那条不算");
    assert_eq!(body["by_day"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn day_buckets_follow_the_requested_utc_offset(pool: PgPool) {
    let (app, admin, _, _) = setup(&pool).await;
    let days = |body: &Value| -> Vec<String> {
        body["by_day"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["key"].as_str().unwrap().to_owned())
            .collect()
    };
    let (_, utc) = get(&app, &admin, "/api/v1/usage/summary").await;
    assert_eq!(days(&utc), ["2026-03-01", "2026-03-02"]);
    // UTC+14:00：3/1 10:00Z 是 3/2 00:00。
    let (_, east) = get(&app, &admin, "/api/v1/usage/summary?tz_offset_minutes=840").await;
    assert_eq!(days(&east), ["2026-03-02", "2026-03-03"]);
    // UTC-11:00：3/1 10:00Z 是 2/28 23:00，3/2 11:00Z 才刚到 3/2 00:00。
    let (_, west) = get(&app, &admin, "/api/v1/usage/summary?tz_offset_minutes=-660").await;
    assert_eq!(days(&west), ["2026-02-28", "2026-03-01", "2026-03-02"]);
}

#[sqlx::test]
async fn summary_rejects_bad_parameters_and_anonymous_callers(pool: PgPool) {
    let (app, admin, _, _) = setup(&pool).await;
    for query in [
        "?from=yesterday",
        "?tz_offset_minutes=900",
        "?tz_offset_minutes=-900",
        "?tz_offset_minutes=abc",
        "?account_id=abc",
    ] {
        let (status, _) = get(&app, &admin, &format!("/api/v1/usage/summary{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
    let (status, _) = call(&app, Method::GET, "/api/v1/usage/summary", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn an_empty_team_gives_an_empty_summary(pool: PgPool) {
    seed_admin(&pool, "root").await;
    let app = app(&pool);
    let admin = token_of(&app, "root").await;
    let (status, body) = get(&app, &admin, "/api/v1/usage/summary").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["totals"]["record_count"], 0);
    for key in [
        "by_day",
        "by_account",
        "by_source",
        "by_model",
        "by_project",
    ] {
        assert_eq!(body[key].as_array().unwrap().len(), 0, "{key}");
    }
}

// ---------- 会话列表 ----------

#[sqlx::test]
async fn session_list_has_metadata_but_never_the_transcript(pool: PgPool) {
    let (app, _, alice, _) = setup(&pool).await;
    push_session(&app, &alice, "s-a", "codex", "2026-03-02T10:00:00Z").await;
    let (status, body) = get(&app, &alice, "/api/v1/sessions").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 1);
    let session = &body["sessions"][0];
    assert_eq!(session["session_id"], "s-a");
    assert_eq!(session["account"], "alice");
    assert_eq!(session["device_name"], "办公室");
    assert_eq!(session["event_count"], 1);
    assert_eq!(session["source"], "codex");
    assert!(session["id"].is_i64());
    assert!(!body.to_string().contains("正文不该出现在列表里"));
    assert!(session.get("events").is_none());
}

#[sqlx::test]
async fn members_only_list_their_own_sessions_and_admin_sees_everyone(pool: PgPool) {
    let (app, admin, alice, bob) = setup(&pool).await;
    push_session(&app, &alice, "s-a", "codex", "2026-03-02T10:00:00Z").await;
    push_session(&app, &bob, "s-b", "codex", "2026-03-02T10:00:00Z").await;

    let (_, mine) = get(&app, &alice, "/api/v1/sessions").await;
    assert_eq!(mine["total"], 1);
    assert_eq!(mine["sessions"][0]["session_id"], "s-a");

    let (_, all) = get(&app, &admin, "/api/v1/sessions").await;
    assert_eq!(all["total"], 2);

    let bob_id: i64 = sqlx::query_scalar("SELECT id FROM remote_accounts WHERE account = 'bob'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (status, _) = get(
        &app,
        &alice,
        &format!("/api/v1/sessions?account_id={bob_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, only_bob) = get(
        &app,
        &admin,
        &format!("/api/v1/sessions?account_id={bob_id}"),
    )
    .await;
    assert_eq!(only_bob["total"], 1);
    assert_eq!(only_bob["sessions"][0]["session_id"], "s-b");
}

#[sqlx::test]
async fn session_list_is_newest_first_filterable_and_paged(pool: PgPool) {
    let (app, _, alice, _) = setup(&pool).await;
    push_session(&app, &alice, "old", "codex", "2026-03-01T10:00:00Z").await;
    push_session(&app, &alice, "new", "codex", "2026-03-05T10:00:00Z").await;

    let (_, all) = get(&app, &alice, "/api/v1/sessions").await;
    assert_eq!(all["sessions"][0]["session_id"], "new");
    assert_eq!(all["sessions"][1]["session_id"], "old");

    let (_, recent) = get(&app, &alice, "/api/v1/sessions?from=2026-03-03T00:00:00Z").await;
    assert_eq!(recent["total"], 1);

    let (_, page) = get(&app, &alice, "/api/v1/sessions?limit=1&offset=1").await;
    assert_eq!(page["total"], 2, "total 不受分页影响");
    assert_eq!(page["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(page["sessions"][0]["session_id"], "old");
}

#[sqlx::test]
async fn session_list_rejects_bad_parameters_and_anonymous_callers(pool: PgPool) {
    let (app, _, alice, _) = setup(&pool).await;
    for query in ["?limit=0", "?limit=100000", "?offset=-1", "?from=yesterday"] {
        let (status, _) = get(&app, &alice, &format!("/api/v1/sessions{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
    let (status, _) = call(&app, Method::GET, "/api/v1/sessions", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
