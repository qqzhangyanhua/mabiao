//! 统一子查询工厂：把切分形态变成列结构与 `usage_rollup` 一致的子查询。
//!
//! 调用方 `FROM ({sql}) d`。本模块不接任何查询函数。
//!
//! 每个 SELECT 分支先绑时间条件、再绑维度（source / model / project / provider）。
//! 切分形态先整天预聚合，再把两端 partial 以 OR 并进同一条明细 SELECT。

use rusqlite::types::Value;

use crate::domain::Filter;
use crate::rollup_split::{PartialRange, RollupPlan, RollupSplit};

/// 列结构与 `usage_rollup` 一致的子查询及其绑定参数。
#[derive(Debug, Clone, PartialEq)]
pub struct RollupSource {
    pub sql: String,
    pub params: Vec<Value>,
}

/// 明细一行一记录的重命名投影，列顺序与 `usage_rollup` 建表一致。
const RAW_PROJECTION: &str = "
    substr(r.occurred_at, 1, 10) AS day,
    r.source AS source,
    r.model AS model,
    r.provider AS provider,
    r.project AS project,
    r.session_id AS session_id,
    CASE WHEN r.native_cost IS NOT NULL THEN 1 ELSE 0 END AS has_native,
    r.input_tokens AS input_tokens,
    r.output_tokens AS output_tokens,
    r.cache_read_tokens AS cache_read_tokens,
    r.cache_creation_tokens AS cache_creation_tokens,
    r.reasoning_tokens AS reasoning_tokens,
    r.total_tokens AS total_tokens,
    COALESCE(r.native_cost, 0) AS native_cost,
    1 AS record_count,
    r.occurred_at AS first_at,
    r.occurred_at AS last_at,
    CASE WHEN r.source_file != '' THEN r.occurred_at || char(31) || r.source_file END AS file_key";

/// 按切分判定产出的形态拼出子查询。
pub fn rollup_source(plan: &RollupPlan, filter: &Filter) -> RollupSource {
    match plan {
        RollupPlan::Raw => raw_source(filter),
        RollupPlan::Rollup => rollup_only_source(filter),
        RollupPlan::Split(split) => split_source(split, filter),
    }
}

fn raw_source(filter: &Filter) -> RollupSource {
    let mut clauses = Vec::new();
    let mut params = Vec::new();
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
    RollupSource {
        sql: select_sql(RAW_PROJECTION, "usage_records r", &clauses),
        params,
    }
}

fn rollup_only_source(filter: &Filter) -> RollupSource {
    let (clauses, params) = dimension_clauses(filter, "d");
    RollupSource {
        sql: select_sql("*", "usage_rollup d", &clauses),
        params,
    }
}

fn split_source(split: &RollupSplit, filter: &Filter) -> RollupSource {
    let middle = complete_days_source(split, filter);
    match partial_raw_source(split, filter) {
        Some(raw) => union_all(middle, raw),
        None => middle,
    }
}

fn complete_days_source(split: &RollupSplit, filter: &Filter) -> RollupSource {
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    if let Some(from) = &split.complete_from {
        clauses.push("d.day >= ?".to_string());
        params.push(Value::Text(from.clone()));
    }
    if let Some(to) = &split.complete_to {
        clauses.push("d.day < ?".to_string());
        params.push(Value::Text(to.clone()));
    }
    let (dim_clauses, dim_params) = dimension_clauses(filter, "d");
    clauses.extend(dim_clauses);
    params.extend(dim_params);
    RollupSource {
        sql: select_sql("*", "usage_rollup d", &clauses),
        params,
    }
}

fn partial_raw_source(split: &RollupSplit, filter: &Filter) -> Option<RollupSource> {
    let mut bounds = Vec::new();
    let mut params = Vec::new();
    if let Some(head) = &split.head {
        push_partial_bound(&mut bounds, &mut params, head, "<");
    }
    if let Some(tail) = &split.tail {
        push_partial_bound(&mut bounds, &mut params, tail, "<=");
    }
    if bounds.is_empty() {
        return None;
    }
    let mut clauses = vec![format!("({})", bounds.join(" OR "))];
    let (dim_clauses, dim_params) = dimension_clauses(filter, "r");
    clauses.extend(dim_clauses);
    params.extend(dim_params);
    Some(RollupSource {
        sql: select_sql(RAW_PROJECTION, "usage_records r", &clauses),
        params,
    })
}

fn push_partial_bound(
    bounds: &mut Vec<String>,
    params: &mut Vec<Value>,
    range: &PartialRange,
    upper: &str,
) {
    bounds.push(format!("(r.occurred_at >= ? AND r.occurred_at {upper} ?)"));
    params.push(Value::Text(range.from.clone()));
    params.push(Value::Text(range.to.clone()));
}

pub(crate) fn dimension_clauses(filter: &Filter, alias: &str) -> (Vec<String>, Vec<Value>) {
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    for (column, values) in [
        ("source", &filter.sources),
        ("model", &filter.models),
        ("project", &filter.projects),
        ("provider", &filter.providers),
    ] {
        if values.is_empty() {
            continue;
        }
        clauses.push(format!(
            "{alias}.{column} IN ({})",
            placeholders(values.len())
        ));
        for value in values {
            params.push(Value::Text(value.clone()));
        }
    }
    (clauses, params)
}

fn placeholders(n: usize) -> String {
    std::iter::repeat_n("?", n).collect::<Vec<_>>().join(", ")
}

fn select_sql(list: &str, from: &str, clauses: &[String]) -> String {
    format!("SELECT {list}\nFROM {from}{}", where_sql(clauses))
}

fn where_sql(clauses: &[String]) -> String {
    if clauses.is_empty() {
        String::new()
    } else {
        format!("\nWHERE {}", clauses.join(" AND "))
    }
}

fn union_all(left: RollupSource, right: RollupSource) -> RollupSource {
    let mut params = left.params;
    params.extend(right.params);
    RollupSource {
        sql: format!("{}\nUNION ALL\n{}", left.sql, right.sql),
        params,
    }
}
