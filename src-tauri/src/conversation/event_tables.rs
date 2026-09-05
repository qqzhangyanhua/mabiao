//! `conversation_events` 旁边的两张派生小表：源文件路径字典与会话工具汇总。
//!
//! 两者都只是为了不让百万行的事件表替它们扛体积——路径的去重基数只有几千条，
//! 「这个会话用过哪个工具」也只有几万条事实。写入侧改事件表时必须同步这两张表，
//! 所以维护逻辑集中在这里，`event_index` 只负责在正确的时机调用。

use std::collections::BTreeMap;

use rusqlite::{params, Connection};

/// 路径 → `conversation_files.file_id`。一次写入通常只涉及一两个文件，但事件是逐条插的，
/// 不缓存就是每行一次往返查询。
#[derive(Default)]
pub(super) struct FileIds {
    cache: BTreeMap<String, i64>,
}

impl FileIds {
    pub(super) fn resolve(&mut self, conn: &Connection, path: &str) -> Result<i64, String> {
        if let Some(file_id) = self.cache.get(path) {
            return Ok(*file_id);
        }
        conn.execute(
            "INSERT OR IGNORE INTO conversation_files(path) VALUES(?1)",
            params![path],
        )
        .map_err(|error| error.to_string())?;
        let file_id = conn
            .query_row(
                "SELECT file_id FROM conversation_files WHERE path = ?1",
                params![path],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())?;
        self.cache.insert(path.to_string(), file_id);
        Ok(file_id)
    }
}

/// 按 (来源, 会话, 代次) 重算工具汇总。聚合覆盖该代次的全部事件，所以追加之后重跑即可，
/// 不需要区分「这一轮新增了哪几条」。
pub(super) fn refresh_session_tools(
    conn: &Connection,
    source: &str,
    session_id: &str,
    generation: i64,
) -> Result<(), String> {
    conn.execute(
        "DELETE FROM conversation_session_tools
         WHERE source = ?1 AND session_id = ?2 AND index_generation = ?3",
        params![source, session_id, generation],
    )
    .map_err(|error| error.to_string())?;
    conn.execute(
        &crate::store::conversation_session_tools_sql(
            "source = ?1 AND session_id = ?2 AND index_generation = ?3",
        ),
        params![source, session_id, generation],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// 跟着事件行一起删。`keep_generation` 对应事件表那边「只留当前代次」的清理。
pub(super) fn clear_session_tools(
    conn: &Connection,
    source: &str,
    session_id: &str,
    keep_generation: Option<i64>,
) -> Result<(), String> {
    match keep_generation {
        Some(generation) => conn.execute(
            "DELETE FROM conversation_session_tools
             WHERE source = ?1 AND session_id = ?2 AND index_generation != ?3",
            params![source, session_id, generation],
        ),
        None => conn.execute(
            "DELETE FROM conversation_session_tools WHERE source = ?1 AND session_id = ?2",
            params![source, session_id],
        ),
    }
    .map(|_| ())
    .map_err(|error| error.to_string())
}
