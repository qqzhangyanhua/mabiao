use chrono::{DateTime, Utc};
use push_protocol::RemoteRole;
use sqlx::PgPool;

use crate::error::AppError;
use crate::password;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AccountRow {
    pub id: i64,
    pub account: String,
    pub password_hash: String,
    role: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub deactivated_at: Option<DateTime<Utc>>,
}

impl AccountRow {
    pub fn role(&self) -> RemoteRole {
        parse_role(&self.role)
    }
}

pub fn role_str(role: RemoteRole) -> &'static str {
    match role {
        RemoteRole::Admin => "admin",
        RemoteRole::Member => "member",
    }
}

/// 库里有 CHECK 约束，走不到未知值；真走到了按最低权限处理。
pub fn parse_role(value: &str) -> RemoteRole {
    match value {
        "admin" => RemoteRole::Admin,
        _ => RemoteRole::Member,
    }
}

/// HTTP 管理接口与命令行共用的建号入口：校验、哈希、落库。
pub async fn create(
    pool: &PgPool,
    account: &str,
    password: &str,
    role: RemoteRole,
) -> Result<AccountRow, AppError> {
    password::validate_account_name(account)?;
    password::validate_password(password)?;
    let owned = password.to_owned();
    let password_hash = tokio::task::spawn_blocking(move || password::hash(&owned))
        .await
        .map_err(AppError::internal)??;

    let inserted = sqlx::query_as::<_, AccountRow>(
        "INSERT INTO remote_accounts (account, password_hash, role)
         VALUES ($1, $2, $3)
         RETURNING *",
    )
    .bind(account)
    .bind(password_hash)
    .bind(role_str(role))
    .fetch_one(pool)
    .await;

    match inserted {
        Ok(row) => Ok(row),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            Err(AppError::conflict("账号名已被占用"))
        }
        Err(error) => Err(error.into()),
    }
}

pub async fn find_by_name(pool: &PgPool, account: &str) -> Result<Option<AccountRow>, AppError> {
    Ok(
        sqlx::query_as("SELECT * FROM remote_accounts WHERE lower(account) = lower($1)")
            .bind(account)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn get(pool: &PgPool, id: i64) -> Result<Option<AccountRow>, AppError> {
    Ok(
        sqlx::query_as("SELECT * FROM remote_accounts WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn list(pool: &PgPool) -> Result<Vec<AccountRow>, AppError> {
    Ok(sqlx::query_as("SELECT * FROM remote_accounts ORDER BY id")
        .fetch_all(pool)
        .await?)
}

/// 停用并立刻吊销该账号所有 token。重复调用结果相同。
pub async fn deactivate(pool: &PgPool, id: i64) -> Result<Option<AccountRow>, AppError> {
    let mut tx = pool.begin().await?;
    let row: Option<AccountRow> = sqlx::query_as(
        "UPDATE remote_accounts
         SET active = FALSE, deactivated_at = COALESCE(deactivated_at, now())
         WHERE id = $1
         RETURNING *",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if row.is_some() {
        sqlx::query("DELETE FROM login_tokens WHERE account_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(row)
}
