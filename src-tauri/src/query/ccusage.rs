use rusqlite::{params_from_iter, Connection};

use crate::ccusage_export::{self, CcusageDailyJson, ModelDayRow};
use crate::domain::{Filter, PriceTable};
use crate::rollup_source::rollup_source;
use crate::rollup_split::rollup_plan;

use super::sql::*;

/// 与 `ccusage daily --json` 同构。按本地日历日切，不叠 Cursor 账号用量。
pub fn ccusage_daily(
    conn: &Connection,
    filter: &Filter,
    prices: &PriceTable,
) -> Result<CcusageDailyJson, String> {
    install_prices(conn, prices)?;
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
        "SELECT strftime('%Y-%m-%d', d.first_at, 'localtime') AS date,
                d.model,
                COALESCE(SUM(d.input_tokens), 0),
                COALESCE(SUM(d.output_tokens), 0),
                COALESCE(SUM(d.cache_creation_tokens), 0),
                COALESCE(SUM(d.cache_read_tokens), 0),
                COALESCE(SUM({ROLLUP_COST_EXPR}), 0)
         FROM ({}) d
         {ROLLUP_PRICE_JOINS}
         GROUP BY 1, 2
         ORDER BY 1, 2",
        inner.sql,
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(inner.params.iter()), |row| {
            Ok(ModelDayRow {
                date: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                model: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                input_tokens: row.get(2)?,
                output_tokens: row.get(3)?,
                cache_creation_tokens: row.get(4)?,
                cache_read_tokens: row.get(5)?,
                cost: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut model_rows = Vec::new();
    for row in rows {
        let row = row.map_err(|e| e.to_string())?;
        if row.date.is_empty() {
            continue;
        }
        model_rows.push(row);
    }
    Ok(ccusage_export::from_model_rows(model_rows))
}

pub fn ccusage_daily_json(
    conn: &Connection,
    filter: &Filter,
    prices: &PriceTable,
) -> Result<String, String> {
    ccusage_export::to_pretty_json(&ccusage_daily(conn, filter, prices)?)
}
