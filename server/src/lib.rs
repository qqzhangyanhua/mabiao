//! 码表远程服务（ADR 0026）。一个部署对应一个团队。
//!
//! 含远程账号、登录与鉴权，以及推送接收；统一价目与只读网页是后续票。

pub mod accounts;
pub mod api;
pub mod auth;
pub mod coverage;
pub mod db;
pub mod devices;
pub mod error;
pub mod password;
pub mod projects;
pub mod push;
pub mod routes;
pub mod sessions;
pub mod tokens;
pub mod usage;

use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

pub use routes::router;
