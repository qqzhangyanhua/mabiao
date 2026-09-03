use chrono::{DateTime, Utc};
use rusqlite::{params_from_iter, types::Value, Connection};

use crate::billing_window;
use crate::cursor_account;
use crate::domain::{BillingWindowsDto, Filter, PriceTable, Source};

use super::sql::*;

pub fn billing_windows(
    conn: &Connection,
    filter: &Filter,
    prices: &PriceTable,
    now: DateTime<Utc>,
) -> Result<BillingWindowsDto, String> {
    install_prices(conn, prices)?;
    let scoped = Filter {
        from: None,
        to: None,
        sources: filter.sources.clone(),
        models: filter.models.clone(),
        projects: filter.projects.clone(),
        providers: filter.providers.clone(),
    };
    let (mut clauses, mut params) = filter_clauses(&scoped);
    // 日期前缀比较能走 idx_usage_occurred；substr(occurred_at) 会废掉索引。
    clauses.push("r.occurred_at >= ?".to_string());
    params.push(Value::Text(billing_window::lookback_date(now)));
    let sql = format!(
        "SELECT
            r.occurred_at, r.source, r.session_id,
            r.input_tokens, r.output_tokens, r.cache_read_tokens,
            r.cache_creation_tokens, r.reasoning_tokens, r.total_tokens,
            {COST_EXPR}, {UNPRICED_EXPR}
        FROM usage_records r
        {PRICE_JOINS}
        {}",
        where_sql(&clauses),
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(params.iter()), |row| {
            let source_value: String = row.get(1)?;
            let source = Source::parse(&source_value).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    format!("未知来源：{source_value}").into(),
                )
            })?;
            Ok(billing_window::BillingEvent {
                occurred_at: row.get(0)?,
                source,
                session_id: row.get(2)?,
                input_tokens: row.get(3)?,
                output_tokens: row.get(4)?,
                cache_read_tokens: row.get(5)?,
                cache_creation_tokens: row.get(6)?,
                reasoning_tokens: row.get(7)?,
                total_tokens: row.get(8)?,
                cost: row.get(9)?,
                unpriced: row.get::<_, i64>(10)? > 0,
            })
        })
        .map_err(|e| e.to_string())?;
    let events = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let dto = billing_window::summarize_events(&events, now);
    let cursor_events = cursor_account::events_for_weekly_window(conn, filter)?;
    Ok(billing_window::attach_cursor_weekly(
        dto,
        &cursor_events,
        prices,
        now,
    ))
}
