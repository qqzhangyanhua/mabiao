use std::collections::BTreeSet;

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{Source, UsageRecord};

use super::ADAPTER_VERSION;

pub fn insert_records(conn: &Connection, records: &[UsageRecord]) -> Result<u64, String> {
    let mut stmt = conn
        .prepare(
            r#"
            INSERT INTO usage_records (
                occurred_at, source, model, provider, project, session_id, source_file,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                reasoning_tokens, total_tokens, native_cost
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
            "#,
        )
        .map_err(|e| e.to_string())?;
    let mut written = 0u64;
    for record in records {
        stmt.execute(params![
            record.occurred_at,
            record.source.as_str(),
            record.model.to_ascii_lowercase(),
            record.provider,
            record.project,
            record.session_id,
            record.source_file,
            record.input_tokens,
            record.output_tokens,
            record.cache_read_tokens,
            record.cache_creation_tokens,
            record.reasoning_tokens,
            record.total_tokens,
            record.native_cost,
        ])
        .map_err(|e| e.to_string())?;
        written += 1;
    }
    Ok(written)
}

pub fn record_count_for_file(conn: &Connection, source_file: &str) -> Result<u64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM usage_records WHERE source_file = ?1",
        params![source_file],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count as u64)
    .map_err(|e| e.to_string())
}

pub fn cached_adapter_version(conn: &Connection, path: &str) -> Result<Option<i64>, String> {
    conn.query_row(
        "SELECT adapter_version FROM ingested_files WHERE path = ?1",
        params![path],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn delete_records_for_file(conn: &Connection, source_file: &str) -> Result<u64, String> {
    conn.execute(
        "DELETE FROM usage_records WHERE source_file = ?1",
        params![source_file],
    )
    .map(|count| count as u64)
    .map_err(|e| e.to_string())
}

pub fn file_unchanged(
    conn: &Connection,
    path: &str,
    mtime_ms: i64,
    size: i64,
    source: Source,
    fingerprint: &str,
) -> Result<bool, String> {
    let row: Option<(i64, i64, String, String, i64)> = conn
        .query_row(
            "SELECT mtime_ms, size, source, fingerprint, adapter_version FROM ingested_files WHERE path = ?1",
            params![path],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(matches!(
        row,
        Some((m, s, cached_source, cached_fingerprint, version))
            if m == mtime_ms
                && s == size
                && cached_source == source.as_str()
                && cached_fingerprint == fingerprint
                && version == ADAPTER_VERSION
    ))
}

/// 托盘心跳用的轻量对账：一次取出比对所需字段，避免扫盘时再逐条查库。
#[derive(Debug, Clone)]
pub struct IngestedFileCacheRow {
    pub path: String,
    pub mtime_ms: i64,
    pub size: i64,
    pub source: String,
    pub fingerprint: String,
    pub adapter_version: i64,
}

pub fn cached_ingested_files(conn: &Connection) -> Result<Vec<IngestedFileCacheRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT path, mtime_ms, size, source, fingerprint, adapter_version FROM ingested_files",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(IngestedFileCacheRow {
                path: row.get(0)?,
                mtime_ms: row.get(1)?,
                size: row.get(2)?,
                source: row.get(3)?,
                fingerprint: row.get(4)?,
                adapter_version: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn mark_file(
    conn: &Connection,
    path: &str,
    mtime_ms: i64,
    size: i64,
    source: Source,
    fingerprint: &str,
) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO ingested_files(path, mtime_ms, size, source, fingerprint, adapter_version)
        VALUES(?1,?2,?3,?4,?5,?6)
        ON CONFLICT(path) DO UPDATE SET
            mtime_ms = excluded.mtime_ms,
            size = excluded.size,
            source = excluded.source,
            fingerprint = excluded.fingerprint,
            adapter_version = excluded.adapter_version
        "#,
        params![
            path,
            mtime_ms,
            size,
            source.as_str(),
            fingerprint,
            ADAPTER_VERSION
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 本轮扫描已看不到的文件不再物理删除其历史记录：工具自身的日志清理/轮转不应抹掉
/// 本地已经统计过的用量。改为给对应记录打归档时间戳，记录仍计入所有统计查询；
/// 只清理 `ingested_files` 的缓存指纹（文件既已消失，也没有 mtime/大小可再对比）。
/// 见 `docs/adr/0004-archive-missing-source-files.md`。
pub fn reconcile_source(
    conn: &Connection,
    source: Source,
    seen_paths: &BTreeSet<String>,
) -> Result<u64, String> {
    let mut stmt = conn
        .prepare("SELECT path FROM ingested_files WHERE source = ?1")
        .map_err(|e| e.to_string())?;
    let cached = stmt
        .query_map(params![source.as_str()], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    let mut archived = 0;
    for path in cached {
        if !seen_paths.contains(&path) {
            archived += archive_records_for_file(conn, &path)?;
            conn.execute("DELETE FROM ingested_files WHERE path = ?1", params![path])
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(archived)
}

/// 把某源文件名下尚未归档的记录标记为已归档（幂等：重复调用不会改写已有的归档时间）。
/// 返回本次新归档的记录数。
pub fn archive_records_for_file(conn: &Connection, source_file: &str) -> Result<u64, String> {
    conn.execute(
        "UPDATE usage_records SET archived_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE source_file = ?1 AND archived_at IS NULL",
        params![source_file],
    )
    .map(|count| count as u64)
    .map_err(|e| e.to_string())
}

/// 永久删除某个来源（或全部来源）已归档的记录。用户在设置页显式触发，不参与常规摄取流程。
pub fn purge_archived(conn: &Connection, source: Option<Source>) -> Result<u64, String> {
    let removed = match source {
        Some(source) => conn.execute(
            "DELETE FROM usage_records WHERE archived_at IS NOT NULL AND source = ?1",
            params![source.as_str()],
        ),
        None => conn.execute(
            "DELETE FROM usage_records WHERE archived_at IS NOT NULL",
            [],
        ),
    }
    .map_err(|e| e.to_string())?;
    Ok(removed as u64)
}

pub fn invalidate_source(conn: &Connection, source: Source) -> Result<(), String> {
    conn.execute(
        "UPDATE ingested_files SET adapter_version = 0 WHERE source = ?1",
        params![source.as_str()],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE conversation_sessions SET adapter_version = 0 WHERE source = ?1",
        params![source.as_str()],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE conversation_session_files SET adapter_version = 0 WHERE source = ?1",
        params![source.as_str()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn remove_unknown_sources(conn: &Connection) -> Result<u64, String> {
    let known = Source::ALL
        .iter()
        .map(|source| format!("'{}'", source.as_str()))
        .collect::<Vec<_>>()
        .join(",");
    let removed = conn
        .execute(
            &format!("DELETE FROM usage_records WHERE source NOT IN ({known})"),
            [],
        )
        .map_err(|e| e.to_string())? as u64;
    conn.execute(
        &format!("DELETE FROM ingested_files WHERE source NOT IN ({known})"),
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(removed)
}

/// 返回 (缓存文件数, 记录总数（含已归档）, Token 总数（含已归档）, 已归档记录数)。
pub fn source_cache_stats(
    conn: &Connection,
    source: Source,
) -> Result<(u64, u64, i64, u64), String> {
    let cached_files = conn
        .query_row(
            "SELECT COUNT(*) FROM ingested_files WHERE source = ?1",
            params![source.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())? as u64;
    let (record_count, total_tokens, archived_record_count) = conn
        .query_row(
            r#"
            SELECT COUNT(*), COALESCE(SUM(total_tokens), 0),
                   COUNT(*) FILTER (WHERE archived_at IS NOT NULL)
            FROM usage_records WHERE source = ?1
            "#,
            params![source.as_str()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;
    Ok((
        cached_files,
        record_count as u64,
        total_tokens,
        archived_record_count as u64,
    ))
}

pub fn load_all(conn: &Connection) -> Result<Vec<UsageRecord>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT occurred_at, source, model, provider, project, session_id, source_file,
                   input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                   reasoning_tokens, total_tokens, native_cost
            FROM usage_records
            "#,
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let source_value: String = row.get(1)?;
            let source = Source::parse(&source_value).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    format!("未知来源：{source_value}").into(),
                )
            })?;
            Ok(UsageRecord {
                occurred_at: row.get(0)?,
                source,
                model: row.get(2)?,
                provider: row.get(3)?,
                project: row.get(4)?,
                session_id: row.get(5)?,
                source_file: row.get(6)?,
                input_tokens: row.get(7)?,
                output_tokens: row.get(8)?,
                cache_read_tokens: row.get(9)?,
                cache_creation_tokens: row.get(10)?,
                reasoning_tokens: row.get(11)?,
                total_tokens: row.get(12)?,
                native_cost: row.get(13)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
