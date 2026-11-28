//! 读 VSCode 系编辑器的 `globalStorage/state.vscdb`（`ItemTable` 键值表）。
//!
//! Cursor 和 Antigravity 都是 VSCode fork，登录态都明文写在这张表里。
//! 库可能有几百 MB 且编辑器常驻占用，所以原地只读打开：WAL 下只读连接不阻塞写者。
//! 不用 `immutable=1` 之类的降级——它跳过 WAL，会读到陈旧值。

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

const STATE_DB: &str = "state.vscdb";

/// Win `%APPDATA%\<app>\...`、mac `~/Library/Application Support/<app>\...`、
/// Linux `~/.config/<app>/...` —— 三个平台都正好落在 `dirs::config_dir()` 下。
pub fn global_storage_dir(app_dir: &str) -> Option<PathBuf> {
    dirs::config_dir().map(|dir| global_storage_dir_in(&dir, app_dir))
}

pub fn global_storage_dir_in(config_dir: &Path, app_dir: &str) -> PathBuf {
    config_dir.join(app_dir).join("User").join("globalStorage")
}

pub fn open_read_only(global_storage: &Path) -> Result<Option<Connection>, String> {
    let db = global_storage.join(STATE_DB);
    if !db.exists() {
        return Ok(None);
    }
    Connection::open_with_flags(
        &db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map(Some)
    .map_err(|error| format!("打开 {} 失败：{error}", db.display()))
}

/// value 列在不同版本里可能是 TEXT 也可能是 BLOB，两种都要认。
pub fn read_item(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM ItemTable WHERE key = ?1", [key], |row| {
        row.get::<_, rusqlite::types::Value>(0)
    })
    .ok()
    .and_then(|value| match value {
        rusqlite::types::Value::Text(text) => Some(text),
        rusqlite::types::Value::Blob(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        _ => None,
    })
    .map(|value| value.trim().to_string())
}
