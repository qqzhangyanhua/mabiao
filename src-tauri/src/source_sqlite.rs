//! 第三方客户端 SQLite 只读访问基底。
//!
//! 统一封装两类只读打开语义、WAL 侧车指纹与修订号（只看主库与 `-wal`，永不算 `-shm`）、
//! 表/列自省及动态列投影/读取，供 Usage 与 Conversation 适配器共用。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, Row};

/// 只读打开并跟随 WAL：`file:…?mode=ro` + `SQLITE_OPEN_URI` + `query_only = true`。
///
/// 不用 `immutable=1`——它会跳过未 checkpoint 的 WAL 页，正在写入的会话会静默读到旧数据。
pub(crate) fn open_wal_readonly(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(
        sqlite_file_uri(path, "mode=ro"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

/// 纯只读 flags 打开：`SQLITE_OPEN_READ_ONLY` + `query_only = true`。
///
/// 保留给 OpenCode 与 Cursor 代码量等原有普通只读语义的来源，不改动其打开方式。
pub(crate) fn open_plain_readonly(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

/// SQLite URI：转义路径里的 `%`、空格、`?`、`#`，统一斜杠后再拼查询串。
pub(crate) fn sqlite_file_uri(path: &Path, query: &str) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file:");
    for ch in raw.chars() {
        match ch {
            '%' => uri.push_str("%25"),
            ' ' => uri.push_str("%20"),
            '?' => uri.push_str("%3F"),
            '#' => uri.push_str("%23"),
            c => uri.push(c),
        }
    }
    uri.push('?');
    uri.push_str(query);
    uri
}

pub(crate) fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

/// 主库旁 `-wal` 的元数据指纹（mtime+size 等），WAL 变化会让缓存失效。
///
/// 不算 `-shm`：它只是 WAL 索引，内容完全由主库与 `-wal` 决定；而每次只读打开都会
/// 刷新它的 mtime，算进来的话摄取自己的读取就会让下一轮判定为已变化，每轮整份重解析。
pub(crate) fn wal_fingerprint(path: &Path) -> String {
    crate::ingest::metadata_fingerprint(&sqlite_sidecar_path(path, "-wal"))
}

/// WAL 模式 SQLite 源的对话记录修订号：`main + -wal`（永不算 `-shm`）。
///
/// 新写入先落 `-wal`，主文件的大小与 mtime 可能不动，因此修订号必须纳入 `-wal`；
/// 不能算 `-shm`，否则每次只读打开都会刷新它导致详情读取前后修订号永不匹配。
pub(crate) fn wal_source_revision(path: &Path, label: &str) -> Result<String, String> {
    std::fs::metadata(path).map_err(|error| format!("读取 {label} 数据库元数据失败：{error}"))?;
    Ok(format!(
        "{}|{}",
        crate::ingest::metadata_fingerprint(path),
        wal_fingerprint(path),
    ))
}

pub(crate) fn table_names(conn: &Connection) -> Result<BTreeSet<String>, String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    let mut names = BTreeSet::new();
    for name in rows {
        names.insert(name.map_err(|error| error.to_string())?);
    }
    Ok(names)
}

pub(crate) fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

pub(crate) fn table_columns(conn: &Connection, table: &str) -> Result<BTreeSet<String>, String> {
    if table.is_empty()
        || !table
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(format!("不支持的表名：{table}"));
    }
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?;
    let mut names = BTreeSet::new();
    for name in rows {
        names.insert(name.map_err(|error| error.to_string())?);
    }
    Ok(names)
}

pub(crate) fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    table_columns(conn, table)
        .map(|columns| columns.contains(column))
        .unwrap_or(false)
}

pub(crate) fn sql_text(columns: &BTreeSet<String>, name: &str) -> String {
    sql_coalesce(columns, "", name, "''")
}

pub(crate) fn sql_int(columns: &BTreeSet<String>, name: &str) -> String {
    sql_coalesce(columns, "", name, "0")
}

pub(crate) fn sql_real(columns: &BTreeSet<String>, name: &str) -> String {
    sql_coalesce(columns, "", name, "0")
}

pub(crate) fn sql_nullable(columns: &BTreeSet<String>, name: &str) -> String {
    sql_aliased_nullable(columns, "", name)
}

pub(crate) fn sql_aliased_nullable(columns: &BTreeSet<String>, alias: &str, name: &str) -> String {
    if columns.contains(name) {
        if alias.is_empty() {
            name.to_string()
        } else {
            format!("{alias}.{name}")
        }
    } else {
        "NULL".to_string()
    }
}

pub(crate) fn sql_coalesce(
    columns: &BTreeSet<String>,
    alias: &str,
    name: &str,
    fallback: &str,
) -> String {
    if columns.contains(name) {
        if alias.is_empty() {
            format!("COALESCE({name}, {fallback})")
        } else {
            format!("COALESCE({alias}.{name}, {fallback})")
        }
    } else {
        fallback.to_string()
    }
}

pub(crate) fn sql_fallback(columns: &BTreeSet<String>, name: &str, fallback: &str) -> String {
    if columns.contains(name) {
        name.to_string()
    } else {
        fallback.to_string()
    }
}

pub(crate) fn row_opt_string(row: &Row<'_>, idx: usize) -> rusqlite::Result<String> {
    Ok(row.get::<_, Option<String>>(idx)?.unwrap_or_default())
}

pub(crate) fn row_blob_or_text(row: &Row<'_>, idx: usize) -> rusqlite::Result<Vec<u8>> {
    match row.get_ref(idx)? {
        ValueRef::Blob(bytes) | ValueRef::Text(bytes) => Ok(bytes.to_vec()),
        ValueRef::Null => Ok(Vec::new()),
        value => Err(rusqlite::Error::InvalidColumnType(
            idx,
            "data".to_string(),
            value.data_type(),
        )),
    }
}

pub(crate) fn row_text_lossy(row: &Row<'_>, idx: usize) -> rusqlite::Result<String> {
    match row.get_ref(idx)? {
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            Ok(String::from_utf8_lossy(bytes).into_owned())
        }
        ValueRef::Integer(n) => Ok(n.to_string()),
        ValueRef::Real(n) => Ok(n.to_string()),
        ValueRef::Null => Ok(String::new()),
    }
}
