//! 账号隔离、项目访问校验与查询范围 SQL 条件。
//!
//! 所有按账号 / 项目 / 时间查询数据的接口都从这里拿到已校验的 [`Scope`]，
//! 保证成员隔离（403）和项目存在 / 未合并 / 可见校验（404）口径唯一，
//! SQL 条件与 `$1..=$4` 绑定顺序只写一份。

use chrono::{DateTime, Utc};
use sqlx::postgres::{PgArguments, PgRow};
use sqlx::query::{QueryAs, QueryScalar};
use sqlx::{FromRow, PgPool, Postgres};

use crate::auth::AuthedAccount;
use crate::error::AppError;
use crate::project_admin::{self, ProjectRow};
use crate::sessions::parse_timestamp;

/// `usage_records u` 的范围条件，参数位固定为 `$1 = account_id, $2 = from, $3 = to, $4 = project_id`。
pub const USAGE_WHERE: &str = "($1::bigint IS NULL OR u.account_id = $1)
    AND ($2::timestamptz IS NULL OR u.occurred_at >= $2)
    AND ($3::timestamptz IS NULL OR u.occurred_at < $3)
    AND ($4::bigint IS NULL OR u.project_id = $4)";

/// 会话落点时间：优先结束时间，解析不了退回开始、再退回推送时间，免得会话从列表与聚合里消失。
pub const SESSION_TIME: &str = "COALESCE(s.ended_at, s.started_at, s.pushed_at)";

/// 会话开始时间（时间线重叠判定用）：优先开始时间，缺则退回结束、再退回推送时间。
pub const SESSION_START_TIME: &str = "COALESCE(s.started_at, s.ended_at, s.pushed_at)";

/// `sessions s` 的落点范围条件，参数位固定为 `$1 = account_id, $2 = from, $3 = to, $4 = project_id`。
///
/// 这里**不过滤** `generated_by_work_notes`：按 `CONTEXT.md`，码表生成的会话仍计入聚合与行为指标，
/// 只有会话列表在调用方显式传参时才附加过滤。
pub const SESSION_WHERE: &str = "($1::bigint IS NULL OR s.account_id = $1)
    AND ($2::timestamptz IS NULL OR COALESCE(s.ended_at, s.started_at, s.pushed_at) >= $2)
    AND ($3::timestamptz IS NULL OR COALESCE(s.ended_at, s.started_at, s.pushed_at) < $3)
    AND ($4::bigint IS NULL OR s.project_id = $4)";

/// `sessions s` 的重叠区间条件（工作时间线用）：结束 ≥ from 且 开始 < to。
/// 参数位固定为 `$1 = account_id, $2 = from, $3 = to, $4 = project_id`。
pub const SESSION_OVERLAP_WHERE: &str = "($1::bigint IS NULL OR s.account_id = $1)
    AND ($2::timestamptz IS NULL OR COALESCE(s.ended_at, s.started_at, s.pushed_at) >= $2)
    AND ($3::timestamptz IS NULL OR COALESCE(s.started_at, s.ended_at, s.pushed_at) < $3)
    AND ($4::bigint IS NULL OR s.project_id = $4)";

/// HTTP 查询串里尚未校验的范围参数。
#[derive(Debug, Clone, Copy, Default)]
pub struct ScopeInput<'a> {
    pub account_id: Option<i64>,
    pub project_id: Option<i64>,
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
}

/// 已通过鉴权与项目可见性校验的查询范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub account_id: Option<i64>,
    /// 含。
    pub from: Option<DateTime<Utc>>,
    /// 不含。
    pub to: Option<DateTime<Utc>>,
    /// 已校验存在、未合并且调用方有权查看的项目 id。
    pub project_id: Option<i64>,
}

/// 成员只看自己的；管理员默认看全体，可用 `account_id` 收窄。
/// 成员指定别人的 `account_id` 得到 403，与其它按账号归属的接口走同一个 `can_access`。
pub fn scope_account(
    caller: &AuthedAccount,
    requested: Option<i64>,
) -> Result<Option<i64>, AppError> {
    match requested {
        Some(id) if !caller.can_access(id) => Err(AppError::forbidden("只能查看自己的数据")),
        Some(id) => Ok(Some(id)),
        None if caller.is_admin() => Ok(None),
        None => Ok(Some(caller.id)),
    }
}

pub fn parse_bound(name: &str, value: Option<&str>) -> Result<Option<DateTime<Utc>>, AppError> {
    value
        .map(|text| {
            parse_timestamp(text)
                .ok_or_else(|| AppError::invalid(format!("{name} 不是 RFC 3339 时间")))
        })
        .transpose()
}

async fn verify_project(
    pool: &PgPool,
    caller: &AuthedAccount,
    project_id: i64,
) -> Result<ProjectRow, AppError> {
    let project = project_admin::get(pool, project_id)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;
    if project.merged_into.is_some() {
        return Err(AppError::not_found("项目已被合并到别的项目"));
    }
    // 项目名与 remote 是团队共享的，但成员只该看到自己做过的项目。
    if !caller.is_admin() && !project_admin::has_data_of(pool, project_id, caller.id).await? {
        return Err(AppError::not_found("项目不存在"));
    }
    Ok(project)
}

impl Scope {
    /// 校验账号隔离（403）、时间边界（400）以及可选 `project_id`（404）。
    pub async fn resolve(
        pool: &PgPool,
        caller: &AuthedAccount,
        input: ScopeInput<'_>,
    ) -> Result<Self, AppError> {
        let (scope, _) = Self::resolve_inner(pool, caller, input).await?;
        Ok(scope)
    }

    /// 与 [`Self::resolve`] 同一套校验，并直接返回查出的项目行，供项目详情页复用，避免查两次。
    pub async fn resolve_project(
        pool: &PgPool,
        caller: &AuthedAccount,
        project_id: i64,
        account_id: Option<i64>,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<(Self, ProjectRow), AppError> {
        let (scope, project) = Self::resolve_inner(
            pool,
            caller,
            ScopeInput {
                account_id,
                project_id: Some(project_id),
                from,
                to,
            },
        )
        .await?;
        Ok((scope, project.expect("project_id is Some")))
    }

    async fn resolve_inner(
        pool: &PgPool,
        caller: &AuthedAccount,
        input: ScopeInput<'_>,
    ) -> Result<(Self, Option<ProjectRow>), AppError> {
        let account_id = scope_account(caller, input.account_id)?;
        let from = parse_bound("from", input.from)?;
        let to = parse_bound("to", input.to)?;
        let project = match input.project_id {
            Some(id) => Some(verify_project(pool, caller, id).await?),
            None => None,
        };
        Ok((
            Self {
                account_id,
                from,
                to,
                project_id: input.project_id,
            },
            project,
        ))
    }

    /// 按 `$1..=$4` 的固定顺序绑定 `account_id`、`from`、`to`、`project_id`。
    pub fn bind_query_as<'q, O>(&self, sql: &'q str) -> QueryAs<'q, Postgres, O, PgArguments>
    where
        O: for<'r> FromRow<'r, PgRow>,
    {
        sqlx::query_as(sql)
            .bind(self.account_id)
            .bind(self.from)
            .bind(self.to)
            .bind(self.project_id)
    }

    /// 按 `$1..=$4` 的固定顺序绑定标量查询参数。
    pub fn bind_query_scalar<'q, O>(
        &self,
        sql: &'q str,
    ) -> QueryScalar<'q, Postgres, O, PgArguments>
    where
        (O,): for<'r> FromRow<'r, PgRow>,
    {
        sqlx::query_scalar(sql)
            .bind(self.account_id)
            .bind(self.from)
            .bind(self.to)
            .bind(self.project_id)
    }
}

#[cfg(test)]
mod tests {
    use push_protocol::RemoteRole;

    use super::*;

    fn account(id: i64, role: RemoteRole) -> AuthedAccount {
        AuthedAccount {
            id,
            account: format!("u{id}"),
            role,
        }
    }

    #[test]
    fn member_scope_defaults_to_self_and_forbids_other_accounts() {
        let alice = account(2, RemoteRole::Member);
        assert_eq!(scope_account(&alice, None).unwrap(), Some(2));
        assert_eq!(scope_account(&alice, Some(2)).unwrap(), Some(2));
        assert!(scope_account(&alice, Some(3)).is_err());
    }

    #[test]
    fn admin_scope_defaults_to_all_and_can_narrow_to_any_account() {
        let admin = account(1, RemoteRole::Admin);
        assert_eq!(scope_account(&admin, None).unwrap(), None);
        assert_eq!(scope_account(&admin, Some(2)).unwrap(), Some(2));
    }

    #[test]
    fn parse_bound_accepts_rfc3339_and_rejects_non_rfc3339() {
        assert_eq!(parse_bound("from", None).unwrap(), None);
        let parsed = parse_bound("from", Some("2026-03-01T08:00:00+08:00"))
            .unwrap()
            .unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-03-01T00:00:00+00:00");
        assert!(parse_bound("to", Some("yesterday")).is_err());
    }
}
