//! 连真 PostgreSQL（`DATABASE_URL`）：每个测试一个临时库，迁移自动跑。

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mabiao_server::{accounts, devices, router, AppState};
use push_protocol::{ApiErrorCode, DeviceInfo, RemoteRole, PROTOCOL_VERSION};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

const PASSWORD: &str = "correct-horse-battery";

fn app(pool: &PgPool) -> Router {
    router(AppState { pool: pool.clone() })
}

async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn login(app: &Router, account: &str, password: &str) -> (StatusCode, Value) {
    call(
        app,
        Method::POST,
        "/api/v1/login",
        None,
        Some(json!({
            "protocol_version": PROTOCOL_VERSION,
            "account": account,
            "password": password,
        })),
    )
    .await
}

async fn token_of(app: &Router, account: &str) -> String {
    let (status, body) = login(app, account, PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["token"].as_str().unwrap().to_owned()
}

async fn seed_admin(pool: &PgPool, name: &str) -> i64 {
    accounts::create(pool, name, PASSWORD, RemoteRole::Admin)
        .await
        .unwrap()
        .id
}

async fn seed_member(pool: &PgPool, name: &str) -> i64 {
    accounts::create(pool, name, PASSWORD, RemoteRole::Member)
        .await
        .unwrap()
        .id
}

fn error_code(body: &Value) -> ApiErrorCode {
    serde_json::from_value(body["code"].clone()).unwrap()
}

#[sqlx::test]
async fn login_returns_a_30_day_token_with_role(pool: PgPool) {
    seed_admin(&pool, "root").await;
    let app = app(&pool);

    let (status, body) = login(&app, "root", PASSWORD).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["role"], "admin");
    assert_eq!(body["account"], "root");
    assert!(body["token"].as_str().unwrap().starts_with("mbt_"));
    let expires = chrono::DateTime::parse_from_rfc3339(body["expires_at"].as_str().unwrap())
        .unwrap()
        .to_utc();
    let days = (expires - chrono::Utc::now()).num_hours() as f64 / 24.0;
    assert!((29.9..=30.0).contains(&days), "有效期 {days} 天");
}

#[sqlx::test]
async fn login_is_case_insensitive_on_account_name(pool: PgPool) {
    seed_member(&pool, "Alice").await;
    let (status, body) = login(&app(&pool), "aLiCe", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["account"], "Alice");
}

#[sqlx::test]
async fn bad_password_unknown_account_and_disabled_account_look_the_same(pool: PgPool) {
    seed_admin(&pool, "root").await;
    let member = seed_member(&pool, "bob").await;
    let app = app(&pool);
    let admin_token = token_of(&app, "root").await;
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/api/v1/admin/accounts/{member}/deactivate"),
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let cases = [
        login(&app, "root", "wrong-password-here").await,
        login(&app, "nobody", PASSWORD).await,
        login(&app, "bob", PASSWORD).await,
    ];
    for (status, body) in cases {
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(error_code(&body), ApiErrorCode::InvalidCredentials);
        assert_eq!(body["message"], "账号或密码错误");
    }
}

#[sqlx::test]
async fn incompatible_protocol_version_is_rejected(pool: PgPool) {
    seed_admin(&pool, "root").await;
    let app = app(&pool);
    for version in [0, PROTOCOL_VERSION + 1] {
        let (status, body) = call(
            &app,
            Method::POST,
            "/api/v1/login",
            None,
            Some(json!({"protocol_version": version, "account": "root", "password": PASSWORD})),
        )
        .await;
        assert_eq!(status, StatusCode::UPGRADE_REQUIRED, "版本 {version}");
        assert_eq!(error_code(&body), ApiErrorCode::UnsupportedProtocolVersion);
    }
}

#[sqlx::test]
async fn malformed_login_body_is_an_api_error_not_plain_text(pool: PgPool) {
    let (status, body) = call(
        &app(&pool),
        Method::POST,
        "/api/v1/login",
        None,
        Some(json!({"account": "root"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), ApiErrorCode::InvalidPayload);
}

#[sqlx::test]
async fn there_is_no_self_service_registration(pool: PgPool) {
    let app = app(&pool);
    let body = json!({"account": "mallory", "password": PASSWORD});
    for uri in [
        "/api/v1/register",
        "/api/v1/signup",
        "/api/v1/accounts",
        "/register",
    ] {
        let (status, _) = call(&app, Method::POST, uri, None, Some(body.clone())).await;
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
            "{uri} 不该存在，得到 {status}"
        );
    }
    // 管理员建号接口没有 token 也进不去。
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/v1/admin/accounts",
        None,
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(accounts::find_by_name(&pool, "mallory")
        .await
        .unwrap()
        .is_none());
}

#[sqlx::test]
async fn protected_routes_need_a_valid_unexpired_token(pool: PgPool) {
    seed_member(&pool, "carol").await;
    let app = app(&pool);

    let (status, body) = call(&app, Method::GET, "/api/v1/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), ApiErrorCode::TokenExpired);

    let (status, _) = call(&app, Method::GET, "/api/v1/me", Some("mbt_garbage"), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let token = token_of(&app, "carol").await;
    let (status, body) = call(&app, Method::GET, "/api/v1/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["account"], "carol");
    assert!(body.get("password_hash").is_none());

    sqlx::query("UPDATE login_tokens SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = call(&app, Method::GET, "/api/v1/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), ApiErrorCode::TokenExpired);
}

#[sqlx::test]
async fn token_is_stored_only_as_a_hash(pool: PgPool) {
    seed_member(&pool, "dave").await;
    let token = token_of(&app(&pool), "dave").await;
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM login_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(stored, token.as_bytes());
    assert_eq!(stored, mabiao_server::tokens::hash(&token));
}

#[sqlx::test]
async fn passwords_are_stored_as_argon2_hashes(pool: PgPool) {
    seed_member(&pool, "erin").await;
    let stored: String = sqlx::query_scalar("SELECT password_hash FROM remote_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(stored.starts_with("$argon2id$"));
    assert!(!stored.contains(PASSWORD));
}

#[sqlx::test]
async fn members_are_refused_on_admin_routes(pool: PgPool) {
    seed_member(&pool, "frank").await;
    let app = app(&pool);
    let token = token_of(&app, "frank").await;

    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/admin/accounts",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), ApiErrorCode::Forbidden);

    let (status, _) = call(
        &app,
        Method::POST,
        "/api/v1/admin/accounts",
        Some(&token),
        Some(json!({"account": "sneaky", "password": PASSWORD})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(accounts::find_by_name(&pool, "sneaky")
        .await
        .unwrap()
        .is_none());
}

#[sqlx::test]
async fn admin_creates_members_who_can_then_log_in(pool: PgPool) {
    seed_admin(&pool, "root").await;
    let app = app(&pool);
    let admin_token = token_of(&app, "root").await;

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/admin/accounts",
        Some(&admin_token),
        Some(json!({"account": "grace", "password": PASSWORD})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["role"], "member", "管理员接口只建成员");
    assert_eq!(body["active"], true);
    assert!(body.get("password_hash").is_none());

    let (status, body) = login(&app, "grace", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["role"], "member");

    let (status, list) = call(
        &app,
        Method::GET,
        "/api/v1/admin/accounts",
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<_> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["account"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["root", "grace"]);
}

#[sqlx::test]
async fn account_creation_validates_and_rejects_duplicates_ignoring_case(pool: PgPool) {
    seed_admin(&pool, "root").await;
    seed_member(&pool, "Heidi").await;
    let app = app(&pool);
    let token = token_of(&app, "root").await;
    let create = |account: &'static str, password: &'static str| {
        let app = app.clone();
        let token = token.clone();
        async move {
            call(
                &app,
                Method::POST,
                "/api/v1/admin/accounts",
                Some(&token),
                Some(json!({"account": account, "password": password})),
            )
            .await
        }
    };

    let (status, body) = create("heidi", PASSWORD).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), ApiErrorCode::Conflict);

    let (status, body) = create("ivan", "short").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), ApiErrorCode::InvalidPayload);

    let (status, _) = create("bad name!", PASSWORD).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn deactivating_a_member_revokes_tokens_and_blocks_login(pool: PgPool) {
    seed_admin(&pool, "root").await;
    let member = seed_member(&pool, "judy").await;
    let app = app(&pool);
    let admin_token = token_of(&app, "root").await;
    let member_token = token_of(&app, "judy").await;

    let (status, body) = call(&app, Method::GET, "/api/v1/me", Some(&member_token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let uri = format!("/api/v1/admin/accounts/{member}/deactivate");
    let (status, body) = call(&app, Method::POST, &uri, Some(&admin_token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], false);
    assert!(body["deactivated_at"].is_string());

    let (status, _) = call(&app, Method::GET, "/api/v1/me", Some(&member_token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = login(&app, "judy", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let tokens_left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM login_tokens WHERE account_id = $1")
            .bind(member)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(tokens_left, 0);

    // 重复停用结果相同，账号行还在（推送来的数据要继续归它）。
    let (status, again) = call(&app, Method::POST, &uri, Some(&admin_token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["deactivated_at"], body["deactivated_at"]);
}

#[sqlx::test]
async fn admin_cannot_deactivate_self_and_unknown_id_is_404(pool: PgPool) {
    let admin = seed_admin(&pool, "root").await;
    let app = app(&pool);
    let token = token_of(&app, "root").await;

    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/api/v1/admin/accounts/{admin}/deactivate"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), ApiErrorCode::Conflict);

    let (status, _) = call(
        &app,
        Method::POST,
        "/api/v1/admin/accounts/999999/deactivate",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, Method::GET, "/api/v1/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
}

fn device(id: &str, name: &str) -> DeviceInfo {
    DeviceInfo {
        device_id: id.into(),
        device_name: name.into(),
    }
}

#[sqlx::test]
async fn members_see_only_their_own_devices_and_admins_see_all(pool: PgPool) {
    let admin = seed_admin(&pool, "root").await;
    let alice = seed_member(&pool, "alice").await;
    let bob = seed_member(&pool, "bob").await;
    devices::upsert(&pool, alice, &device("a-1", "alice 的 MacBook"))
        .await
        .unwrap();
    devices::upsert(&pool, bob, &device("b-1", "bob 的 ThinkPad"))
        .await
        .unwrap();
    let app = app(&pool);
    let alice_token = token_of(&app, "alice").await;
    let admin_token = token_of(&app, "root").await;
    let devices_uri = |id: i64| format!("/api/v1/accounts/{id}/devices");

    let (status, body) = call(
        &app,
        Method::GET,
        &devices_uri(alice),
        Some(&alice_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["device_id"], "a-1");

    let (status, body) = call(
        &app,
        Method::GET,
        &devices_uri(bob),
        Some(&alice_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), ApiErrorCode::Forbidden);

    for (id, expected) in [(alice, "a-1"), (bob, "b-1")] {
        let (status, body) = call(
            &app,
            Method::GET,
            &devices_uri(id),
            Some(&admin_token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body[0]["device_id"], expected);
    }
    let (status, body) = call(
        &app,
        Method::GET,
        &devices_uri(admin),
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));

    let (status, _) = call(
        &app,
        Method::GET,
        &devices_uri(999_999),
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn device_upsert_registers_once_and_refreshes_the_name(pool: PgPool) {
    let alice = seed_member(&pool, "alice").await;
    devices::upsert(&pool, alice, &device("a-1", "旧名字"))
        .await
        .unwrap();
    devices::upsert(&pool, alice, &device("a-1", "新名字"))
        .await
        .unwrap();
    let rows = devices::list(&pool, alice).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].device_name, "新名字");
    assert!(rows[0].last_seen_at >= rows[0].first_seen_at);
}

#[sqlx::test]
async fn cli_style_admin_creation_rejects_duplicates(pool: PgPool) {
    let first = accounts::create(&pool, "root", PASSWORD, RemoteRole::Admin)
        .await
        .unwrap();
    assert_eq!(first.role(), RemoteRole::Admin);
    let again = accounts::create(&pool, "ROOT", PASSWORD, RemoteRole::Admin).await;
    assert_eq!(again.unwrap_err().code(), ApiErrorCode::Conflict);
}

#[sqlx::test]
async fn healthz_checks_the_database(pool: PgPool) {
    let (status, _) = call(&app(&pool), Method::GET, "/healthz", None, None).await;
    assert_eq!(status, StatusCode::OK);
}
