//! 正文倒排（FTS5）的形态、触发器与持续维护（ADR 0014、0024）。

use rusqlite::{params, Connection, OptionalExtension};

use super::FTS_REBUILD_VERSION;

/// `detail=none` 只记「哪一行含这个三元组」，不记它出现在什么位置。位置表在这里是纯开销：
/// 检索侧不用 `bm25()`/`snippet()`，排序键是手写的 0/1，片段由 Rust 从正文切。106 万事件、
/// 400MB 正文的真实库实测倒排从 2732MB 降到 294MB，查询还快一倍。代价是 FTS5 不再接受短语
/// 查询，调用方要自己把关键词切成三元组用 AND 连接、再回表用 LIKE 剔假阳性，
/// 见 `conversation::catalog_search`。
pub(super) const CONVERSATION_FTS_DEFINITION: &str = r#"
    text,
    name,
    content='conversation_events',
    content_rowid='rowid',
    tokenize='trigram',
    detail='none',
    columnsize=0
"#;

/// 更新触发器只看 `text` / `name`：换代时 `finalize_session_events` 逐行重排 `sequence`，
/// 不收窄的话每条事件都要在倒排里白删白插一次。删除计数供 `conversation_fts_needs_optimize`
/// 判断该不该整份合并——外置 content 的删除只是追加删除标记，不合并就一直占着空间。
pub(super) const CONVERSATION_FTS_TRIGGERS: &str = r#"
CREATE TRIGGER IF NOT EXISTS conversation_events_ai AFTER INSERT ON conversation_events BEGIN
    INSERT INTO conversation_events_fts(rowid, text, name)
    VALUES (new.rowid, COALESCE(new.text, ''), COALESCE(new.name, ''));
END;
CREATE TRIGGER IF NOT EXISTS conversation_events_ad AFTER DELETE ON conversation_events BEGIN
    INSERT INTO conversation_events_fts(conversation_events_fts, rowid, text, name)
    VALUES ('delete', old.rowid, COALESCE(old.text, ''), COALESCE(old.name, ''));
    UPDATE conversation_fts_maintenance
    SET deleted_since_optimize = deleted_since_optimize + 1;
END;
CREATE TRIGGER IF NOT EXISTS conversation_events_au
AFTER UPDATE OF text, name ON conversation_events BEGIN
    INSERT INTO conversation_events_fts(conversation_events_fts, rowid, text, name)
    VALUES ('delete', old.rowid, COALESCE(old.text, ''), COALESCE(old.name, ''));
    INSERT INTO conversation_events_fts(rowid, text, name)
    VALUES (new.rowid, COALESCE(new.text, ''), COALESCE(new.name, ''));
    UPDATE conversation_fts_maintenance
    SET deleted_since_optimize = deleted_since_optimize + 1;
END;
"#;

/// 每轮摄取后小步合并的工作量上限（FTS5 `merge` 的页数）。
pub(crate) const FTS_MERGE_PAGES: i64 = 500;

/// 全量合并的兜底门槛：自上次合并以来删掉的事件数达到上次行数的一半，且不少于这么多。
const OPTIMIZE_MIN_DELETED: i64 = 100_000;

/// 重灌倒排在真实库上腾出约 500MB、占整库两成多；门槛再高这些页就一直挂在 freelist 里。
const VACUUM_MIN_FREE_BYTES: i64 = 256 * 1024 * 1024;

fn table_exists(conn: &Connection, name: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![name],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

fn conversation_fts_sql(conn: &Connection) -> Result<Option<String>, String> {
    sqlite_master_sql(conn, "table", "conversation_events_fts")
}

fn sqlite_master_sql(conn: &Connection, kind: &str, name: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = ?1 AND name = ?2",
        params![kind, name],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// 旧库建的是 FTS5 默认的 `detail=full`。判据就看建表语句里有没有写 `detail=`——
/// 正文索引只有这一次形态变更，不值得为它再开一张版本表。
pub(crate) fn conversation_fts_needs_migration(conn: &Connection) -> Result<bool, String> {
    Ok(conversation_fts_sql(conn)?.is_some_and(|sql| !sql.contains("detail=")))
}

/// 换掉正文索引的形态。整份倒排要从头灌一遍，百万行量级要几十秒，所以调用方必须放到
/// 后台线程（见 `lib.rs::spawn_conversation_cache_migration`）。迁移期间旧表继续服务查询：
/// `catalog_search` 发的三元组 AND 查询在两种形态上召回一致。
pub(crate) fn migrate_conversation_events_fts(conn: &Connection) -> Result<(), String> {
    if !conversation_fts_needs_migration(conn)? {
        return Ok(());
    }
    let migration = conn.execute_batch(&format!(
        r#"
        SAVEPOINT migrate_conversation_events_fts;
        DROP TABLE conversation_events_fts;
        CREATE VIRTUAL TABLE conversation_events_fts USING fts5({CONVERSATION_FTS_DEFINITION});
        INSERT INTO conversation_events_fts(conversation_events_fts) VALUES('rebuild');
        RELEASE migrate_conversation_events_fts;
        "#
    ));
    if let Err(error) = migration {
        let _ = conn.execute_batch(
            "ROLLBACK TO migrate_conversation_events_fts; RELEASE migrate_conversation_events_fts;",
        );
        return Err(error.to_string());
    }
    Ok(())
}

/// `rows_at_optimize` 为空表示从没整份合并过：老库升级、从备份恢复的库都落在这里，
/// 由维护线程补做一次，不另写一次性迁移。
fn ensure_maintenance_table(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS conversation_fts_maintenance (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            deleted_since_optimize INTEGER NOT NULL DEFAULT 0,
            rows_at_optimize INTEGER,
            index_bytes INTEGER
        );
        INSERT OR IGNORE INTO conversation_fts_maintenance (id) VALUES (1);
        "#,
    )
    .map_err(|e| e.to_string())
}

/// 老库的触发器是改任何列都重写倒排、也不计删除数的版本。`CREATE TRIGGER IF NOT EXISTS`
/// 对同名触发器是空操作，只能先删再建。
fn triggers_are_current(conn: &Connection) -> Result<bool, String> {
    let update = sqlite_master_sql(conn, "trigger", "conversation_events_au")?;
    let delete = sqlite_master_sql(conn, "trigger", "conversation_events_ad")?;
    Ok(update.is_some_and(|sql| sql.contains("UPDATE OF"))
        && delete.is_some_and(|sql| sql.contains("conversation_fts_maintenance")))
}

/// 正文全文索引是 `conversation_events` 的派生缓存：源文件仍是权威，重建事件表后可再 rebuild。
/// trigram 按子串匹配，对应原先目录 LIKE 的「关键字」预期；短于 3 个字符的查询只走标题。
///
/// 这里只负责「没有就建」和换触发器。已存在的旧形态表不在这条路径上换——那要几十秒，
/// 不能挡启动。
pub(super) fn ensure_conversation_events_fts(conn: &Connection) -> Result<(), String> {
    ensure_maintenance_table(conn)?;
    let existed = table_exists(conn, "conversation_events_fts")?;
    if existed && !triggers_are_current(conn)? {
        conn.execute_batch(
            r#"
            DROP TRIGGER IF EXISTS conversation_events_ad;
            DROP TRIGGER IF EXISTS conversation_events_au;
            "#,
        )
        .map_err(|e| e.to_string())?;
    }
    conn.execute_batch(&format!(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS conversation_events_fts USING fts5({CONVERSATION_FTS_DEFINITION});
        {CONVERSATION_FTS_TRIGGERS}
        "#
    ))
    .map_err(|e| e.to_string())?;
    if existed {
        return Ok(());
    }
    let has_events: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation_events LIMIT 1)",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if has_events {
        conn.execute(
            "INSERT INTO conversation_events_fts(conversation_events_fts) VALUES('rebuild')",
            [],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 有上限的增量合并，耗时与库大小无关，可以挂在每轮摄取之后。
pub(crate) fn merge_conversation_fts_step(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "INSERT INTO conversation_events_fts(conversation_events_fts, rank) VALUES('merge', ?1)",
        params![FTS_MERGE_PAGES],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// 只读一行维护记录，每轮摄取后都可以问。旧形态倒排交给形态迁移整份重灌，这里不插手。
pub(crate) fn conversation_fts_needs_optimize(conn: &Connection) -> Result<bool, String> {
    if conversation_fts_needs_migration(conn)? {
        return Ok(false);
    }
    let row = conn
        .query_row(
            "SELECT deleted_since_optimize, rows_at_optimize FROM conversation_fts_maintenance WHERE id = 1",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(match row {
        None | Some((_, None)) => true,
        Some((deleted, Some(rows))) => deleted >= (rows / 2).max(OPTIMIZE_MIN_DELETED),
    })
}

/// 从事件表整份重灌倒排。不用 `optimize`：真实库上它合并完仍有 737MB，重灌只要 308MB，
/// 而且重灌约 1 分钟、`optimize` 约 3 分钟。调用方必须在后台线程里拿写锁做。
pub(crate) fn rebuild_conversation_fts(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "INSERT INTO conversation_events_fts(conversation_events_fts) VALUES('rebuild')",
        [],
    )
    .map_err(|e| e.to_string())?;
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM conversation_events", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    conn.execute(
        r#"
        UPDATE conversation_fts_maintenance
        SET deleted_since_optimize = 0, rows_at_optimize = ?1
        WHERE id = 1
        "#,
        params![rows],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// 维护方式从 `optimize` 换成整份重灌之前，老库已经记过基准行数，删除量要很久才到门槛。
/// 清掉基准，让维护线程尽快重灌一次；`user_version` 记账，只做一次。
pub(super) fn migrate_fts_maintenance_to_rebuild(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if version >= FTS_REBUILD_VERSION {
        return Ok(());
    }
    conn.execute_batch(&format!(
        "UPDATE conversation_fts_maintenance SET rows_at_optimize = NULL WHERE id = 1;
         PRAGMA user_version = {FTS_REBUILD_VERSION};"
    ))
    .map_err(|e| e.to_string())
}

pub(crate) fn vacuum_is_due(page_size: i64, page_count: i64, freelist_count: i64) -> bool {
    freelist_count.saturating_mul(page_size) >= VACUUM_MIN_FREE_BYTES
        && freelist_count.saturating_mul(10) >= page_count.saturating_mul(2)
}

/// `auto_vacuum` 是关的，合并腾出的页只进 freelist；攒够了才值得整库重写一次。
pub(crate) fn database_vacuum_is_due(conn: &Connection) -> Result<bool, String> {
    let pragma = |name: &str| -> Result<i64, String> {
        conn.pragma_query_value(None, name, |row| row.get(0))
            .map_err(|e| e.to_string())
    };
    Ok(vacuum_is_due(
        pragma("page_size")?,
        pragma("page_count")?,
        pragma("freelist_count")?,
    ))
}

/// 实测对话索引在磁盘上的占用。`dbstat` 要把相关页全读一遍，大库冷缓存下要几十秒，
/// 只能在后台线程里量，量完用 `store_conversation_index_bytes` 记下来给界面读。
pub(crate) fn measure_conversation_index_bytes(conn: &Connection) -> u64 {
    if let Ok(bytes) = conn.query_row(
        "SELECT COALESCE(SUM(pgsize), 0) FROM dbstat
         WHERE name GLOB 'conversation_events*'
            OR name IN ('conversation_files', 'conversation_session_tools')",
        [],
        |row| row.get::<_, i64>(0),
    ) {
        return bytes.max(0) as u64;
    }
    conn.query_row(
        "SELECT COALESCE(SUM(LENGTH(COALESCE(text, '')) + LENGTH(COALESCE(name, ''))), 0)
         FROM conversation_events",
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
    .max(0) as u64
}

pub(crate) fn store_conversation_index_bytes(conn: &Connection, bytes: u64) -> Result<(), String> {
    conn.execute(
        "UPDATE conversation_fts_maintenance SET index_bytes = ?1 WHERE id = 1",
        params![bytes as i64],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// 上次实测的占用；还没量过时为 0，界面据此不显示这一项。
pub(crate) fn stored_conversation_index_bytes(conn: &Connection) -> u64 {
    conn.query_row(
        "SELECT index_bytes FROM conversation_fts_maintenance WHERE id = 1",
        [],
        |row| row.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
    .unwrap_or(0)
    .max(0) as u64
}
