use std::collections::BTreeMap;

use rusqlite::{params, params_from_iter, types::Value, Connection};

use crate::domain::{Filter, PriceTable};
use crate::rollup_source::dimension_clauses;

pub(crate) const COST_EXPR: &str = "
    CASE
        WHEN r.native_cost IS NOT NULL THEN r.native_cost
        WHEN COALESCE(pe.input, pf.input) IS NOT NULL THEN
            COALESCE(pe.input, pf.input) * r.input_tokens
            + COALESCE(pe.output, pf.output) * r.output_tokens
            + COALESCE(pe.cache_read, pf.cache_read) * r.cache_read_tokens
            + COALESCE(pe.cache_creation, pf.cache_creation) * r.cache_creation_tokens
        ELSE NULL
    END";

/// 未定价标志（每行 0/1）。
pub(crate) const UNPRICED_EXPR: &str = "
    CASE
        WHEN r.native_cost IS NOT NULL THEN 0
        WHEN COALESCE(pe.input, pf.input) IS NOT NULL THEN 0
        ELSE 1
    END";

/// 费用来源：native > 精确匹配条目 origin > 兜底条目 origin > none。
pub(crate) const COST_SOURCE_EXPR: &str = "
    CASE
        WHEN r.native_cost IS NOT NULL THEN 'native'
        WHEN pe.model IS NOT NULL THEN COALESCE(pe.origin, 'user')
        WHEN pf.model IS NOT NULL THEN COALESCE(pf.origin, 'user')
        ELSE 'none'
    END";

/// 价格表两次 LEFT JOIN：pe 匹配 model+provider，pf 兜底 model 且 provider 为空。
/// 键在 `install_prices` 里已折成 ASCII 小写，与 `cost::model_matches` 一致。
///
/// `r.model` 不套 `lower()`：`store::insert_records` 写入时已归一化，`migrate_lowercase_model`
/// 也补齐了历史数据，两边同口径。对全表逐行调函数会让 17 万行各多付两次调用。
/// `r.provider` 仍要 `lower()`——历史值里有 `cpaApi` 这类混合大小写，归一化会改到界面显示。
pub(crate) const PRICE_JOINS: &str = "
    LEFT JOIN price_rows pe ON pe.model = r.model AND pe.provider = lower(r.provider)
    LEFT JOIN price_rows pf ON pf.model = r.model AND pf.provider IS NULL";

/// 把价目表装进临时表 `price_rows`，供 `PRICE_JOINS` 取价。
///
/// 每次查询重建一遍看着浪费，实测却只要 1.7ms——SQLite 在单个事务里插 1400 行就是这么快。
/// 曾经按指纹跳过重建，收益不到首屏的 1%，不值当缓存失效那份复杂度，已经撤掉。
/// 真正的开销在 `PRICE_JOINS` 逐行取价那边，那个换不掉：先按 (model, provider) 聚合再算价
/// 数学上等价，但 GROUP BY 比 JOIN 更贵，实测反而慢。
pub(crate) fn install_prices(conn: &Connection, prices: &PriceTable) -> Result<(), String> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS price_rows;
         CREATE TEMP TABLE price_rows (
             model TEXT NOT NULL,
             provider TEXT,
             input REAL NOT NULL DEFAULT 0,
             output REAL NOT NULL DEFAULT 0,
             cache_read REAL NOT NULL DEFAULT 0,
             cache_creation REAL NOT NULL DEFAULT 0,
             origin TEXT NOT NULL DEFAULT 'user'
         );
         CREATE INDEX price_rows_model_provider ON price_rows(model, provider);",
    )
    .map_err(|e| e.to_string())?;
    if prices.prices.is_empty() {
        return Ok(());
    }
    let mut stmt = conn
        .prepare(
            "INSERT INTO price_rows (model, provider, input, output, cache_read, cache_creation, origin)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .map_err(|e| e.to_string())?;
    for entry in &prices.prices {
        stmt.execute(params![
            entry.model.to_ascii_lowercase(),
            entry
                .provider
                .as_ref()
                .map(|value| value.to_ascii_lowercase()),
            entry.input,
            entry.output,
            entry.cache_read,
            entry.cache_creation,
            entry.origin.as_str(),
        ])
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Filter → (WHERE 子句片段列表, 参数)。时间条件加在明细表 `r.` 上，维度过滤与工厂共用。
pub(crate) fn filter_clauses(filter: &Filter) -> (Vec<String>, Vec<Value>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<Value> = Vec::new();
    if let Some(from) = &filter.from {
        clauses.push("r.occurred_at >= ?".to_string());
        params.push(Value::Text(from.clone()));
    }
    if let Some(to) = &filter.to {
        clauses.push("r.occurred_at <= ?".to_string());
        params.push(Value::Text(to.clone()));
    }
    let (dim_clauses, dim_params) = dimension_clauses(filter, "r");
    clauses.extend(dim_clauses);
    params.extend(dim_params);
    (clauses, params)
}

pub(crate) struct SessionUsageTotals {
    pub total_tokens: i64,
    pub cost: Option<f64>,
    pub unpriced: bool,
}

/// 按精确 `(source, session_id)` 聚合消耗记录。对话目录挂用量，不改变会话管理口径。
pub(crate) fn usage_rollups_for_sessions(
    conn: &Connection,
    prices: &PriceTable,
    keys: &[(String, String)],
) -> Result<BTreeMap<(String, String), SessionUsageTotals>, String> {
    let mut totals = BTreeMap::new();
    if keys.is_empty() {
        return Ok(totals);
    }
    install_prices(conn, prices)?;
    let mut clauses = Vec::with_capacity(keys.len());
    let mut params: Vec<Value> = Vec::with_capacity(keys.len() * 2);
    for (source, session_id) in keys {
        clauses.push("(r.source = ? AND r.session_id = ?)".to_string());
        params.push(Value::Text(source.clone()));
        params.push(Value::Text(session_id.clone()));
    }
    let sql = format!(
        "SELECT r.source, r.session_id,
            COALESCE(SUM(r.total_tokens), 0),
            SUM({COST_EXPR}),
            COALESCE(SUM({UNPRICED_EXPR}), 0)
         FROM usage_records r
         {PRICE_JOINS}
         WHERE {}
         GROUP BY r.source, r.session_id",
        clauses.join(" OR "),
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(params.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<f64>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for (source, session_id, total_tokens, cost, unpriced_count) in rows {
        totals.insert(
            (source, session_id),
            SessionUsageTotals {
                total_tokens,
                cost,
                unpriced: unpriced_count > 0,
            },
        );
    }
    Ok(totals)
}

/// 转义 LIKE 通配符，避免用户输入的 `%`/`_` 被解释为通配符。
pub(crate) fn escape_like(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub(crate) fn where_sql(clauses: &[String]) -> String {
    if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    }
}

pub(crate) fn ratio(numerator: i64, denominator: i64) -> Option<f64> {
    if denominator <= 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}

/// 预聚合表上的费用表达式。与 `COST_EXPR` 逐行版语义一一对应：
/// `has_native` 进了主键，所以一组行要么全带 native_cost（直接取和），
/// 要么全不带（按 token 计价），两类不会混在同一行里。
///
/// 与逐行版有一处无法消除的差别：这里是「先把 token 加起来再乘单价」，逐行版是
/// 「每行乘完单价再相加」。数学上等价，浮点下不是——实测两者的费用差在 1e-14 量级
/// （token 数与占比完全一致）。金额以美元计，这个差比一分钱的十亿分之一还小，
/// 界面上取不到；但比对两条路径的测试得用容差，不能要求 f64 逐位相等。
pub(crate) const ROLLUP_COST_EXPR: &str = "
    CASE
        WHEN d.has_native = 1 THEN d.native_cost
        WHEN COALESCE(pe.input, pf.input) IS NOT NULL THEN
            COALESCE(pe.input, pf.input) * d.input_tokens
            + COALESCE(pe.output, pf.output) * d.output_tokens
            + COALESCE(pe.cache_read, pf.cache_read) * d.cache_read_tokens
            + COALESCE(pe.cache_creation, pf.cache_creation) * d.cache_creation_tokens
        ELSE NULL
    END";

/// 未定价计数。逐行版每行贡献 0/1，聚合版一组贡献 `record_count`，求和结果一致。
pub(crate) const ROLLUP_UNPRICED_EXPR: &str = "
    CASE
        WHEN d.has_native = 1 THEN 0
        WHEN COALESCE(pe.input, pf.input) IS NOT NULL THEN 0
        ELSE d.record_count
    END";

pub(crate) const ROLLUP_PRICE_JOINS: &str = "
    LEFT JOIN price_rows pe ON pe.model = d.model AND pe.provider = lower(d.provider)
    LEFT JOIN price_rows pf ON pf.model = d.model AND pf.provider IS NULL";

/// 预聚合表上的费用来源，与逐行 `COST_SOURCE_EXPR` 同口径。
pub(crate) const ROLLUP_COST_SOURCE_EXPR: &str = "
    CASE
        WHEN d.has_native = 1 THEN 'native'
        WHEN pe.model IS NOT NULL THEN COALESCE(pe.origin, 'user')
        WHEN pf.model IS NOT NULL THEN COALESCE(pf.origin, 'user')
        ELSE 'none'
    END";

/// 用户单价 / 快照按口径拆出的费用。native 整笔不进这四档。
pub(crate) const ROLLUP_COST_INPUT_EXPR: &str = "
    CASE
        WHEN d.has_native = 1 THEN NULL
        WHEN COALESCE(pe.input, pf.input) IS NOT NULL THEN
            COALESCE(pe.input, pf.input) * d.input_tokens
        ELSE NULL
    END";

pub(crate) const ROLLUP_COST_OUTPUT_EXPR: &str = "
    CASE
        WHEN d.has_native = 1 THEN NULL
        WHEN COALESCE(pe.input, pf.input) IS NOT NULL THEN
            COALESCE(pe.output, pf.output) * d.output_tokens
        ELSE NULL
    END";

pub(crate) const ROLLUP_COST_CACHE_READ_EXPR: &str = "
    CASE
        WHEN d.has_native = 1 THEN NULL
        WHEN COALESCE(pe.input, pf.input) IS NOT NULL THEN
            COALESCE(pe.cache_read, pf.cache_read) * d.cache_read_tokens
        ELSE NULL
    END";

pub(crate) const ROLLUP_COST_CACHE_CREATION_EXPR: &str = "
    CASE
        WHEN d.has_native = 1 THEN NULL
        WHEN COALESCE(pe.input, pf.input) IS NOT NULL THEN
            COALESCE(pe.cache_creation, pf.cache_creation) * d.cache_creation_tokens
        ELSE NULL
    END";

pub(crate) fn bucket_expr(grain: &str) -> &'static str {
    match grain {
        "hour" => "substr(d.first_at, 1, 13)",
        "week" => "strftime('%G-W%V', d.day)",
        "month" => "substr(d.day, 1, 7)",
        _ => "d.day",
    }
}

pub(crate) fn latest_nonempty_key_sql(column: &str) -> String {
    format!("MAX(CASE WHEN r.{column} != '' THEN r.occurred_at || char(31) || r.{column} END)")
}

pub(crate) fn unwrap_latest_key_sql(alias: &str) -> String {
    format!("COALESCE(substr({alias}, instr({alias}, char(31)) + 1), '')")
}

pub(crate) fn session_rollup_sql(inner_sql: &str, include_cost: bool, limit_top: bool) -> String {
    let project = unwrap_latest_key_sql("project_key");
    let model = unwrap_latest_key_sql("model_key");
    let source_file = unwrap_latest_key_sql("file_key");
    let (cost_select, joins) = if include_cost {
        (
            format!(
                "SUM({ROLLUP_COST_EXPR}) AS cost, COALESCE(SUM({ROLLUP_UNPRICED_EXPR}), 0) AS unpriced_count"
            ),
            ROLLUP_PRICE_JOINS,
        )
    } else {
        (
            "CAST(NULL AS REAL) AS cost, 0 AS unpriced_count".to_string(),
            "",
        )
    };
    let order_limit = if limit_top {
        "ORDER BY total_tokens DESC, source ASC, session_id ASC\n            LIMIT ?"
    } else {
        ""
    };
    format!(
        "SELECT source, session_id, total_tokens, started_at, ended_at,
            {project} AS project,
            {model} AS model,
            {source_file} AS source_file,
            cost, unpriced_count
         FROM (
            SELECT d.source AS source, d.session_id AS session_id,
                SUM(d.total_tokens) AS total_tokens,
                MIN(d.first_at) AS started_at,
                MAX(d.last_at) AS ended_at,
                MAX(CASE WHEN d.project != '' THEN d.last_at || char(31) || d.project END) AS project_key,
                MAX(CASE WHEN d.model != '' THEN d.last_at || char(31) || d.model END) AS model_key,
                MAX(d.file_key) AS file_key,
                {cost_select}
            FROM ({inner_sql}) d
            {joins}
            GROUP BY d.source, d.session_id
            {order_limit}
         )"
    )
}

pub(crate) fn distinct_values(
    conn: &Connection,
    column: &str,
    skip_empty: bool,
) -> Result<Vec<String>, String> {
    let seed_where = if skip_empty {
        format!(" WHERE {column} != ''")
    } else {
        String::new()
    };
    let step_and = if skip_empty {
        format!(" AND {column} != ''")
    } else {
        String::new()
    };
    let sql = format!(
        "WITH RECURSIVE distinct_scan(value) AS (
            SELECT MIN({column}) FROM usage_records{seed_where}
            UNION ALL
            SELECT (
                SELECT MIN({column}) FROM usage_records
                WHERE {column} > distinct_scan.value{step_and}
            )
            FROM distinct_scan WHERE distinct_scan.value IS NOT NULL
         )
         SELECT value FROM distinct_scan WHERE value IS NOT NULL"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
