use std::collections::BTreeSet;

use rusqlite::{params, Connection};

pub fn rollup_is_ready(conn: &Connection) -> bool {
    conn.query_row("SELECT ready FROM rollup_state WHERE id = 1", [], |row| {
        row.get::<_, i64>(0)
    })
    .map(|ready| ready != 0)
    .unwrap_or(false)
}

/// 需要补建吗——原始表有数据而预聚合表还没就绪。
pub fn rollup_needs_backfill(conn: &Connection) -> Result<bool, String> {
    if rollup_is_ready(conn) {
        return Ok(false);
    }
    conn.query_row("SELECT EXISTS(SELECT 1 FROM usage_records)", [], |row| {
        row.get(0)
    })
    .map_err(|e| e.to_string())
}

/// 整表补建并置为就绪。
///
/// 老库第一次升到带 `usage_rollup` 的版本、或从不含该表的旧备份恢复时都要跑一次。
/// 350 万行要十几秒，所以调用方应当放到后台——补建期间 `rollup_is_ready` 为假，
/// 查询会自动回退原始表，慢一点但数字是对的。
pub fn backfill_rollup(conn: &Connection) -> Result<u64, String> {
    let written = rebuild_rollup(conn)?;
    conn.execute("UPDATE rollup_state SET ready = 1 WHERE id = 1", [])
        .map_err(|e| e.to_string())?;
    Ok(written)
}

/// 某个源文件的记录落在哪些 UTC 日期上。
///
/// 摄取要替换一个文件时，得先问清它原来占了哪几天——那几天的预聚合行在记录删掉后就失效了。
/// 走 `idx_usage_source_file`，几千个文件的库上也是索引查找。
pub fn days_for_file(conn: &Connection, source_file: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT substr(occurred_at, 1, 10) FROM usage_records WHERE source_file = ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![source_file], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// 只重算指定几天的预聚合行。
///
/// 全量重建在 350 万行的库上要十几秒，而一次摄取通常只动到今天这一两天。按���重算把
/// 这份开销压到跟改动量成正比，摄取才不会随历史数据一起变慢。
///
/// 用 `occurred_at >= day AND occurred_at < day+1` 而不是 `substr(...) = day`：
/// 前者能走 `idx_usage_occurred`，后者对每行调函数，退化成全表扫描。
/// 日期边界用字符串比较即可——`occurred_at` 是 ISO 8601，字典序就是时间序；
/// 上界取 `day` 后缀 `~`（ASCII 126）是因为同一天的时间戳第 11 位只会是 `T`（84），
/// 一定小于 `~`，而下一天的日期部分已经变大，落不进这个区间。
pub fn rebuild_rollup_days(conn: &Connection, days: &BTreeSet<String>) -> Result<(), String> {
    for day in days {
        conn.execute("DELETE FROM usage_rollup WHERE day = ?1", params![day])
            .map_err(|e| e.to_string())?;
        // 空 day 对应 occurred_at 本身为空的脏数据，范围比较框不住，单独按 substr 兜。
        let (predicate, bounds): (&str, Vec<String>) = if day.is_empty() {
            ("substr(r.occurred_at, 1, 10) = ''", Vec::new())
        } else {
            (
                "r.occurred_at >= ?1 AND r.occurred_at < ?2",
                vec![day.clone(), format!("{day}~")],
            )
        };
        let sql = format!(
            r#"
            INSERT INTO usage_rollup (
                day, source, model, provider, project, session_id, has_native,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                reasoning_tokens, total_tokens, native_cost, record_count,
                first_at, last_at, file_key
            )
            SELECT
                substr(r.occurred_at, 1, 10),
                r.source, r.model, r.provider, r.project, r.session_id,
                CASE WHEN r.native_cost IS NOT NULL THEN 1 ELSE 0 END,
                SUM(r.input_tokens), SUM(r.output_tokens), SUM(r.cache_read_tokens),
                SUM(r.cache_creation_tokens), SUM(r.reasoning_tokens), SUM(r.total_tokens),
                COALESCE(SUM(r.native_cost), 0),
                COUNT(*),
                MIN(r.occurred_at), MAX(r.occurred_at),
                MAX(CASE WHEN r.source_file != '' THEN r.occurred_at || char(31) || r.source_file END)
            FROM usage_records r
            WHERE {predicate}
            GROUP BY 1, 2, 3, 4, 5, 6, 7
            "#
        );
        conn.execute(&sql, rusqlite::params_from_iter(bounds.iter()))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 从 `usage_records` 整表重建 `usage_rollup`。
///
/// 刻意做成全量重建而不是增量维护：预聚合表一旦和原始表对不上，界面会显示错误数字
/// 且很难察觉，而增量维护的边界（删文件、改文件、跨天会话）正是最容易漏的地方。
/// 实测 17 万行重建约 0.2s，摄取本身要扫几千个文件，这点开销可以忽略；调用方只在
/// 真有记录写入或删除时才调，缓存全命中的摄取不会触发。
pub fn rebuild_rollup(conn: &Connection) -> Result<u64, String> {
    conn.execute("DELETE FROM usage_rollup", [])
        .map_err(|e| e.to_string())?;
    let written = conn
        .execute(
            r#"
            INSERT INTO usage_rollup (
                day, source, model, provider, project, session_id, has_native,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                reasoning_tokens, total_tokens, native_cost, record_count,
                first_at, last_at, file_key
            )
            SELECT
                substr(r.occurred_at, 1, 10),
                r.source, r.model, r.provider, r.project, r.session_id,
                CASE WHEN r.native_cost IS NOT NULL THEN 1 ELSE 0 END,
                SUM(r.input_tokens), SUM(r.output_tokens), SUM(r.cache_read_tokens),
                SUM(r.cache_creation_tokens), SUM(r.reasoning_tokens), SUM(r.total_tokens),
                COALESCE(SUM(r.native_cost), 0),
                COUNT(*),
                MIN(r.occurred_at), MAX(r.occurred_at),
                MAX(CASE WHEN r.source_file != '' THEN r.occurred_at || char(31) || r.source_file END)
            FROM usage_records r
            GROUP BY 1, 2, 3, 4, 5, 6, 7
            "#,
            [],
        )
        .map_err(|e| e.to_string())?;
    Ok(written as u64)
}
