//! 码表远程服务（ADR 0026）。一个部署对应一个团队。
//!
//! 本骨架只含远程账号、登录与鉴权；推送接收、统一价目与只读网页是后续票。

pub mod accounts;
pub mod api;
pub mod auth;
pub mod db;
pub mod devices;
pub mod error;
pub mod password;
pub mod routes;
pub mod tokens;

use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

pub use routes::router;
