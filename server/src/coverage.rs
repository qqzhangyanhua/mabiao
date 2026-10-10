use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;

use crate::error::AppError;

/// 一个成员的推送覆盖情况，让管理员一眼看出缺口。
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct MemberCoverage {
    pub account_id: i64,
    pub account: String,
    pub active: bool,
    pub device_count: i64,
    /// 任一设备最后一次推送的时间；从没推过为空。
    pub last_push_at: Option<DateTime<Utc>>,
    /// 已入库数据覆盖到的最后一天（UTC）：会话结束时间与消耗记录发生时间的最大值。
    /// 现算而不是单存一列，所以删除会话后不会留下虚高的覆盖日期。
    pub covered_through: Option<NaiveDate>,
}

pub async fn list(pool: &PgPool) -> Result<Vec<MemberCoverage>, AppError> {
    Ok(sqlx::query_as(
        "SELECT a.id AS account_id, a.account, a.active,
                (SELECT count(*) FROM devices d WHERE d.account_id = a.id) AS device_count,
                (SELECT max(d.last_seen_at) FROM devices d WHERE d.account_id = a.id) AS last_push_at,
                (GREATEST(
                    (SELECT max(s.ended_at) FROM sessions s WHERE s.account_id = a.id),
                    (SELECT max(u.occurred_at) FROM usage_records u WHERE u.account_id = a.id)
                 ) AT TIME ZONE 'UTC')::date AS covered_through
         FROM remote_accounts a
         ORDER BY a.id",
    )
    .fetch_all(pool)
    .await?)
}
