use rusqlite::{params_from_iter, types::Value, Connection};

use crate::domain::{
    CostSource, Filter, PriceTable, SessionPage, SessionQuery, SessionRow, TurnRow, UsageCallPage,
    UsageCallRow,
};
use crate::rollup_source::rollup_source;
use crate::rollup_split::rollup_plan;

use super::sql::*;

pub fn top_sessions(
    conn: &Connection,
    filter: &Filter,
    prices: &PriceTable,
    limit: usize,
) -> Result<Vec<SessionRow>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
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
    let sql = session_rollup_sql(&inner.sql, true, true);
    let mut params = inner.params;
    params.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(params.iter()), |row| {
            Ok(SessionRow {
                source: row.get(0)?,
                session_id: row.get(1)?,
                total_tokens: row.get(2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
                project: row.get(5)?,
                model: row.get(6)?,
                source_file: row.get(7)?,
                cost: row.get(8)?,
                unpriced: row.get::<_, i64>(9)? > 0,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// 会话列表的分页查询：搜索（session/项目/模型/应用/原始文件）、排序、分页均在 SQL 层完成。
/// 汇总与当前页共用一次 MATERIALIZED 聚合，避免对消耗记录扫两遍。
pub fn sessions_page(
    conn: &Connection,
    prices: &PriceTable,
    query: &SessionQuery,
) -> Result<SessionPage, String> {
    let include_cost = query.include_cost.unwrap_or(false);
    if include_cost {
        install_prices(conn, prices)?;
    }
    let inner = rollup_source(
        &rollup_plan(
            query.filter.from.as_deref(),
            query.filter.to.as_deref(),
            crate::store::rollup_is_ready(conn),
            None,
        ),
        &query.filter,
    );
    let sessions_cte = session_rollup_sql(&inner.sql, include_cost, false);
    let mut params = inner.params;

    let search_clause = match query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(search) => {
            let pattern = format!("%{}%", escape_like(search));
            for _ in 0..5 {
                params.push(Value::Text(pattern.clone()));
            }
            "WHERE (session_id LIKE ? ESCAPE '\\' OR project LIKE ? ESCAPE '\\'
                OR model LIKE ? ESCAPE '\\' OR source LIKE ? ESCAPE '\\'
                OR source_file LIKE ? ESCAPE '\\')"
                .to_string()
        }
        None => String::new(),
    };

    let sort_column = match query.sort_by.as_deref() {
        Some("session") => "session_id",
        Some("application") => "source",
        Some("project") => "project",
        Some("model") => "model",
        Some("cost") => "cost",
        Some("time") => "ended_at",
        _ => "total_tokens",
    };
    let sort_dir = if query.sort_dir.as_deref() == Some("asc") {
        "ASC"
    } else {
        "DESC"
    };

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 20_000);
    let offset = (page - 1) * page_size;
    params.push(Value::Integer(page_size as i64));
    params.push(Value::Integer(offset as i64));

    let sql = format!(
        "WITH sessions AS MATERIALIZED ({sessions_cte}),
            filtered AS MATERIALIZED (
                SELECT * FROM sessions {search_clause}
            ),
            summary AS (
                SELECT COUNT(*) AS match_count,
                    COALESCE(SUM(total_tokens), 0) AS match_tokens,
                    MAX(ended_at) AS match_last_ended
                FROM filtered
            ),
            page AS (
                SELECT session_id, source, project, model, total_tokens, started_at, ended_at,
                    source_file, cost, unpriced_count
                FROM filtered
                ORDER BY {sort_column} {sort_dir}, session_id ASC
                LIMIT ? OFFSET ?
            )
         SELECT summary.match_count, summary.match_tokens, summary.match_last_ended,
            page.session_id, page.source, page.project, page.model, page.total_tokens,
            page.started_at, page.ended_at, page.source_file, page.cost, page.unpriced_count
         FROM summary
         LEFT JOIN page ON 1"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let raw = stmt
        .query_map(params_from_iter(params.iter()), |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<f64>>(11)?,
                row.get::<_, Option<i64>>(12)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut total = 0;
    let mut total_tokens = 0;
    let mut last_ended = None;
    let mut rows = Vec::new();
    for (
        match_count,
        match_tokens,
        match_last_ended,
        session_id,
        source,
        project,
        model,
        row_tokens,
        started_at,
        ended_at,
        source_file,
        cost,
        unpriced_count,
    ) in raw
    {
        total = match_count;
        total_tokens = match_tokens;
        last_ended = match_last_ended;
        let Some(session_id) = session_id else {
            continue;
        };
        rows.push(SessionRow {
            session_id,
            source: source.unwrap_or_default(),
            project: project.unwrap_or_default(),
            model: model.unwrap_or_default(),
            total_tokens: row_tokens.unwrap_or(0),
            started_at: started_at.unwrap_or_default(),
            ended_at: ended_at.unwrap_or_default(),
            source_file: source_file.unwrap_or_default(),
            cost,
            unpriced: unpriced_count.unwrap_or(0) > 0,
        });
    }

    Ok(SessionPage {
        rows,
        total,
        total_tokens,
        last_ended,
    })
}

pub fn session_turns(
    conn: &Connection,
    session_id: &str,
    source: Option<&str>,
    filter: &Filter,
    prices: &PriceTable,
) -> Result<Vec<TurnRow>, String> {
    install_prices(conn, prices)?;
    let (mut clauses, mut params) = filter_clauses(filter);
    clauses.push("r.session_id = ?".to_string());
    params.push(Value::Text(session_id.to_string()));
    if let Some(source) = source {
        clauses.push("r.source = ?".to_string());
        params.push(Value::Text(source.to_string()));
    }
    let sql = format!(
        "SELECT r.occurred_at, r.model, r.provider,
            r.input_tokens, r.output_tokens, r.cache_read_tokens, r.cache_creation_tokens,
            r.reasoning_tokens, r.total_tokens, r.source_file,
            {COST_EXPR},
            {UNPRICED_EXPR},
            {COST_SOURCE_EXPR}
        FROM usage_records r
        {PRICE_JOINS}
        {}
        ORDER BY r.occurred_at",
        where_sql(&clauses),
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(params.iter()), |row| {
            let cost: Option<f64> = row.get(10)?;
            let unpriced: i64 = row.get(11)?;
            let cost_source = CostSource::from_sql(row.get::<_, String>(12)?.as_str());
            Ok(TurnRow {
                occurred_at: row.get(0)?,
                model: row.get(1)?,
                provider: row.get(2)?,
                input_tokens: row.get(3)?,
                output_tokens: row.get(4)?,
                cache_read_tokens: row.get(5)?,
                cache_creation_tokens: row.get(6)?,
                reasoning_tokens: row.get(7)?,
                total_tokens: row.get(8)?,
                source_file: row.get(9)?,
                cost,
                unpriced: unpriced > 0,
                cost_source,
                cost_note: Some(cost_source.note().to_string()),
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

const MAX_USAGE_CALL_PAGE_SIZE: u32 = 200;

/// 按当前筛选分页列出单条消耗记录，供 Provider 等聚合页下钻「明细调用」。
pub fn usage_calls_page(
    conn: &Connection,
    filter: &Filter,
    prices: &PriceTable,
    page: u32,
    page_size: u32,
) -> Result<UsageCallPage, String> {
    install_prices(conn, prices)?;
    let page = page.max(1);
    let page_size = page_size.clamp(1, MAX_USAGE_CALL_PAGE_SIZE);
    let (clauses, params) = filter_clauses(filter);
    let where_sql = where_sql(&clauses);
    let total = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM usage_records r {where_sql}"),
            params_from_iter(params.iter()),
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())? as u32;
    let offset = i64::from((page - 1) * page_size);
    let mut list_params = params;
    list_params.push(Value::Integer(i64::from(page_size)));
    list_params.push(Value::Integer(offset));
    let sql = format!(
        "SELECT r.occurred_at, r.source, r.model, r.provider, r.project, r.session_id,
            r.input_tokens, r.output_tokens, r.cache_read_tokens, r.cache_creation_tokens,
            r.reasoning_tokens, r.total_tokens,
            {COST_EXPR},
            {UNPRICED_EXPR}
        FROM usage_records r
        {PRICE_JOINS}
        {where_sql}
        ORDER BY r.occurred_at DESC, r.source ASC, r.session_id ASC
        LIMIT ? OFFSET ?"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(list_params.iter()), |row| {
            Ok(UsageCallRow {
                occurred_at: row.get(0)?,
                source: row.get(1)?,
                model: row.get(2)?,
                provider: row.get(3)?,
                project: row.get(4)?,
                session_id: row.get(5)?,
                input_tokens: row.get(6)?,
                output_tokens: row.get(7)?,
                cache_read_tokens: row.get(8)?,
                cache_creation_tokens: row.get(9)?,
                reasoning_tokens: row.get(10)?,
                total_tokens: row.get(11)?,
                cost: row.get(12)?,
                unpriced: row.get::<_, i64>(13)? > 0,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(UsageCallPage { rows, total })
}
