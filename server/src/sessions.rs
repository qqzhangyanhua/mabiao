use chrono::{DateTime, Utc};
use push_protocol::SessionPayload;
use sqlx::types::Json;
use sqlx::{PgConnection, PgPool, Row};

use crate::error::AppError;

/// 客户端时间都是 RFC 3339 字符串；解析不了就当没有，不因此拒收整场会话。
pub fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// 按（账号, 设备, 来源, session_id）整场覆盖。返回 `(行 id, 是否覆盖了已有的)`。
///
/// 事件、清单整份替换：会话后来又长了，再推一次就是新的全貌，不做增量合并。
/// `first_pushed_at` 保留首次入库时间，`pushed_at` 取这一次。
pub async fn upsert(
    conn: &mut PgConnection,
    account_id: i64,
    device_pk: i64,
    session: &SessionPayload,
    project_id: Option<i64>,
) -> Result<(i64, bool), AppError> {
    let row = sqlx::query(
        "INSERT INTO sessions (
             account_id, device_pk, source, session_id, title, project_path, project_id, model,
             started_at, ended_at, source_files, generated_by_work_notes, redaction_count,
             event_count, events, context_manifest)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
         ON CONFLICT (account_id, device_pk, source, session_id) DO UPDATE SET
             title = EXCLUDED.title,
             project_path = EXCLUDED.project_path,
             project_id = EXCLUDED.project_id,
             model = EXCLUDED.model,
             started_at = EXCLUDED.started_at,
             ended_at = EXCLUDED.ended_at,
             source_files = EXCLUDED.source_files,
             generated_by_work_notes = EXCLUDED.generated_by_work_notes,
             redaction_count = EXCLUDED.redaction_count,
             event_count = EXCLUDED.event_count,
             events = EXCLUDED.events,
             context_manifest = EXCLUDED.context_manifest,
             pushed_at = now()
         RETURNING id, (xmax = 0) AS inserted",
    )
    .bind(account_id)
    .bind(device_pk)
    .bind(&session.source)
    .bind(&session.session_id)
    .bind(&session.title)
    .bind(&session.project)
    .bind(project_id)
    .bind(&session.model)
    .bind(parse_timestamp(&session.started_at))
    .bind(parse_timestamp(&session.ended_at))
    .bind(Json(&session.source_files))
    .bind(session.generated_by_work_notes)
    .bind(i32::try_from(session.redaction_count).unwrap_or(i32::MAX))
    .bind(i32::try_from(session.events.len()).unwrap_or(i32::MAX))
    .bind(Json(&session.events))
    .bind(session.context_manifest.as_ref().map(Json))
    .fetch_one(&mut *conn)
    .await?;
    let inserted: bool = row.get("inserted");
    Ok((row.get("id"), !inserted))
}

/// 会话所属账号。会话不存在时为 `None`。
pub async fn owner_of(pool: &PgPool, id: i64) -> Result<Option<i64>, AppError> {
    Ok(
        sqlx::query_scalar("SELECT account_id FROM sessions WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

/// 手动删除，不波及消耗记录与项目。首版没有任何自动删除。
pub async fn delete(pool: &PgPool, id: i64) -> Result<bool, AppError> {
    Ok(sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected()
        > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_with_any_offset_is_converted_to_utc() {
        let t = parse_timestamp("2026-01-01T08:00:00+08:00").unwrap();
        assert_eq!(t.to_rfc3339(), "2026-01-01T00:00:00+00:00");
        assert!(parse_timestamp("2026-01-01T00:00:00Z").is_some());
    }

    #[test]
    fn non_rfc3339_is_none() {
        for bad in ["", "yesterday", "2026-01-01", "2026-01-01 00:00:00"] {
            assert_eq!(parse_timestamp(bad), None, "{bad:?}");
        }
    }
}
