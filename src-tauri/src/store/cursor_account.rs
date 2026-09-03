use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::CursorUsageEvent;

/// 按指纹去重写入 Cursor 账号用量事件，返回新插入的行数。
pub fn upsert_cursor_account_events(
    conn: &Connection,
    events: &[CursorUsageEvent],
) -> Result<u64, String> {
    let mut stmt = conn
        .prepare(
            r#"
            INSERT OR IGNORE INTO cursor_account_usage (
                fingerprint, occurred_at, model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                is_headless
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
            "#,
        )
        .map_err(|e| e.to_string())?;
    let mut written = 0u64;
    for event in events {
        let changed = stmt
            .execute(params![
                event.fingerprint(),
                event.occurred_at,
                event.model,
                event.input_tokens,
                event.output_tokens,
                event.cache_read_tokens,
                event.cache_creation_tokens,
                i64::from(event.is_headless),
            ])
            .map_err(|e| e.to_string())?;
        written += changed as u64;
    }
    Ok(written)
}

pub fn load_cursor_account_events(conn: &Connection) -> Result<Vec<CursorUsageEvent>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT occurred_at, model, input_tokens, output_tokens,
                   cache_read_tokens, cache_creation_tokens, is_headless
            FROM cursor_account_usage
            ORDER BY occurred_at ASC
            "#,
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CursorUsageEvent {
                occurred_at: row.get(0)?,
                model: row.get(1)?,
                input_tokens: row.get(2)?,
                output_tokens: row.get(3)?,
                cache_read_tokens: row.get(4)?,
                cache_creation_tokens: row.get(5)?,
                is_headless: row.get::<_, i64>(6)? != 0,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn cursor_account_events_page(
    conn: &Connection,
    page: u32,
    page_size: u32,
    sort_dir: &str,
) -> Result<(u32, Vec<crate::domain::CursorUsageEvent>), String> {
    let total: u32 = conn
        .query_row("SELECT COUNT(*) FROM cursor_account_usage", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    let dir = if sort_dir.eq_ignore_ascii_case("asc") {
        "ASC"
    } else {
        "DESC"
    };
    let offset = (page.saturating_sub(1) as i64) * page_size as i64;
    let sql = format!(
        r#"
        SELECT occurred_at, model, input_tokens, output_tokens,
               cache_read_tokens, cache_creation_tokens, is_headless
        FROM cursor_account_usage
        ORDER BY occurred_at {dir}, model ASC
        LIMIT ?1 OFFSET ?2
        "#
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![page_size as i64, offset], |row| {
            Ok(crate::domain::CursorUsageEvent {
                occurred_at: row.get(0)?,
                model: row.get(1)?,
                input_tokens: row.get(2)?,
                output_tokens: row.get(3)?,
                cache_read_tokens: row.get(4)?,
                cache_creation_tokens: row.get(5)?,
                is_headless: row.get::<_, i64>(6)? != 0,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok((total, rows))
}

pub fn set_cursor_account_as_of(conn: &Connection, as_of: &str) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO cursor_account_meta(key, value) VALUES('as_of', ?1)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
        params![as_of],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn cursor_account_as_of(conn: &Connection) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM cursor_account_meta WHERE key = 'as_of'",
        [],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn max_cursor_account_occurred_ms(conn: &Connection) -> Result<Option<i64>, String> {
    let occurred_at: Option<String> = conn
        .query_row(
            "SELECT MAX(occurred_at) FROM cursor_account_usage",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let Some(occurred_at) = occurred_at else {
        return Ok(None);
    };
    let millis = chrono::DateTime::parse_from_rfc3339(&occurred_at)
        .map_err(|e| format!("Cursor 账号用量时间戳无法解析：{e}"))?
        .timestamp_millis();
    Ok(Some(millis))
}

pub fn clear_cursor_account_usage(conn: &Connection) -> Result<(), String> {
    conn.execute("DELETE FROM cursor_account_usage", [])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM cursor_account_meta", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}
