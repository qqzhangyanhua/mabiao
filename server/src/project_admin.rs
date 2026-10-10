//! 项目的人工管理：列出、改名、合并（覆盖 git remote / 目录名的自动归并结果）。
//!
//! 合并不删项目行：被合并项目带 `merged_into` 留着，归并键继续指向目标，见 `projects::settle`。

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};

use crate::error::AppError;

pub const MAX_PROJECT_NAME_CHARS: usize = 200;
const MAX_LISTED_PROJECTS: i64 = 2000;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProjectRow {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub git_remote: Option<String>,
    pub merged_into: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProjectListRow {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub git_remote: Option<String>,
    pub created_at: DateTime<Utc>,
    pub session_count: i64,
    pub usage_record_count: i64,
    /// 已经合并进它的项目个数。
    pub merged_count: i64,
}

pub async fn get(pool: &PgPool, id: i64) -> Result<Option<ProjectRow>, AppError> {
    Ok(sqlx::query_as(
        "SELECT id, key, name, git_remote, merged_into, created_at FROM projects WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

/// 还有效（没被合并掉）的项目，附数据量，供管理员挑合并目标。
pub async fn list_live(pool: &PgPool) -> Result<Vec<ProjectListRow>, AppError> {
    Ok(sqlx::query_as(&format!(
        "SELECT p.id, p.key, p.name, p.git_remote, p.created_at,
                (SELECT count(*) FROM sessions s WHERE s.project_id = p.id)::bigint AS session_count,
                (SELECT count(*) FROM usage_records u WHERE u.project_id = p.id)::bigint
                    AS usage_record_count,
                (SELECT count(*) FROM projects m WHERE m.merged_into = p.id)::bigint AS merged_count
         FROM projects p
         WHERE p.merged_into IS NULL
         ORDER BY lower(p.name), p.id
         LIMIT {MAX_LISTED_PROJECTS}"
    ))
    .fetch_all(pool)
    .await?)
}

/// 这个账号在项目里有没有自己的会话或消耗记录。成员只能打开自己有数据的项目页，
/// 免得靠遍历 id 看到别人在做的仓库名与 remote。
pub async fn has_data_of(
    pool: &PgPool,
    project_id: i64,
    account_id: i64,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sessions WHERE project_id = $1 AND account_id = $2)
             OR EXISTS (SELECT 1 FROM usage_records WHERE project_id = $1 AND account_id = $2)",
    )
    .bind(project_id)
    .bind(account_id)
    .fetch_one(pool)
    .await?)
}

pub fn normalize_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::invalid("项目名不能为空"));
    }
    if name.chars().count() > MAX_PROJECT_NAME_CHARS {
        return Err(AppError::invalid(format!(
            "项目名最多 {MAX_PROJECT_NAME_CHARS} 个字符"
        )));
    }
    Ok(name.to_owned())
}

/// 改名。新推送不会改回自动名字（`projects::find_or_create` 冲突时不碰 name）。
pub async fn rename(pool: &PgPool, id: i64, name: &str) -> Result<ProjectRow, AppError> {
    let current = get(pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;
    if current.merged_into.is_some() {
        return Err(AppError::conflict("项目已被合并，请改目标项目的名字"));
    }
    Ok(sqlx::query_as(
        "UPDATE projects SET name = $2 WHERE id = $1
         RETURNING id, key, name, git_remote, merged_into, created_at",
    )
    .bind(id)
    .bind(name)
    .fetch_one(pool)
    .await?)
}

#[derive(Debug, Clone)]
pub struct MergeOutcome {
    /// 合并后的目标项目，与合并在同一事务里读出。
    pub target: ProjectRow,
    pub sessions_moved: u64,
    pub usage_records_moved: u64,
}

/// 把 `source` 并进 `target`：会话、消耗记录、路径映射全部改挂目标，`source` 变成目标的别名。
///
/// 两行都按 id 顺序加行锁。推送解析项目时（`projects::settle`）对项目行加共享锁，
/// 所以合并要等在途推送提交，之后的推送一定看到 `merged_into`，不会再往被合并项目里写。
pub async fn merge(
    conn: &mut PgConnection,
    source: i64,
    target: i64,
) -> Result<MergeOutcome, AppError> {
    if source == target {
        return Err(AppError::invalid("不能把项目合并进它自己"));
    }
    let locked: Vec<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT id, merged_into FROM projects WHERE id = ANY($1) ORDER BY id FOR UPDATE",
    )
    .bind(vec![source, target])
    .fetch_all(&mut *conn)
    .await?;
    for id in [source, target] {
        let (_, merged_into) = locked
            .iter()
            .find(|(row_id, _)| *row_id == id)
            .ok_or_else(|| AppError::not_found(format!("项目 {id} 不存在")))?;
        if merged_into.is_some() {
            return Err(AppError::conflict(format!("项目 {id} 已被合并过")));
        }
    }

    let sessions_moved = repoint(conn, "sessions", source, target).await?;
    let usage_records_moved = repoint(conn, "usage_records", source, target).await?;
    repoint(conn, "project_paths", source, target).await?;
    sqlx::query("UPDATE projects SET merged_into = $2 WHERE id = $1 OR merged_into = $1")
        .bind(source)
        .bind(target)
        .execute(&mut *conn)
        .await?;
    let target = sqlx::query_as(
        "SELECT id, key, name, git_remote, merged_into, created_at FROM projects WHERE id = $1",
    )
    .bind(target)
    .fetch_one(&mut *conn)
    .await?;
    Ok(MergeOutcome {
        target,
        sessions_moved,
        usage_records_moved,
    })
}

async fn repoint(
    conn: &mut PgConnection,
    table: &str,
    source: i64,
    target: i64,
) -> Result<u64, AppError> {
    // `table` 只来自上面三个字面量。
    Ok(sqlx::query(&format!(
        "UPDATE {table} SET project_id = $2 WHERE project_id = $1"
    ))
    .bind(source)
    .bind(target)
    .execute(&mut *conn)
    .await?
    .rows_affected())
}
