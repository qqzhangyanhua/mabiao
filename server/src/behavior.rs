//! 已推送会话的行为指标：工具分布和失败率。
//!
//! 时间口径与会话列表一致（结束时间，退回开始、再退回推送时间），不按消耗记录的发生时间。
//! 失败指摄取时记成 `kind = error` 且 `actor = tool` 的事件。这不是桌面端 Cursor 会话的失败率。

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::AppError;
use crate::scope::{self, Scope};

/// 工具分布最多返回这么多行。合计始终是全量。
pub const MAX_TOOLS: i64 = 30;

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
struct TotalsSql {
    tool_calls: i64,
    tool_failures: i64,
    other_errors: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct ToolStat {
    pub name: String,
    pub calls: i64,
    pub failures: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Behavior {
    pub tool_calls: i64,
    pub tool_failures: i64,
    pub other_errors: i64,
    /// 工具失败 ÷ 工具调用。没有调用时为空。
    pub failure_rate: Option<f64>,
    pub tools: Vec<ToolStat>,
    /// 调用最多的前 [`MAX_TOOLS`] 个之外还有工具。
    pub tools_truncated: bool,
}

pub async fn report(pool: &PgPool, filter: &Scope) -> Result<Behavior, AppError> {
    let session_where = scope::SESSION_WHERE;
    let totals_sql = format!(
        "SELECT
            count(*) FILTER (WHERE e->>'kind' = 'tool_call')::bigint AS tool_calls,
            count(*) FILTER (WHERE e->>'kind' = 'error' AND e->>'actor' = 'tool')::bigint
                AS tool_failures,
            count(*) FILTER (
                WHERE e->>'kind' = 'error' AND e->>'actor' IS DISTINCT FROM 'tool'
            )::bigint AS other_errors
         FROM sessions s
         CROSS JOIN LATERAL jsonb_array_elements(s.events) AS e
         WHERE {session_where}"
    );
    let totals: TotalsSql = filter.bind_query_as(&totals_sql).fetch_one(pool).await?;
    let tools_sql = format!(
        "SELECT COALESCE(NULLIF(e->>'name', ''), '未命名') AS name,
                count(*) FILTER (WHERE e->>'kind' = 'tool_call')::bigint AS calls,
                count(*) FILTER (WHERE e->>'kind' = 'error' AND e->>'actor' = 'tool')::bigint
                    AS failures
         FROM sessions s
         CROSS JOIN LATERAL jsonb_array_elements(s.events) AS e
         WHERE {session_where}
           AND (e->>'kind' = 'tool_call' OR (e->>'kind' = 'error' AND e->>'actor' = 'tool'))
         GROUP BY 1
         ORDER BY calls DESC, failures DESC, name ASC
         LIMIT {limit}",
        limit = MAX_TOOLS + 1
    );
    let mut tools: Vec<ToolStat> = filter.bind_query_as(&tools_sql).fetch_all(pool).await?;
    let tools_truncated = tools.len() as i64 > MAX_TOOLS;
    if tools_truncated {
        tools.truncate(MAX_TOOLS as usize);
    }
    let failure_rate =
        (totals.tool_calls > 0).then(|| totals.tool_failures as f64 / totals.tool_calls as f64);
    Ok(Behavior {
        tool_calls: totals.tool_calls,
        tool_failures: totals.tool_failures,
        other_errors: totals.other_errors,
        failure_rate,
        tools,
        tools_truncated,
    })
}
