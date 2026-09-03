use rusqlite::{params_from_iter, Connection};

use crate::cursor_account;
use crate::domain::{Filter, NamedAmount, PriceTable, SeriesPoint, Source};
use crate::rollup_source::rollup_source;
use crate::rollup_split::rollup_plan;

use super::sql::*;

pub fn trend(
    conn: &Connection,
    filter: &Filter,
    prices: &PriceTable,
    grain: &str,
) -> Result<Vec<SeriesPoint>, String> {
    install_prices(conn, prices)?;
    let inner = rollup_source(
        &rollup_plan(
            filter.from.as_deref(),
            filter.to.as_deref(),
            crate::store::rollup_is_ready(conn),
            Some(grain),
        ),
        filter,
    );
    let bucket = bucket_expr(grain);
    let sql = format!(
        "SELECT {bucket} AS bucket,
            SUM(d.total_tokens),
            SUM(d.input_tokens),
            SUM(d.output_tokens),
            SUM(d.cache_read_tokens),
            SUM(d.cache_creation_tokens),
            SUM(d.reasoning_tokens),
            SUM({ROLLUP_COST_EXPR})
        FROM ({}) d
        {ROLLUP_PRICE_JOINS}
        GROUP BY 1
        ORDER BY 1",
        inner.sql,
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(inner.params.iter()), |row| {
            Ok(SeriesPoint {
                bucket: row.get(0)?,
                total_tokens: row.get(1)?,
                input_tokens: row.get(2)?,
                output_tokens: row.get(3)?,
                cache_read_tokens: row.get(4)?,
                cache_creation_tokens: row.get(5)?,
                reasoning_tokens: row.get(6)?,
                cost: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let points = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let events = cursor_account::events_for_application_analytics(conn, filter)?;
    Ok(crate::aggregate::attach_cursor_trend(
        points, &events, prices, grain,
    ))
}

/// 按本地一天中的第几个小时（0–23）跨天汇总 token。
///
/// 与 `trend(grain="hour")` 不同：这里合并所有日期的同一小时，不保留时间轴。
/// 小时无法从日级预聚合还原，强制走明细。
pub fn hour_of_day(conn: &Connection, filter: &Filter) -> Result<[i64; 24], String> {
    let inner = rollup_source(
        &rollup_plan(
            filter.from.as_deref(),
            filter.to.as_deref(),
            crate::store::rollup_is_ready(conn),
            Some("hour"),
        ),
        filter,
    );
    let sql = format!(
        "SELECT CAST(strftime('%H', d.first_at, 'localtime') AS INTEGER),
                COALESCE(SUM(d.total_tokens), 0)
         FROM ({}) d
         GROUP BY 1",
        inner.sql,
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(inner.params.iter()), |row| {
            Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut hours = [0i64; 24];
    for row in rows {
        let (hour, tokens) = row.map_err(|e| e.to_string())?;
        let Some(hour) = hour else {
            continue;
        };
        if (0..24).contains(&hour) {
            hours[hour as usize] = tokens;
        }
    }
    Ok(hours)
}

/// 按本地日历日汇总 token。与 `trend(grain="day")` 不同：这里用本地日切，不叠 Cursor 账号用量。
pub fn tokens_by_local_day(
    conn: &Connection,
    filter: &Filter,
) -> Result<Vec<(String, i64)>, String> {
    let inner = rollup_source(
        &rollup_plan(
            filter.from.as_deref(),
            filter.to.as_deref(),
            crate::store::rollup_is_ready(conn),
            Some("hour"),
        ),
        filter,
    );
    let sql = format!(
        "SELECT strftime('%Y-%m-%d', d.first_at, 'localtime'),
                COALESCE(SUM(d.total_tokens), 0)
         FROM ({}) d
         GROUP BY 1
         ORDER BY 1",
        inner.sql,
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(inner.params.iter()), |row| {
            Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut days = Vec::new();
    for row in rows {
        let (date, tokens) = row.map_err(|e| e.to_string())?;
        let Some(date) = date else {
            continue;
        };
        days.push((date, tokens));
    }
    Ok(days)
}

fn breakdown_name_expr(dimension: &str) -> Result<&'static str, String> {
    match dimension {
        "application" | "source" => Ok("d.source"),
        "model" => Ok("d.model"),
        "provider" => Ok("d.provider"),
        "project" => Ok("d.project"),
        _ => Err(format!("不支持的统计维度：{dimension}")),
    }
}

fn display_name(raw: &str, dimension: &str) -> String {
    if dimension == "application" {
        Source::parse(raw)
            .map(|s| s.application_name().to_string())
            .unwrap_or_else(|| raw.to_string())
    } else if raw.is_empty() {
        "（未标注）".to_string()
    } else {
        raw.to_string()
    }
}

pub fn breakdown(
    conn: &Connection,
    filter: &Filter,
    prices: &PriceTable,
    dimension: &str,
) -> Result<Vec<NamedAmount>, String> {
    install_prices(conn, prices)?;
    let name_expr = breakdown_name_expr(dimension)?;
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
        "SELECT {name_expr} AS name,
            SUM(d.total_tokens),
            SUM({ROLLUP_COST_EXPR}),
            COALESCE(SUM({ROLLUP_UNPRICED_EXPR}), 0)
        FROM ({}) d
        {ROLLUP_PRICE_JOINS}
        GROUP BY 1",
        inner.sql,
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let raw = stmt
        .query_map(params_from_iter(inner.params.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<f64>>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let grand: i64 = raw.iter().map(|(_, total, _, _)| *total).sum();
    let mut rows: Vec<NamedAmount> = raw
        .into_iter()
        .map(|(name, total_tokens, cost, unpriced_count)| NamedAmount {
            name: display_name(&name, dimension),
            total_tokens,
            share: if grand == 0 {
                0.0
            } else {
                total_tokens as f64 / grand as f64
            },
            cost,
            unpriced: unpriced_count > 0,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.name.cmp(&b.name))
    });
    if dimension == "project" {
        let events = cursor_account::events_for_application_analytics(conn, filter)?;
        return Ok(crate::aggregate::attach_cursor_project_breakdown(
            rows, &events, prices,
        ));
    }
    Ok(rows)
}
