use rusqlite::{Connection, OpenFlags};

use super::schema::init_schema;

const WAL_SIZE_LIMIT_BYTES: i64 = 32 * 1024 * 1024;

pub fn open_db(path: &str) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    configure_connection(&conn)?;
    init_schema(&conn)?;
    Ok(conn)
}

pub fn open_memory() -> Result<Connection, String> {
    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    init_schema(&conn)?;
    Ok(conn)
}

/// 只读连接。摄取会长时间占着写连接和写事务；查询必须走另一条连接，才能用上 WAL
/// 的「读者不阻塞未提交写者」。这里不能跑 `init_schema` / `journal_mode`，那些是写操作。
pub fn open_readonly(path: &str) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())
}

/// 只对真实文件落盘的连接生效：`:memory:` 数据库本来就没有并发读写者，WAL/NORMAL 这两个
/// pragma 在内存模式下会被 SQLite 静默忽略甚至报错，所以不对 `open_memory` 调用。
///
/// - `journal_mode=WAL`：托盘后台线程每隔几分钟跑一次完整 ingest，会长时间持有写事务；
///   WAL 让前端查询（读者）不必等这次写事务提交就能读到旧版本页，避免 UI 卡顿。
/// - `synchronous=NORMAL`：WAL 模式下官方推荐搭配 NORMAL，牺牲的持久性仅在系统级崩溃
///   （断电/内核崩溃，而非应用崩溃）时才可能丢最后几条已提交事务，可接受，换来显著更少的 fsync。
/// - `journal_size_limit`：整轮摄取是一个大事务，「重建全部」会把整库的页都写进 WAL；
///   不设上限的话 checkpoint 之后 WAL 文件仍按峰值大小常驻磁盘。
fn configure_connection(conn: &Connection) -> Result<(), String> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "journal_size_limit", WAL_SIZE_LIMIT_BYTES)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 把 freelist 里的页还给文件系统。`auto_vacuum` 是关的——它必须在建表之前就定下来，
/// 老库改不了——所以批量释放页之后只能靠这一次全量重写。整库重写不便宜，只在确实腾出
/// GB 级页面之后才值得调用。
pub fn vacuum(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("VACUUM").map_err(|e| e.to_string())
}

/// 把 SQLite 页缓存和临时分配还给系统。mimalloc 管不到 libc malloc 上的这些页。
pub fn shrink_memory(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("PRAGMA shrink_memory")
        .map_err(|e| e.to_string())
}
