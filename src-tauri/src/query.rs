//! SQL 下推的聚合查询：把原先「load_all 全量载入内存再聚合」改为在 sqlite 里
//! GROUP BY / 过滤，只返回聚合结果。费用通过临时价格表 `price_rows` LEFT JOIN 计算，
//! 与 `cost::derive_cost` 保持同一语义（native_cost 优先，其次 model+provider 匹配，
//! 再次 model 且 provider 为 NULL 的兜底，都没有则标记 unpriced；model/provider 大小写不敏感）。

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use rusqlite::{params, params_from_iter, types::Value, Connection};

use crate::billing_window;
use crate::cost::{finish_unpriced_groups, UnpricedGroupAcc};
use crate::cursor_account;
use crate::domain::{
    ApplicationAnalyticsDto, ApplicationEfficiency, ApplicationTrendPoint, BillingWindowsDto,
    CostSource, EfficiencyMetrics, Filter, FilterOptions, InstructionSourceUsage,
    InstructionUsageSummary, NamedAmount, OverviewDto, PriceTable, ProjectApplicationRow,
    SeriesPoint, SessionPage, SessionQuery, SessionRow, Source, TurnRow, UnpricedGroupDto,
    UsageCallPage, UsageCallRow, WorkSessionSpan, WorkTimelineDto,
};
use crate::rollup_source::rollup_source;
use crate::rollup_split::rollup_plan;

/// 费用表达式（每行）：native_cost 优先，否则加权价格，否则 NULL（未定价）。
const COST_EXPR: &str = "
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
const UNPRICED_EXPR: &str = "
    CASE
        WHEN r.native_cost IS NOT NULL THEN 0
        WHEN COALESCE(pe.input, pf.input) IS NOT NULL THEN 0
        ELSE 1
    END";

/// 费用来源：native > 精确匹配条目 origin > 兜底条目 origin > none。
const COST_SOURCE_EXPR: &str = "
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
const PRICE_JOINS: &str = "
    LEFT JOIN price_rows pe ON pe.model = r.model AND pe.provider = lower(r.provider)
    LEFT JOIN price_rows pf ON pf.model = r.model AND pf.provider IS NULL";

/// 把价目表装进临时表 `price_rows`，供 `PRICE_JOINS` 取价。
///
/// 每次查询重建一遍看着浪费，实测却只要 1.7ms——SQLite 在单个事务里插 1400 行就是这么快。
/// 曾经按指纹跳过重建，收益不到首屏的 1%，不值当缓存失效那份复杂度，已经撤掉。
/// 真正的开销在 `PRICE_JOINS` 逐行取价那边，那个换不掉：先按 (model, provider) 聚合再算价
/// 数学上等价，但 GROUP BY 比 JOIN 更贵，实测反而慢。
fn install_prices(conn: &Connection, prices: &PriceTable) -> Result<(), String> {
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

/// Filter → (WHERE 子句片段列表, 参数)。所有列都加 `r.` 前缀（表别名 r）。
fn filter_clauses(filter: &Filter) -> (Vec<String>, Vec<Value>) {
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
    if !filter.sources.is_empty() {
        clauses.push(format!(
            "r.source IN ({})",
            placeholders(filter.sources.len())
        ));
        for s in &filter.sources {
            params.push(Value::Text(s.clone()));
        }
    }
    if !filter.models.is_empty() {
        clauses.push(format!(
            "r.model IN ({})",
            placeholders(filter.models.len())
        ));
        for m in &filter.models {
            params.push(Value::Text(m.clone()));
        }
    }
    if !filter.projects.is_empty() {
        clauses.push(format!(
            "r.project IN ({})",
            placeholders(filter.projects.len())
        ));
        for p in &filter.projects {
            params.push(Value::Text(p.clone()));
        }
    }
    if !filter.providers.is_empty() {
        clauses.push(format!(
            "r.provider IN ({})",
            placeholders(filter.providers.len())
        ));
        for p in &filter.providers {
            params.push(Value::Text(p.clone()));
        }
    }
    (clauses, params)
}

fn placeholders(n: usize) -> String {
    std::iter::repeat_n("?", n).collect::<Vec<_>>().join(", ")
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
fn escape_like(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn where_sql(clauses: &[String]) -> String {
    if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    }
}

fn ratio(numerator: i64, denominator: i64) -> Option<f64> {
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
const ROLLUP_COST_EXPR: &str = "
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
const ROLLUP_UNPRICED_EXPR: &str = "
    CASE
        WHEN d.has_native = 1 THEN 0
        WHEN COALESCE(pe.input, pf.input) IS NOT NULL THEN 0
        ELSE d.record_count
    END";

const ROLLUP_PRICE_JOINS: &str = "
    LEFT JOIN price_rows pe ON pe.model = d.model AND pe.provider = lower(d.provider)
    LEFT JOIN price_rows pf ON pf.model = d.model AND pf.provider IS NULL";

/// 查询相对预聚合表的走法。
///
/// `usage_rollup` 按 UTC 天聚合，装不下任意时刻的边界：前端默认「近 7 天」和托盘
/// 「今日」给的 `from`/`to` 都带时分秒，按天一刀切会把边界那天整天算进来。
/// 因此有时间窗时拆成三段——两端边界天走原始表，中间完整 UTC 日走预聚合。
/// 没有完整中间日（同一天或相邻两天）就整段回退原始表，避免为空的 UNION。
///
/// 预聚合未就绪（老库升级 / 旧备份恢复，补建还在后台）时也整段走原始表：慢一点，
/// 但数字是对的。就绪用显式标记而不是「表非空」——补建期间若发生一次摄取，增量
/// 重建会往空表里只写进那一两天，表非空了内容却只有零头。
#[derive(Debug, Clone, PartialEq, Eq)]
enum RollupMode {
    Raw,
    Rollup,
    Hybrid(TimeSplit),
}

/// 时间窗切分：`middle_after`/`middle_before` 是预聚合日的开区间；
/// `start_*` / `end_*` 是两端边界天在原始表上的半开/闭区间。
#[derive(Debug, Clone, PartialEq, Eq)]
struct TimeSplit {
    middle_after: Option<String>,
    middle_before: Option<String>,
    start_from: Option<String>,
    start_before: Option<String>,
    end_from: Option<String>,
    end_to: Option<String>,
}

impl TimeSplit {
    fn has_middle(&self) -> bool {
        match (&self.middle_after, &self.middle_before) {
            (Some(after), Some(before)) => {
                next_utc_day(after).is_some_and(|next| next.as_str() < before.as_str())
            }
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        }
    }
}

fn utc_day(iso: &str) -> Option<&str> {
    let day = iso.get(..10)?;
    if day.as_bytes().get(4) == Some(&b'-') && day.as_bytes().get(7) == Some(&b'-') {
        Some(day)
    } else {
        None
    }
}

fn next_utc_day(day: &str) -> Option<String> {
    let parsed = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    Some((parsed + Duration::days(1)).format("%Y-%m-%d").to_string())
}

fn time_split(from: Option<&str>, to: Option<&str>) -> Option<TimeSplit> {
    let from_day = match from {
        Some(value) => Some(utc_day(value)?.to_string()),
        None => None,
    };
    let to_day = match to {
        Some(value) => Some(utc_day(value)?.to_string()),
        None => None,
    };
    if from_day.is_none() && to_day.is_none() {
        return None;
    }
    let mut split = TimeSplit {
        middle_after: from_day.clone(),
        middle_before: to_day.clone(),
        start_from: None,
        start_before: None,
        end_from: None,
        end_to: None,
    };
    if let (Some(from), Some(day)) = (from, from_day.as_deref()) {
        split.start_from = Some(from.to_string());
        split.start_before = Some(format!("{day}~"));
    }
    if let (Some(to), Some(day)) = (to, to_day.as_deref()) {
        split.end_from = Some(day.to_string());
        split.end_to = Some(to.to_string());
    }
    Some(split)
}

fn rollup_mode(conn: &Connection, filter: &Filter) -> RollupMode {
    if !crate::store::rollup_is_ready(conn) {
        return RollupMode::Raw;
    }
    if filter.from.is_none() && filter.to.is_none() {
        return RollupMode::Rollup;
    }
    match time_split(filter.from.as_deref(), filter.to.as_deref()) {
        Some(split) if split.has_middle() => RollupMode::Hybrid(split),
        _ => RollupMode::Raw,
    }
}

/// 维度过滤（不含时间）。`alias` 为表别名（`r` / `d`）。
fn dimension_clauses(filter: &Filter, alias: &str) -> (Vec<String>, Vec<Value>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<Value> = Vec::new();
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

/// 预聚合表过滤：维度 + 可选的中间完整日开区间。
fn rollup_filter_clauses(filter: &Filter, split: Option<&TimeSplit>) -> (Vec<String>, Vec<Value>) {
    let (mut clauses, mut params) = dimension_clauses(filter, "d");
    if let Some(split) = split {
        if let Some(after) = &split.middle_after {
            clauses.push("d.day > ?".to_string());
            params.push(Value::Text(after.clone()));
        }
        if let Some(before) = &split.middle_before {
            clauses.push("d.day < ?".to_string());
            params.push(Value::Text(before.clone()));
        }
    }
    (clauses, params)
}

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
            COALESCE(SUM({ROLLUP_UNPRICED_EXPR}), 0)
        FROM ({}) d
        {ROLLUP_PRICE_JOINS}",
        inner.sql,
    );
    conn.query_row(&sql, params_from_iter(inner.params.iter()), |row| {
        Ok(OverviewDto {
            total_tokens: row.get(0)?,
            input_tokens: row.get(1)?,
            output_tokens: row.get(2)?,
            cache_read_tokens: row.get(3)?,
            cache_creation_tokens: row.get(4)?,
            reasoning_tokens: row.get(5)?,
            session_count: row.get(6)?,
            cost: row.get(7)?,
            unpriced: row.get::<_, i64>(8)? > 0,
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

/// 预聚合表的时间桶。表按 UTC 天聚合，比天更细的粒度取不出来，只能回原始表。
fn rollup_bucket_expr(grain: &str) -> Option<&'static str> {
    match grain {
        "hour" => None,
        "week" => Some("strftime('%G-W%V', d.day)"),
        "month" => Some("substr(d.day, 1, 7)"),
        _ => Some("d.day"),
    }
}

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
    // 小时粒度 rollup_plan 已强制纯明细：first_at 即原 occurred_at，可还原小时桶。
    // 不能回落到预聚合行的 first_at——那是当日最早时刻，会把全天塌进第一个小时。
    let bucket = match grain {
        "hour" => "substr(d.first_at, 1, 13)",
        _ => rollup_bucket_expr(grain).unwrap_or("d.day"),
    };
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

pub fn application_analytics(
    conn: &Connection,
    filter: &Filter,
    grain: &str,
) -> Result<ApplicationAnalyticsDto, String> {
    // 四组聚合先物化同一工厂子查询，切分时明细只扫一遍。
    // COUNT(DISTINCT session) 在 UNION ALL 之后计算，跨 partial 边界只计一次。
    let inner = rollup_source(
        &rollup_plan(
            filter.from.as_deref(),
            filter.to.as_deref(),
            crate::store::rollup_is_ready(conn),
            Some(grain),
        ),
        filter,
    );
    conn.execute("DROP TABLE IF EXISTS application_analytics_src", [])
        .map_err(|e| e.to_string())?;
    conn.execute(
        &format!(
            "CREATE TEMP TABLE application_analytics_src AS {}",
            inner.sql
        ),
        params_from_iter(inner.params.iter()),
    )
    .map_err(|e| e.to_string())?;
    // 小时粒度 rollup_plan 已强制纯明细：first_at 即原 occurred_at，可还原小时桶。
    // 不能回落到预聚合行的 first_at——那是当日最早时刻，会把全天塌进第一个小时。
    let bucket = match grain {
        "hour" => "substr(d.first_at, 1, 13)",
        _ => rollup_bucket_expr(grain).unwrap_or("d.day"),
    };

    let summary_sql = "SELECT
            COALESCE(SUM(d.total_tokens), 0),
            COALESCE(SUM(d.input_tokens), 0),
            COALESCE(SUM(d.cache_read_tokens), 0),
            COALESCE(SUM(d.reasoning_tokens), 0),
            COUNT(DISTINCT d.source || char(31) || d.session_id)
        FROM application_analytics_src d";
    let (total, input, cache_read, reasoning, session_count) = conn
        .query_row(summary_sql, [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let summary = EfficiencyMetrics {
        total_tokens: total,
        session_count,
        cache_hit_rate: ratio(cache_read, input + cache_read),
        average_session_tokens: if session_count == 0 {
            None
        } else {
            Some(total as f64 / session_count as f64)
        },
        reasoning_share: ratio(reasoning, total),
    };

    let app_sql = "SELECT d.source,
            SUM(d.total_tokens),
            SUM(d.input_tokens),
            SUM(d.cache_read_tokens),
            SUM(d.reasoning_tokens),
            COUNT(DISTINCT d.session_id)
        FROM application_analytics_src d
        GROUP BY d.source";
    let mut stmt = conn.prepare(app_sql).map_err(|e| e.to_string())?;
    let app_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut by_application: Vec<ApplicationEfficiency> = app_rows
        .into_iter()
        .filter_map(
            |(source, total, input, cache_read, reasoning, session_count)| {
                let parsed = Source::parse(&source)?;
                Some(ApplicationEfficiency {
                    source,
                    application: parsed.application_name().to_string(),
                    metrics: EfficiencyMetrics {
                        total_tokens: total,
                        session_count,
                        cache_hit_rate: ratio(cache_read, input + cache_read),
                        average_session_tokens: if session_count == 0 {
                            None
                        } else {
                            Some(total as f64 / session_count as f64)
                        },
                        reasoning_share: ratio(reasoning, total),
                    },
                })
            },
        )
        .collect();
    by_application.sort_by(|a, b| {
        b.metrics
            .total_tokens
            .cmp(&a.metrics.total_tokens)
            .then_with(|| a.application.cmp(&b.application))
    });

    let trend_sql = format!(
        "SELECT {bucket} AS bucket, d.source, SUM(d.total_tokens)
        FROM application_analytics_src d
        GROUP BY 1, 2
        ORDER BY 1, 2"
    );
    let mut stmt = conn.prepare(&trend_sql).map_err(|e| e.to_string())?;
    let trend_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut trend_map: BTreeMap<String, ApplicationTrendPoint> = BTreeMap::new();
    for (bucket, source, total) in trend_rows {
        let point = trend_map
            .entry(bucket.clone())
            .or_insert_with(|| ApplicationTrendPoint {
                bucket,
                total_tokens: 0,
                values: BTreeMap::new(),
            });
        point.total_tokens += total;
        *point.values.entry(source).or_default() += total;
    }

    let project_sql = "SELECT d.project, d.source, SUM(d.total_tokens)
        FROM application_analytics_src d
        GROUP BY 1, 2
        ORDER BY 1, 2";
    let mut stmt = conn.prepare(project_sql).map_err(|e| e.to_string())?;
    let project_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut projects_map: BTreeMap<String, ProjectApplicationRow> = BTreeMap::new();
    for (project, source, total) in project_rows {
        let project = if project.is_empty() {
            "（未标注）".to_string()
        } else {
            project
        };
        let row = projects_map
            .entry(project.clone())
            .or_insert_with(|| ProjectApplicationRow {
                project,
                total_tokens: 0,
                values: BTreeMap::new(),
            });
        row.total_tokens += total;
        *row.values.entry(source).or_default() += total;
    }
    let mut projects: Vec<ProjectApplicationRow> = projects_map.into_values().collect();
    projects.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.project.cmp(&b.project))
    });

    let dto = ApplicationAnalyticsDto {
        summary,
        by_application,
        trend: trend_map.into_values().collect(),
        projects,
    };
    let events = cursor_account::events_for_application_analytics(conn, filter)?;
    Ok(crate::aggregate::attach_cursor_application(
        dto, &events, grain,
    ))
}

/// 从预聚合表取 Top N 会话。
///
/// 比原始表那条路少一次回表：`usage_rollup` 的主键里就带着 project / model，
/// `file_key` 也已按「最晚非空」的形式存好，一次 GROUP BY 就能把展示标签一并算出来。
///
/// project / model 的键这里用 `last_at` 现拼——组内 `last_at` 就是该组 `occurred_at`
/// 的最大值，与逐行版 `MAX(occurred_at || sep || value)` 选出的是同一个值：
/// 时间大的胜出，时间并列时字典序大的胜出。
fn top_sessions_from_rollup(
    conn: &Connection,
    filter: &Filter,
    limit: usize,
) -> Result<Vec<SessionRow>, String> {
    let (clauses, mut params) = rollup_filter_clauses(filter, None);
    let project = unwrap_latest_key_sql("project_key");
    let model = unwrap_latest_key_sql("model_key");
    let source_file = unwrap_latest_key_sql("file_key");
    let sql = format!(
        "SELECT source, session_id, total_tokens, started_at, ended_at, cost, unpriced_count,
            {project} AS project,
            {model} AS model,
            {source_file} AS source_file
         FROM (
            SELECT d.source AS source, d.session_id AS session_id,
                SUM(d.total_tokens) AS total_tokens,
                MIN(d.first_at) AS started_at,
                MAX(d.last_at) AS ended_at,
                SUM({ROLLUP_COST_EXPR}) AS cost,
                COALESCE(SUM({ROLLUP_UNPRICED_EXPR}), 0) AS unpriced_count,
                MAX(CASE WHEN d.project != '' THEN d.last_at || char(31) || d.project END) AS project_key,
                MAX(CASE WHEN d.model != '' THEN d.last_at || char(31) || d.model END) AS model_key,
                MAX(d.file_key) AS file_key
            FROM usage_rollup d
            {ROLLUP_PRICE_JOINS}
            {}
            GROUP BY d.source, d.session_id
            ORDER BY total_tokens DESC, source ASC, session_id ASC
            LIMIT ?
         )",
        where_sql(&clauses),
    );
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
                cost: row.get(5)?,
                unpriced: row.get::<_, i64>(6)? > 0,
                project: row.get(7)?,
                model: row.get(8)?,
                source_file: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

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
    // 有时间窗的 hybrid 会话汇总要跨「中间日 + 边界天」再按会话合并，比直接扫原始表
    // 复杂且收益有限（会话页本就按索引裁时间窗）；只在无时间窗时走预聚合。
    if matches!(rollup_mode(conn, filter), RollupMode::Rollup) {
        return top_sessions_from_rollup(conn, filter, limit);
    }
    let (clauses, mut params) = filter_clauses(filter);
    // 先按 token 取出 Top N。相关子查询不能放进全表 GROUP BY：
    // 17 万行 × 3 次会话回表会把首屏卡死。
    let sql = format!(
        "SELECT r.source, r.session_id,
            SUM(r.total_tokens),
            MIN(r.occurred_at),
            MAX(r.occurred_at),
            SUM({COST_EXPR}),
            COALESCE(SUM({UNPRICED_EXPR}), 0)
        FROM usage_records r
        {PRICE_JOINS}
        {}
        GROUP BY r.source, r.session_id
        ORDER BY SUM(r.total_tokens) DESC, r.source ASC, r.session_id ASC
        LIMIT ?",
        where_sql(&clauses),
    );
    params.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let raw = stmt
        .query_map(params_from_iter(params.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<f64>>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut rows: Vec<SessionRow> = raw
        .into_iter()
        .map(
            |(source, session_id, total_tokens, started_at, ended_at, cost, unpriced_count)| {
                SessionRow {
                    session_id,
                    source,
                    project: String::new(),
                    model: String::new(),
                    total_tokens,
                    started_at,
                    ended_at,
                    source_file: String::new(),
                    cost,
                    unpriced: unpriced_count > 0,
                }
            },
        )
        .collect();
    hydrate_session_labels(conn, &mut rows)?;
    Ok(rows)
}

/// 回表补齐 top N 会话的展示标签（项目 / 模型 / 原始文件）。
///
/// 与 `session_rollup_sql` 同一套「一次扫描取最晚非空值」写法：内层 GROUP BY 聚出
/// `occurred_at || sep || value` 的 MAX，外层再切出值。早先这里用的是三个相关子查询，
/// 每个会话每列都要把该会话的全部行按 occurred_at 排一遍——首屏 top 8 会话合计 6.2 万行时
/// 实测 1.03s，换成一次扫描后 304ms。
fn hydrate_session_labels(conn: &Connection, rows: &mut [SessionRow]) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut clauses = Vec::with_capacity(rows.len());
    let mut params: Vec<Value> = Vec::with_capacity(rows.len() * 2);
    for row in rows.iter() {
        clauses.push("(r.source = ? AND r.session_id = ?)".to_string());
        params.push(Value::Text(row.source.clone()));
        params.push(Value::Text(row.session_id.clone()));
    }
    let sql = format!(
        "SELECT source, session_id, {} AS project, {} AS model, {} AS source_file
         FROM (
            SELECT r.source AS source, r.session_id AS session_id,
                {} AS project_key,
                {} AS model_key,
                {} AS file_key
            FROM usage_records r
            WHERE {}
            GROUP BY r.source, r.session_id
         )",
        unwrap_latest_key_sql("project_key"),
        unwrap_latest_key_sql("model_key"),
        unwrap_latest_key_sql("file_key"),
        latest_nonempty_key_sql("project"),
        latest_nonempty_key_sql("model"),
        latest_nonempty_key_sql("source_file"),
        clauses.join(" OR "),
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let labels = stmt
        .query_map(params_from_iter(params.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for (source, session_id, project, model, source_file) in labels {
        if let Some(row) = rows
            .iter_mut()
            .find(|row| row.source == source && row.session_id == session_id)
        {
            row.project = project;
            row.model = model;
            row.source_file = source_file;
        }
    }
    Ok(())
}

/// 一次扫描取出「最晚非空」键：`MAX(occurred_at || sep || value)` 与
/// `ORDER BY occurred_at DESC, value DESC LIMIT 1` 同序。
fn latest_nonempty_key_sql(column: &str) -> String {
    format!("MAX(CASE WHEN r.{column} != '' THEN r.occurred_at || char(31) || r.{column} END)")
}

fn unwrap_latest_key_sql(alias: &str) -> String {
    format!("COALESCE(substr({alias}, instr({alias}, char(31)) + 1), '')")
}

/// 预聚合表版的会话汇总，输出列与 `session_rollup_sql` 完全一致，
/// 所以外层的搜索 / 排序 / 分页原样复用，不必为两条路径各写一遍。
///
/// project / model 的键用 `last_at` 现拼：组内 `last_at` 就是该组 `occurred_at` 的最大值，
/// 与逐行版 `MAX(occurred_at || sep || value)` 选出同一个值。`file_key` 建表时已按这个
/// 形式存好，直接 MAX 即可。
fn rollup_session_rollup_sql(clauses: &[String], include_cost: bool) -> String {
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
            FROM usage_rollup d
            {joins}
            {}
            GROUP BY d.source, d.session_id
         )",
        where_sql(clauses),
    )
}

fn session_rollup_sql(clauses: &[String], include_cost: bool) -> String {
    let project_key = latest_nonempty_key_sql("project");
    let model_key = latest_nonempty_key_sql("model");
    let file_key = latest_nonempty_key_sql("source_file");
    let project = unwrap_latest_key_sql("project_key");
    let model = unwrap_latest_key_sql("model_key");
    let source_file = unwrap_latest_key_sql("file_key");
    let (cost_select, joins) = if include_cost {
        (
            format!(
                "SUM({COST_EXPR}) AS cost, COALESCE(SUM({UNPRICED_EXPR}), 0) AS unpriced_count"
            ),
            PRICE_JOINS,
        )
    } else {
        (
            "CAST(NULL AS REAL) AS cost, 0 AS unpriced_count".to_string(),
            "",
        )
    };
    format!(
        "SELECT source, session_id, total_tokens, started_at, ended_at,
            {project} AS project,
            {model} AS model,
            {source_file} AS source_file,
            cost, unpriced_count
         FROM (
            SELECT r.source AS source, r.session_id AS session_id,
                SUM(r.total_tokens) AS total_tokens,
                MIN(r.occurred_at) AS started_at,
                MAX(r.occurred_at) AS ended_at,
                {project_key} AS project_key,
                {model_key} AS model_key,
                {file_key} AS file_key,
                {cost_select}
            FROM usage_records r
            {joins}
            {}
            GROUP BY r.source, r.session_id
         )",
        where_sql(clauses),
    )
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
    // 会话汇总：无时间窗走预聚合；带时间窗（含 hybrid）走原始表，外层搜索/排序/分页无感。
    let use_rollup = matches!(rollup_mode(conn, &query.filter), RollupMode::Rollup);
    let (clauses, mut params) = if use_rollup {
        rollup_filter_clauses(&query.filter, None)
    } else {
        filter_clauses(&query.filter)
    };
    let sessions_cte = if use_rollup {
        rollup_session_rollup_sql(&clauses, include_cost)
    } else {
        session_rollup_sql(&clauses, include_cost)
    };

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

/// 单日工作时间线：按会话在 SQL 里聚成区间，WHERE 用 ISO 前缀范围走索引。
/// 宽口径覆盖 `day` 前后各一天（本地时区 ±1），精确裁剪交给 `assemble`。
pub fn work_timeline(conn: &Connection, day: &str) -> Result<WorkTimelineDto, String> {
    let Some((from, to)) = crate::work_timeline::broad_date_bounds(day) else {
        return Ok(WorkTimelineDto::empty(day));
    };
    let Some((day_start, day_end)) = crate::work_timeline::local_day_sql_bounds(day) else {
        return Ok(WorkTimelineDto::empty(day));
    };
    let to_end = billing_window::iso_day_end(&to);
    let project_key = latest_nonempty_key_sql("project");
    let model_key = latest_nonempty_key_sql("model");
    let sql = format!(
        "SELECT
            r.source,
            r.session_id,
            MIN(r.occurred_at),
            MAX(r.occurred_at),
            {project_key},
            {model_key},
            COALESCE(SUM(CASE WHEN r.occurred_at >= ?3 AND r.occurred_at < ?4 THEN r.total_tokens ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN r.occurred_at >= ?3 AND r.occurred_at < ?4 THEN 1 ELSE 0 END), 0)
        FROM usage_records r
        WHERE r.occurred_at >= ?1 AND r.occurred_at < ?2
        GROUP BY r.source, r.session_id"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![from, to_end, day_start, day_end], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut sessions = BTreeMap::new();
    for row in rows {
        let (source, session_id, start_at, end_at, project_key, model_key, day_tokens, day_turns) =
            row.map_err(|e| e.to_string())?;
        let Some(start) = billing_window::parse_occurred_at(&start_at) else {
            continue;
        };
        let Some(end) = billing_window::parse_occurred_at(&end_at) else {
            continue;
        };
        let (project, project_at) = split_latest_key(project_key);
        let (model, model_at) = split_latest_key(model_key);
        sessions.insert(
            (source.clone(), session_id.clone()),
            crate::work_timeline::SessionAcc {
                source,
                session_id,
                project,
                project_at,
                model,
                model_at,
                start,
                end,
                day_tokens,
                day_turns,
            },
        );
    }
    let extra = work_session_spans(conn, &from, &to)?;
    Ok(crate::work_timeline::assemble(sessions, &extra, day))
}

fn split_latest_key(raw: Option<String>) -> (String, Option<String>) {
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return (String::new(), None);
    };
    match raw.split_once('\u{1f}') {
        Some((at, value)) => (value.to_string(), Some(at.to_string())),
        None => (String::new(), None),
    }
}

/// 宽口径拉取与 `[from, to]` 日期串有交集的 Cursor 本机会话，转成时间线补充区间。
/// `first_seen_at` / `last_seen_at` 缺一则无法画条，直接跳过。
pub(crate) fn work_session_spans(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<WorkSessionSpan>, String> {
    let to_end = billing_window::iso_day_end(to);
    let mut stmt = conn
        .prepare(
            r#"
            SELECT session_id, project, models_json, first_seen_at, last_seen_at
            FROM cursor_sessions
            WHERE first_seen_at IS NOT NULL AND first_seen_at != ''
              AND last_seen_at IS NOT NULL AND last_seen_at != ''
              AND first_seen_at < ?2
              AND last_seen_at >= ?1
            "#,
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![from, to_end], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut by_session: BTreeMap<String, WorkSessionSpan> = BTreeMap::new();
    for row in rows {
        let (session_id, project, models_json, first_seen_at, last_seen_at) =
            row.map_err(|e| e.to_string())?;
        let span = WorkSessionSpan {
            source: Source::CursorAgent.as_str().to_string(),
            session_id: session_id.clone(),
            project,
            model: last_model_from_json(&models_json),
            started_at: first_seen_at,
            ended_at: last_seen_at,
        };
        match by_session.get_mut(&session_id) {
            Some(existing) => {
                if span.started_at < existing.started_at {
                    existing.started_at = span.started_at;
                }
                if span.ended_at > existing.ended_at {
                    existing.ended_at = span.ended_at;
                    if !span.model.is_empty() {
                        existing.model = span.model;
                    }
                }
                if !span.project.is_empty() {
                    existing.project = span.project;
                }
            }
            None => {
                by_session.insert(session_id, span);
            }
        }
    }
    Ok(by_session.into_values().collect())
}

fn last_model_from_json(raw: &str) -> String {
    serde_json::from_str::<Vec<String>>(raw)
        .ok()
        .and_then(|models| models.into_iter().next_back())
        .unwrap_or_default()
}

/// 取某列的全部不同值（升序），用递归 CTE 做 loose index scan。
///
/// `SELECT DISTINCT col` 即便有索引，SQLite 也是把整条索引从头扫到尾：348 万行的库上
/// 实测 2.1s，只为取回 26 个值，而且随行数线性变慢。递归写法每步「跳到下一个更大的值」，
/// 索引查找次数等于不同值的个数——同一个库上 6ms，且几乎不随数据量增长。
///
/// 结果天然升序（种子取 MIN，之后每次取严格更大的 MIN），不必再排序。
/// `column` 只接受下面几个写死的列名，不来自外部输入。
fn distinct_values(
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

pub fn filter_options(conn: &Connection) -> Result<FilterOptions, String> {
    Ok(FilterOptions {
        // source 不过滤空串：它是枚举落库，不该有空值，真出现了也要能在筛选里看见。
        sources: distinct_values(conn, "source", false)?,
        models: distinct_values(conn, "model", true)?,
        projects: distinct_values(conn, "project", true)?,
        providers: distinct_values(conn, "provider", true)?,
    })
}

pub fn recent_projects(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT project FROM usage_records
             WHERE project != ''
             GROUP BY project
             ORDER BY MAX(occurred_at) DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn source_token_totals(conn: &Connection) -> Result<InstructionUsageSummary, String> {
    let mut stmt = conn
        .prepare(
            "SELECT source, SUM(total_tokens) FROM usage_records
             GROUP BY source
             ORDER BY SUM(total_tokens) DESC, source ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(InstructionSourceUsage {
                source: row.get(0)?,
                total_tokens: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(InstructionUsageSummary {
        sources: rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?,
    })
}
