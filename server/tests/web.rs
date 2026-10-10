//! 管理网页的静态文件托管：不抢 API 的路由，也不能读到目录之外的文件。

mod common;

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use common::*;
use http_body_util::BodyExt;
use mabiao_server::{router_with_web, AppState};
use sqlx::PgPool;
use tower::ServiceExt;

/// 返回 `(网页目录, 它旁边的机密文件所在目录)`。目录名带随机后缀，测试之间互不干扰。
fn site(tag: &str) -> (PathBuf, PathBuf) {
    let mut nonce = [0u8; 6];
    getrandom::fill(&mut nonce).unwrap();
    let suffix: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let root = std::env::temp_dir().join(format!("mabiao-web-{tag}-{suffix}"));
    let web = root.join("dist");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("index.html"), "<html>码表</html>").unwrap();
    std::fs::write(web.join("assets").join("app.js"), "console.log(1)").unwrap();
    std::fs::write(root.join("secret.txt"), "不该被读到").unwrap();
    (web, root)
}

fn with_web(pool: &PgPool, web: PathBuf) -> Router {
    router_with_web(AppState { pool: pool.clone() }, web)
}

async fn fetch(app: &Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[sqlx::test]
async fn the_site_is_served_from_the_root_and_assets_by_path(pool: PgPool) {
    let (web, root) = site("serve");
    let app = with_web(&pool, web);
    let (status, body) = fetch(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("码表"));
    let (status, body) = fetch(&app, "/assets/app.js").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("console.log"));
    std::fs::remove_dir_all(root).unwrap();
}

#[sqlx::test]
async fn api_routes_still_win_and_unknown_api_paths_are_404_not_the_page(pool: PgPool) {
    let (web, root) = site("api");
    seed_admin(&pool, "root").await;
    let app = with_web(&pool, web);

    let (status, body) = fetch(&app, "/healthz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "ok");

    let (status, body) = fetch(&app, "/api/v1/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!body.contains("码表"));

    let (status, _) = login(&app, "root", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    std::fs::remove_dir_all(root).unwrap();
}

#[sqlx::test]
async fn files_outside_the_site_directory_are_not_served(pool: PgPool) {
    let (web, root) = site("escape");
    let app = with_web(&pool, web);
    for uri in [
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/assets/../../secret.txt",
        "/%2e%2e%2fsecret.txt",
    ] {
        let (status, body) = fetch(&app, uri).await;
        assert!(!body.contains("不该被读到"), "{uri} → {status}");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[sqlx::test]
async fn without_a_site_directory_the_server_is_api_only(pool: PgPool) {
    let app = app(&pool);
    let (status, _) = fetch(&app, "/").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, Method::GET, "/healthz", None, None).await;
    assert_eq!(status, StatusCode::OK);
}
