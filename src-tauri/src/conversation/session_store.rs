//! 会话行、会话文件、指纹缓存的 sqlite 读写。
//!
//! 改这部分 schema 或查询时，只看这里就能确知影响面。路径校验只是会话查询的顺手把关，
//! 走兄弟模块 `trusted_path`，不经模块根。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{ConversationMatchField, ConversationSessionRow, Source, UsageRecord};

use super::conversation_adapter;
use super::event_index;
use super::fill_empty_cursor_hash_model;
use super::merge::IndexedAgentMetadata;
use super::read::conversation_source_roots;
use super::toolbox::ParsedConversation;
use super::trusted_path::{ensure_trusted_path, modified_nanos};
use super::CachedConversationFingerprint;
use super::ConversationIndexIssue;
use super::{CONVERSATION_ADAPTER_VERSION, CONVERSATION_SOURCES};

pub(crate) fn upsert_session(
    conn: &Connection,
    session: &ConversationSessionRow,
    is_top_level: bool,
    agent_metadata: &IndexedAgentMetadata,
    source_file_mtime_ns: i64,
    source_file_size: i64,
    source_revision: &str,
) -> Result<(), String> {
    let capabilities = serde_json::to_string(&session.capabilities).map_err(|e| e.to_string())?;
    let agent_metadata = serde_json::to_string(agent_metadata).map_err(|e| e.to_string())?;
    conn.execute(
        r#"
        INSERT INTO conversation_sessions(
            source, session_id, title, project, model, started_at, ended_at,
            source_file, capabilities_json, support_status, file_available,
            source_file_mtime_ns, source_file_size, adapter_version, source_revision,
            is_top_level, agent_metadata_json
        ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
        ON CONFLICT(source, session_id) DO UPDATE SET
            title = excluded.title,
            project = excluded.project,
            model = excluded.model,
            started_at = excluded.started_at,
            ended_at = excluded.ended_at,
            source_file = excluded.source_file,
            capabilities_json = excluded.capabilities_json,
            support_status = excluded.support_status,
            file_available = excluded.file_available,
            source_file_mtime_ns = excluded.source_file_mtime_ns,
            source_file_size = excluded.source_file_size,
            adapter_version = excluded.adapter_version,
            source_revision = excluded.source_revision,
            is_top_level = excluded.is_top_level,
            agent_metadata_json = excluded.agent_metadata_json
        "#,
        params![
            session.source,
            session.session_id,
            session.title,
            session.project,
            session.model,
            session.started_at,
            session.ended_at,
            session.source_file,
            capabilities,
            session.support_status,
            session.file_available,
            source_file_mtime_ns,
            source_file_size,
            CONVERSATION_ADAPTER_VERSION,
            source_revision,
            is_top_level,
            agent_metadata,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn failed_session_paths(
    conn: &Connection,
    source: Source,
    issues: &[ConversationIndexIssue],
) -> Result<BTreeMap<String, BTreeSet<PathBuf>>, String> {
    let mut paths_by_session = BTreeMap::new();
    let mut statement = conn
        .prepare(
            r#"
            SELECT session_id FROM conversation_session_files
            WHERE source = ?1 AND source_file = ?2
            "#,
        )
        .map_err(|error| error.to_string())?;
    for issue in issues {
        let session_ids = statement
            .query_map(params![source.as_str(), issue.path], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        for session_id in session_ids {
            paths_by_session
                .entry(session_id)
                .or_insert_with(BTreeSet::new)
                .insert(PathBuf::from(&issue.path));
        }
    }
    Ok(paths_by_session)
}

pub(crate) fn update_session_files(
    conn: &Connection,
    source: Source,
    session_id: &str,
    paths: &[PathBuf],
    replace: bool,
) -> Result<(), String> {
    if replace {
        conn.execute(
            "DELETE FROM conversation_session_files WHERE source = ?1 AND session_id = ?2",
            params![source.as_str(), session_id],
        )
        .map_err(|error| error.to_string())?;
    }
    for path in paths {
        let metadata =
            fs::metadata(path).map_err(|error| format!("读取文件元数据失败：{error}"))?;
        let source_revision = (conversation_adapter(source)?.revision)(path)?;
        conn.execute(
            r#"
            INSERT INTO conversation_session_files(
                source, session_id, source_file, source_file_mtime_ns, source_file_size,
                adapter_version, source_revision
            ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(source, session_id, source_file) DO UPDATE SET
                source_file_mtime_ns = excluded.source_file_mtime_ns,
                source_file_size = excluded.source_file_size,
                adapter_version = excluded.adapter_version,
                source_revision = excluded.source_revision
            "#,
            params![
                source.as_str(),
                session_id,
                path.to_string_lossy().to_string(),
                modified_nanos(&metadata),
                metadata.len() as i64,
                CONVERSATION_ADAPTER_VERSION,
                source_revision,
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(crate) fn load_usage_records(
    conn: &Connection,
    source: Source,
    session_id: &str,
) -> Result<Vec<UsageRecord>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT occurred_at, model, provider, project, session_id, source_file,
                   input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                   reasoning_tokens, total_tokens, native_cost
            FROM usage_records
            WHERE source = ?1 AND session_id = ?2
            ORDER BY occurred_at ASC, source_file ASC, rowid ASC
            "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params![source.as_str(), session_id], |row| {
            Ok(UsageRecord {
                occurred_at: row.get(0)?,
                source,
                model: row.get(1)?,
                provider: row.get(2)?,
                project: row.get(3)?,
                session_id: row.get(4)?,
                source_file: row.get(5)?,
                input_tokens: row.get(6)?,
                output_tokens: row.get(7)?,
                cache_read_tokens: row.get(8)?,
                cache_creation_tokens: row.get(9)?,
                reasoning_tokens: row.get(10)?,
                total_tokens: row.get(11)?,
                native_cost: row.get(12)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let mut records = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut seen = BTreeSet::new();
    records.retain(|record| seen.insert(usage_record_identity(record)));
    Ok(records)
}

pub(crate) fn usage_record_identity(record: &UsageRecord) -> String {
    serde_json::json!({
        "occurred_at": record.occurred_at,
        "source": record.source,
        "model": record.model,
        "provider": record.provider,
        "project": record.project,
        "session_id": record.session_id,
        "input_tokens": record.input_tokens,
        "output_tokens": record.output_tokens,
        "cache_read_tokens": record.cache_read_tokens,
        "cache_creation_tokens": record.cache_creation_tokens,
        "reasoning_tokens": record.reasoning_tokens,
        "total_tokens": record.total_tokens,
        "native_cost_bits": record.native_cost.map(f64::to_bits),
    })
    .to_string()
}

pub(crate) fn load_session(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Option<ConversationSessionRow>, String> {
    let mut session = conn
        .query_row(
            r#"
            SELECT source, session_id, title, project, model, started_at, ended_at,
                   source_file, capabilities_json, support_status, file_available,
                   0, -1
            FROM conversation_sessions WHERE source = ?1 AND session_id = ?2
            "#,
            params![source, session_id],
            row_from_sql,
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(session) = &mut session {
        let paths = load_session_files(conn, source, session_id)?;
        if !paths.is_empty() {
            session.source_files = paths
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect();
        }
        fill_empty_cursor_hash_model(conn, session)?;
    }
    Ok(session)
}

pub(crate) fn load_session_files(
    conn: &Connection,
    source: &str,
    session_id: &str,
) -> Result<Vec<PathBuf>, String> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT source_file FROM conversation_session_files
            WHERE source = ?1 AND session_id = ?2
            ORDER BY source_file ASC
            "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![source, session_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .map(|result| result.map(PathBuf::from))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

pub(super) fn load_trusted_session_files(
    conn: &Connection,
    home: &Path,
    source: &str,
    session_id: &str,
) -> Result<(Source, ConversationSessionRow, Vec<PathBuf>), String> {
    let source = Source::parse(source).filter(|source| CONVERSATION_SOURCES.contains(source));
    let Some(source) = source else {
        return Err("该来源尚未支持对话详情".to_string());
    };
    let session = load_session(conn, source.as_str(), session_id)?
        .ok_or_else(|| "未找到该对话记录".to_string())?;
    let roots = conversation_source_roots(home, source);
    let representative = PathBuf::from(&session.source_file);
    if !representative.exists() {
        return Err("原始文件已不存在，无法读取对话详情".to_string());
    }
    ensure_trusted_path(&representative, &roots)?;
    let mut paths = load_session_files(conn, source.as_str(), session_id)?;
    if !paths.is_empty() && !paths.iter().any(|path| path == &representative) {
        return Err("会话索引的代表文件与来源清单不一致".to_string());
    }
    if paths.is_empty() {
        paths.push(representative);
    }
    for path in &paths {
        if !path.exists() {
            return Err("原始文件已不存在，无法读取对话详情".to_string());
        }
        ensure_trusted_path(path, &roots)?;
    }
    Ok((source, session, paths))
}

pub(super) fn ensure_matching_session(
    parsed: &ParsedConversation,
    session: &ConversationSessionRow,
) -> Result<(), String> {
    if parsed.session.session_id == session.session_id {
        Ok(())
    } else {
        Err("原始文件中的会话 ID 与索引不一致".to_string())
    }
}

pub(crate) fn tombstone_missing_sessions(
    conn: &Connection,
    source: Source,
    seen_session_ids: &BTreeSet<String>,
) -> Result<(), String> {
    let cached = conn
        .prepare("SELECT session_id FROM conversation_sessions WHERE source = ?1")
        .map_err(|e| e.to_string())?
        .query_map(params![source.as_str()], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for session_id in cached {
        if seen_session_ids.contains(&session_id) {
            continue;
        }
        mark_session_unavailable(conn, source, &session_id)?;
        event_index::clear_session_events(conn, source, &session_id)?;
    }
    Ok(())
}

pub(crate) fn mark_session_unavailable(
    conn: &Connection,
    source: Source,
    session_id: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE conversation_sessions SET file_available = 0 WHERE source = ?1 AND session_id = ?2",
        params![source.as_str(), session_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationSessionRow> {
    let capabilities_json: String = row.get(8)?;
    let source_file: String = row.get(7)?;
    let match_rank: i64 = row.get(12)?;
    Ok(ConversationSessionRow {
        source: row.get(0)?,
        session_id: row.get(1)?,
        title: row.get(2)?,
        project: row.get(3)?,
        model: row.get(4)?,
        started_at: row.get(5)?,
        ended_at: row.get(6)?,
        source_file: source_file.clone(),
        source_files: vec![source_file],
        capabilities: serde_json::from_str(&capabilities_json).unwrap_or_default(),
        support_status: row.get(9)?,
        file_available: row.get(10)?,
        event_index_ready: row.get::<_, i64>(11)? != 0,
        match_field: match match_rank {
            0 => Some(ConversationMatchField::Title),
            1 => Some(ConversationMatchField::Body),
            _ => None,
        },
        ..Default::default()
    })
}

pub(crate) fn load_cached_fingerprints(
    conn: &Connection,
    source: Source,
    path: &Path,
) -> Result<Vec<CachedConversationFingerprint>, String> {
    let mut stmt = conn
        .prepare(
            r#"
        SELECT DISTINCT session_id, source_file_mtime_ns, source_file_size, adapter_version,
                        source_revision, indexed_byte_offset, indexed_line, has_live_generation
        FROM (
            SELECT files.session_id, files.source_file_mtime_ns, files.source_file_size,
                   files.adapter_version, files.source_revision,
                   files.indexed_byte_offset, files.indexed_line,
                   CASE WHEN sessions.event_index_generation IS NULL THEN 0 ELSE 1 END
                     AS has_live_generation
            FROM conversation_session_files AS files
            JOIN conversation_sessions AS sessions
              ON sessions.source = files.source AND sessions.session_id = files.session_id
            WHERE files.source = ?1 AND files.source_file = ?2 AND sessions.file_available = 1
            UNION ALL
            SELECT session_id, source_file_mtime_ns, source_file_size, adapter_version,
                   source_revision, 0, 0, 0
            FROM conversation_sessions
            WHERE source = ?1 AND source_file = ?2 AND file_available = 1
        )
        "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(
            params![source.as_str(), path.to_string_lossy().to_string()],
            |row| {
                Ok(CachedConversationFingerprint {
                    session_id: row.get(0)?,
                    source_file_mtime_ns: row.get(1)?,
                    source_file_size: row.get(2)?,
                    adapter_version: row.get(3)?,
                    source_revision: row.get(4)?,
                    indexed_byte_offset: row.get(5)?,
                    indexed_line: row.get(6)?,
                    has_live_generation: row.get::<_, i64>(7)? != 0,
                })
            },
        )
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}
