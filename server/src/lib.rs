//! 码表远程服务（ADR 0026）。一个部署对应一个团队。
//!
//! 含远程账号、登录与鉴权、推送接收，团队价目与统一费用重算，以及管理网页用的聚合接口与静态文件托管。

pub mod accounts;
pub mod api;
pub mod api_detail;
pub mod auth;
pub mod coverage;
pub mod db;
pub mod devices;
pub mod error;
pub mod password;
pub mod project_admin;
pub mod projects;
pub mod push;
pub mod routes;
pub mod sessions;
pub mod summary;
pub mod team_pricing;
pub mod tokens;
pub mod usage;
pub mod usage_query;

use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

pub use routes::{router, router_with_web};
