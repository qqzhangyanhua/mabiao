use chrono::{DateTime, Utc};
use push_protocol::{ContextManifestPayload, EventPayload, SessionPayload};
use sqlx::types::Json;
use sqlx::{FromRow, PgConnection, PgPool, Row};

use crate::error::AppError;
use crate::usage_query::Filter;

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

/// 列表里的一场会话：只有目录元数据，不带事件正文与上下文清单。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionListRow {
    pub id: i64,
    pub account_id: i64,
    pub account: String,
    pub device_name: String,
    pub source: String,
    pub session_id: String,
    pub title: String,
    pub project_path: String,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub model: String,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub event_count: i32,
    pub generated_by_work_notes: bool,
    pub pushed_at: DateTime<Utc>,
}

/// 会话时间取结束时间；客户端时间解析不了时退回开始、再退回推送时间，免得会话从列表里消失。
const SESSION_TIME: &str = "COALESCE(s.ended_at, s.started_at, s.pushed_at)";

const SESSION_FILTER: &str = "($1::bigint IS NULL OR s.account_id = $1)
           AND ($2::timestamptz IS NULL OR COALESCE(s.ended_at, s.started_at, s.pushed_at) >= $2)
           AND ($3::timestamptz IS NULL OR COALESCE(s.ended_at, s.started_at, s.pushed_at) < $3)
           AND ($4::bigint IS NULL OR s.project_id = $4)
           AND ($5::boolean IS NULL OR s.generated_by_work_notes = $5)";

/// 会话列表的过滤条件：消耗记录那套范围，加上「码表生成」标记。
#[derive(Debug, Clone)]
pub struct SessionFilter {
    pub scope: Filter,
    /// `Some(true)` 只看码表生成的，`Some(false)` 排除它们，`None` 不过滤。
    pub generated_by_work_notes: Option<bool>,
}

/// 按时间从新到旧。`from` 含、`to` 不含，与消耗记录查询一致。
pub async fn list(
    pool: &PgPool,
    filter: &SessionFilter,
    limit: i64,
    offset: i64,
) -> Result<Vec<SessionListRow>, AppError> {
    let sql = format!(
        "SELECT s.id, s.account_id, a.account, d.device_name, s.source, s.session_id, s.title,
                s.project_path, s.project_id, p.name AS project_name, s.model,
                s.started_at, s.ended_at, s.event_count, s.generated_by_work_notes, s.pushed_at
         FROM sessions s
         JOIN remote_accounts a ON a.id = s.account_id
         JOIN devices d ON d.id = s.device_pk
         LEFT JOIN projects p ON p.id = s.project_id
         WHERE {SESSION_FILTER}
         ORDER BY {SESSION_TIME} DESC, s.id DESC
         LIMIT $6 OFFSET $7"
    );
    Ok(sqlx::query_as(&sql)
        .bind(filter.scope.account_id)
        .bind(filter.scope.from)
        .bind(filter.scope.to)
        .bind(filter.scope.project_id)
        .bind(filter.generated_by_work_notes)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?)
}

/// 过滤条件下的会话总数，不受分页影响。
pub async fn count(pool: &PgPool, filter: &SessionFilter) -> Result<i64, AppError> {
    let sql = format!(
        "SELECT count(*) FROM sessions s
         WHERE {SESSION_FILTER}"
    );
    Ok(sqlx::query_scalar(&sql)
        .bind(filter.scope.account_id)
        .bind(filter.scope.from)
        .bind(filter.scope.to)
        .bind(filter.scope.project_id)
        .bind(filter.generated_by_work_notes)
        .fetch_one(pool)
        .await?)
}

/// 详情页的一场会话：目录元数据加事件与上下文清单。
#[derive(Debug, Clone)]
pub struct SessionDetailRow {
    pub item: SessionListRow,
    pub device_pk: i64,
    pub source_files: Vec<String>,
    pub redaction_count: i32,
    pub events: Vec<EventPayload>,
    pub context_manifest: Option<ContextManifestPayload>,
}

/// 单场会话全文。鉴权由调用方先用 `owner_of` + `can_access` 做；这里只读。
pub async fn detail(pool: &PgPool, id: i64) -> Result<Option<SessionDetailRow>, AppError> {
    let row = sqlx::query(
        "SELECT s.id, s.account_id, a.account, d.device_name, s.source, s.session_id, s.title,
                s.project_path, s.project_id, p.name AS project_name, s.model,
                s.started_at, s.ended_at, s.event_count, s.generated_by_work_notes, s.pushed_at,
                s.device_pk, s.source_files, s.redaction_count, s.events, s.context_manifest
         FROM sessions s
         JOIN remote_accounts a ON a.id = s.account_id
         JOIN devices d ON d.id = s.device_pk
         LEFT JOIN projects p ON p.id = s.project_id
         WHERE s.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let source_files: Json<Vec<String>> = row.get("source_files");
    let events: Json<Vec<EventPayload>> = row.get("events");
    let manifest: Option<Json<ContextManifestPayload>> = row.get("context_manifest");
    Ok(Some(SessionDetailRow {
        item: SessionListRow::from_row(&row)?,
        device_pk: row.get("device_pk"),
        source_files: source_files.0,
        redaction_count: row.get("redaction_count"),
        events: events.0,
        context_manifest: manifest.map(|m| m.0),
    }))
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
