use std::collections::BTreeMap;

use rusqlite::{params_from_iter, Connection};

use crate::cost::{finish_unpriced_groups, UnpricedGroupAcc};
use crate::domain::{
    Filter, OverviewCostBreakdown, OverviewCostSources, OverviewDto, PriceTable, UnpricedGroupDto,
};
use crate::rollup_source::rollup_source;
use crate::rollup_split::rollup_plan;

use super::sql::*;

pub fn overview(
    conn: &Connection,
    filter: &Filter,
    prices: &PriceTable,
) -> Result<OverviewDto, String> {
    install_prices(conn, prices)?;
    let inner = rollup_source(
        &rollup_plan(
            filter.from.as_deref(),
            filter.to.as_deref(),
            crate::store::rollup_is_ready(conn),
            None,
        ),
        filter,
    );
    let sql = format!(
        "SELECT
            COALESCE(SUM(d.total_tokens), 0),
            COALESCE(SUM(d.input_tokens), 0),
            COALESCE(SUM(d.output_tokens), 0),
            COALESCE(SUM(d.cache_read_tokens), 0),
            COALESCE(SUM(d.cache_creation_tokens), 0),
            COALESCE(SUM(d.reasoning_tokens), 0),
            COUNT(DISTINCT d.source || char(31) || d.session_id),
            SUM({ROLLUP_COST_EXPR}),
            COALESCE(SUM({ROLLUP_UNPRICED_EXPR}), 0),
            SUM({ROLLUP_COST_INPUT_EXPR}),
            SUM({ROLLUP_COST_OUTPUT_EXPR}),
            SUM({ROLLUP_COST_CACHE_READ_EXPR}),
            SUM({ROLLUP_COST_CACHE_CREATION_EXPR}),
            SUM(CASE WHEN ({ROLLUP_COST_SOURCE_EXPR}) = 'native' THEN ({ROLLUP_COST_EXPR}) END),
            SUM(CASE WHEN ({ROLLUP_COST_SOURCE_EXPR}) = 'user' THEN ({ROLLUP_COST_EXPR}) END),
            SUM(CASE WHEN ({ROLLUP_COST_SOURCE_EXPR}) = 'snapshot' THEN ({ROLLUP_COST_EXPR}) END)
        FROM ({}) d
        {ROLLUP_PRICE_JOINS}",
        inner.sql,
    );
    conn.query_row(&sql, params_from_iter(inner.params.iter()), |row| {
        let unpriced_records: i64 = row.get(8)?;
        Ok(OverviewDto {
            total_tokens: row.get(0)?,
            input_tokens: row.get(1)?,
            output_tokens: row.get(2)?,
            cache_read_tokens: row.get(3)?,
            cache_creation_tokens: row.get(4)?,
            reasoning_tokens: row.get(5)?,
            session_count: row.get(6)?,
            cost: row.get(7)?,
            unpriced: unpriced_records > 0,
            cost_breakdown: OverviewCostBreakdown {
                input: row.get(9)?,
                output: row.get(10)?,
                cache_read: row.get(11)?,
                cache_creation: row.get(12)?,
            },
            cost_sources: OverviewCostSources {
                native: row.get(13)?,
                user: row.get(14)?,
                snapshot: row.get(15)?,
                unpriced_records,
            },
        })
    })
    .map_err(|e| e.to_string())
}

/// 全库未定价诊断：按 `(模型, provider)` 归组，不接筛选。
///
/// 只统计没能算出费用的那部分：来源自带费用、以及精确查价命中的行都排除。
/// 原因分档在这一层判定——空模型名是结构上无法计费，其余为可补单价。
/// 有预聚合表时走 `usage_rollup`（`has_native` 正好用来剔除自带费用），否则回落明细表。
pub fn unpriced_diagnosis(
    conn: &Connection,
    prices: &PriceTable,
) -> Result<Vec<UnpricedGroupDto>, String> {
    install_prices(conn, prices)?;
    let groups = if crate::store::rollup_is_ready(conn) {
        unpriced_diagnosis_from_rollup(conn)?
    } else {
        unpriced_diagnosis_from_records(conn)?
    };
    Ok(finish_unpriced_groups(groups, prices))
}

fn unpriced_diagnosis_from_rollup(
    conn: &Connection,
) -> Result<BTreeMap<(String, String), UnpricedGroupAcc>, String> {
    let sql = format!(
        "SELECT d.model, d.provider, d.source,
            SUM(d.total_tokens),
            SUM(d.record_count)
         FROM usage_rollup d
         {ROLLUP_PRICE_JOINS}
         WHERE {ROLLUP_UNPRICED_EXPR} > 0
         GROUP BY d.model, d.provider, d.source"
    );
    fold_unpriced_groups(conn, &sql)
}

fn unpriced_diagnosis_from_records(
    conn: &Connection,
) -> Result<BTreeMap<(String, String), UnpricedGroupAcc>, String> {
    let sql = format!(
        "SELECT r.model, r.provider, r.source,
            SUM(r.total_tokens),
            COUNT(*)
         FROM usage_records r
         {PRICE_JOINS}
         WHERE {UNPRICED_EXPR} = 1
         GROUP BY r.model, r.provider, r.source"
    );
    fold_unpriced_groups(conn, &sql)
}

fn fold_unpriced_groups(
    conn: &Connection,
    sql: &str,
) -> Result<BTreeMap<(String, String), UnpricedGroupAcc>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut groups: BTreeMap<(String, String), UnpricedGroupAcc> = BTreeMap::new();
    for row in rows {
        let (model, provider, source, total_tokens, record_count) =
            row.map_err(|e| e.to_string())?;
        let acc = groups.entry((model, provider)).or_default();
        acc.sources.insert(source);
        acc.total_tokens += total_tokens;
        acc.record_count += record_count;
    }
    Ok(groups)
}

/// 全时段、全来源的费用标量。给代码量 ROI 用，不扫 token 维度、不算会话数。
pub fn lifetime_cost(
    conn: &Connection,
    prices: &PriceTable,
) -> Result<(Option<f64>, bool), String> {
    install_prices(conn, prices)?;
    let sql = format!(
        "SELECT SUM({COST_EXPR}), COALESCE(SUM({UNPRICED_EXPR}), 0)
         FROM usage_records r
         {PRICE_JOINS}"
    );
    conn.query_row(&sql, [], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? > 0)))
        .map_err(|e| e.to_string())
}
