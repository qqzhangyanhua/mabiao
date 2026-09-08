//! 会话行注水：SQL 查出来的一行只有会话自己的字段，其余四样都挂在别处。
//!
//! 目录页、正文搜索、单条详情走的都是这一条 `sessions`，注水顺序与口径只在这里定一次。
//! 谁去哪儿取（消耗记录汇总、Cursor 会话补模型、工作纪要打标）是实现细节，调用方不必知道。

use rusqlite::Connection;

use crate::domain::{ConversationSessionRow, PriceTable};
use crate::query;

use super::cursor_bridge::hydrate_cursor_hash_models;
use super::session_store::load_session_files;

/// 给会话行补齐展示需要的一切。
///
/// `prices` 为 `None` 表示调用方手上没有价目表（单条详情就是这样），此时跳过消耗记录汇总，
/// 行上的 token 与费用保持 SQL 里的占位值。
pub(super) fn sessions(
    conn: &Connection,
    prices: Option<&PriceTable>,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    for row in rows.iter_mut() {
        let paths = load_session_files(conn, &row.source, &row.session_id)?;
        if !paths.is_empty() {
            row.source_files = paths
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect();
        }
    }
    if let Some(prices) = prices {
        hydrate_usage(conn, prices, rows)?;
    }
    hydrate_cursor_hash_models(conn, rows)?;
    crate::work_notes::decorate_sessions(conn, rows)
}

fn hydrate_usage(
    conn: &Connection,
    prices: &PriceTable,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    let keys = rows
        .iter()
        .map(|row| (row.source.clone(), row.session_id.clone()))
        .collect::<Vec<_>>();
    let totals = query::usage_rollups_for_sessions(conn, prices, &keys)?;
    for row in rows {
        let Some(usage) = totals.get(&(row.source.clone(), row.session_id.clone())) else {
            continue;
        };
        row.total_tokens = usage.total_tokens;
        row.cost = usage.cost;
        row.unpriced = usage.unpriced;
    }
    Ok(())
}
