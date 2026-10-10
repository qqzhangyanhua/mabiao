//! 集成测试共用：用真路由 + 真 PostgreSQL，经 `tower::oneshot` 发请求。
#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mabiao_server::{accounts, router, AppState};
use push_protocol::{ApiErrorCode, RemoteRole, PROTOCOL_VERSION};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

pub const PASSWORD: &str = "correct-horse-battery";

pub fn app(pool: &PgPool) -> Router {
    router(AppState { pool: pool.clone() })
}

pub async fn call(
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

pub async fn login(app: &Router, account: &str, password: &str) -> (StatusCode, Value) {
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

pub async fn token_of(app: &Router, account: &str) -> String {
    let (status, body) = login(app, account, PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["token"].as_str().unwrap().to_owned()
}

pub async fn seed_admin(pool: &PgPool, name: &str) -> i64 {
    accounts::create(pool, name, PASSWORD, RemoteRole::Admin)
        .await
        .unwrap()
        .id
}

pub async fn seed_member(pool: &PgPool, name: &str) -> i64 {
    accounts::create(pool, name, PASSWORD, RemoteRole::Member)
        .await
        .unwrap()
        .id
}

pub fn error_code(body: &Value) -> ApiErrorCode {
    serde_json::from_value(body["code"].clone()).unwrap()
}
