use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::accounts::parse_role;
use crate::auth::AuthedAccount;
use crate::error::AppError;

/// token 有效期（天）。ADR 0026：过期后桌面端提示重新登录。
pub const TOKEN_TTL_DAYS: i32 = 30;

const TOKEN_PREFIX: &str = "mbt_";

/// 256 位随机数的十六进制，带前缀方便在日志与扫描器里认出来。
pub fn generate() -> Result<String, AppError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(AppError::internal)?;
    let mut token = String::with_capacity(TOKEN_PREFIX.len() + 64);
    token.push_str(TOKEN_PREFIX);
    for byte in bytes {
        token.push_str(&format!("{byte:02x}"));
    }
    Ok(token)
}

/// 库里只存这个哈希。token 本身已是高熵随机数，不需要慢哈希。
pub fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// 签发新 token。过期时间用数据库时钟算，和 `authenticate` 的比较同一个时钟。
pub async fn issue(pool: &PgPool, account_id: i64) -> Result<(String, DateTime<Utc>), AppError> {
    let token = generate()?;
    let expires_at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO login_tokens (account_id, token_hash, expires_at)
         VALUES ($1, $2, now() + make_interval(days => $3))
         RETURNING expires_at",
    )
    .bind(account_id)
    .bind(hash(&token))
    .bind(TOKEN_TTL_DAYS)
    .fetch_one(pool)
    .await?;
    Ok((token, expires_at))
}

/// token 有效、未过期、账号仍启用时返回该账号。
pub async fn authenticate(pool: &PgPool, token: &str) -> Result<Option<AuthedAccount>, AppError> {
    let row: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT a.id, a.account, a.role
         FROM login_tokens t
         JOIN remote_accounts a ON a.id = t.account_id
         WHERE t.token_hash = $1 AND t.expires_at > now() AND a.active",
    )
    .bind(hash(token))
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id, account, role)| AuthedAccount {
        id,
        account,
        role: parse_role(&role),
    }))
}

pub async fn purge_expired(pool: &PgPool) -> Result<u64, AppError> {
    Ok(
        sqlx::query("DELETE FROM login_tokens WHERE expires_at <= now()")
            .execute(pool)
            .await?
            .rows_affected(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_prefixed_hex_and_unique() {
        let a = generate().unwrap();
        let b = generate().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), TOKEN_PREFIX.len() + 64);
        assert!(a.starts_with(TOKEN_PREFIX));
        assert!(a[TOKEN_PREFIX.len()..]
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_is_deterministic_and_not_the_token() {
        assert_eq!(hash("mbt_x"), hash("mbt_x"));
        assert_ne!(hash("mbt_x"), hash("mbt_y"));
        assert_eq!(hash("mbt_x").len(), 32);
        assert_ne!(hash("mbt_x"), b"mbt_x".to_vec());
    }
}
