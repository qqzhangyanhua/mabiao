use chrono::{DateTime, Utc};
use push_protocol::DeviceInfo;
use sqlx::PgPool;

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct DeviceRow {
    pub device_id: String,
    pub device_name: String,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

/// 每次推送带着设备信息过来时调用：首次出现就登记，之后刷新设备名与最后出现时间。
pub async fn upsert(pool: &PgPool, account_id: i64, device: &DeviceInfo) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO devices (account_id, device_id, device_name)
         VALUES ($1, $2, $3)
         ON CONFLICT (account_id, device_id)
         DO UPDATE SET device_name = EXCLUDED.device_name, last_seen_at = now()",
    )
    .bind(account_id)
    .bind(&device.device_id)
    .bind(&device.device_name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list(pool: &PgPool, account_id: i64) -> Result<Vec<DeviceRow>, AppError> {
    Ok(sqlx::query_as(
        "SELECT device_id, device_name, first_seen_at, last_seen_at
         FROM devices WHERE account_id = $1 ORDER BY first_seen_at, id",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?)
}
