use rusqlite::{params, Connection};

use super::LOWERCASE_MODEL_VERSION;

pub(crate) fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS usage_records (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            occurred_at TEXT NOT NULL,
            source TEXT NOT NULL,
            model TEXT NOT NULL,
            provider TEXT NOT NULL,
            project TEXT NOT NULL,
            session_id TEXT NOT NULL,
            source_file TEXT NOT NULL,
            input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            cache_read_tokens INTEGER NOT NULL,
            cache_creation_tokens INTEGER NOT NULL,
            reasoning_tokens INTEGER NOT NULL,
            total_tokens INTEGER NOT NULL,
            native_cost REAL
        );
        CREATE INDEX IF NOT EXISTS idx_usage_occurred ON usage_records(occurred_at);
        CREATE INDEX IF NOT EXISTS idx_usage_source ON usage_records(source);
        CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_records(model);
        CREATE INDEX IF NOT EXISTS idx_usage_project ON usage_records(project);
        CREATE INDEX IF NOT EXISTS idx_usage_session ON usage_records(session_id);
        -- filter_options 每次首屏都要 SELECT DISTINCT provider；没有这个索引就是全表扫描
        -- 加临时 B 树排序（实测 17 万行要 450ms，只为取回 26 个值）。
        CREATE INDEX IF NOT EXISTS idx_usage_provider ON usage_records(provider);
        -- Ingestion replaces records by file; without this index every changed file scans the full cache.
        CREATE INDEX IF NOT EXISTS idx_usage_source_file ON usage_records(source_file);
        -- Almost every aggregate query in query.rs filters by source and/or occurred_at together
        -- (overview/trend/billing_windows); a composite index lets those use one index instead of
        -- a full scan + occurred_at index-only scan.
        CREATE INDEX IF NOT EXISTS idx_usage_source_occurred ON usage_records(source, occurred_at);

        -- 按 UTC 天预聚合。时间窗切成中间完整 UTC 天 + 两端 partial：整天走这张表，
        -- 两端补差走明细。无时间窗时整段走这张表；小时粒度无法从日级还原，仍走明细。
        --
        -- 键里带 session_id 看着反直觉，实测却几乎不涨行数：一个会话通常落在同一天、
        -- 用同一个模型和项目。17 万行原始记录聚成 943 行（不含 session）或 3031 行
        -- （含 session），57:1 的压缩换来 COUNT(DISTINCT session) 和 top_sessions
        -- 也能走这张表，不必再为会话维度单开一张。
        --
        -- has_native 进主键，是为了让「native_cost 优先，否则按 token 计价」这条规则
        -- 在聚合后依然成立：两类行分开存，各自的 token 和不会混。实测只多出 4 行。
        CREATE TABLE IF NOT EXISTS usage_rollup (
            day TEXT NOT NULL,
            source TEXT NOT NULL,
            model TEXT NOT NULL,
            provider TEXT NOT NULL,
            project TEXT NOT NULL,
            session_id TEXT NOT NULL,
            has_native INTEGER NOT NULL,
            input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            cache_read_tokens INTEGER NOT NULL,
            cache_creation_tokens INTEGER NOT NULL,
            reasoning_tokens INTEGER NOT NULL,
            total_tokens INTEGER NOT NULL,
            native_cost REAL NOT NULL DEFAULT 0,
            record_count INTEGER NOT NULL,
            first_at TEXT NOT NULL,
            last_at TEXT NOT NULL,
            -- 「最晚非空」排序键，与 query.rs 的 latest_nonempty_key_sql 同构，
            -- 这样上层还能继续用 MAX() 跨行归并出会话级的展示标签。
            file_key TEXT,
            PRIMARY KEY (day, source, session_id, model, provider, project, has_native)
        );
        CREATE INDEX IF NOT EXISTS idx_usage_rollup_day ON usage_rollup(day);
        CREATE INDEX IF NOT EXISTS idx_usage_rollup_model ON usage_rollup(model);
        CREATE INDEX IF NOT EXISTS idx_usage_rollup_project ON usage_rollup(project);
        CREATE INDEX IF NOT EXISTS idx_usage_rollup_session ON usage_rollup(source, session_id);

        -- 预聚合表是否已建全。补建要十几秒，期间「表非空」并不代表内容完整，
        -- 所以就绪与否得显式记着，不能从行数推断。
        CREATE TABLE IF NOT EXISTS rollup_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            ready INTEGER NOT NULL DEFAULT 0
        );
        INSERT OR IGNORE INTO rollup_state (id, ready) VALUES (1, 0);

        CREATE TABLE IF NOT EXISTS ingested_files (
            path TEXT PRIMARY KEY,
            mtime_ms INTEGER NOT NULL,
            size INTEGER NOT NULL,
            source TEXT NOT NULL DEFAULT '',
            fingerprint TEXT NOT NULL DEFAULT '',
            adapter_version INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS cursor_account_usage (
            fingerprint TEXT PRIMARY KEY,
            occurred_at TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            cache_read_tokens INTEGER NOT NULL,
            cache_creation_tokens INTEGER NOT NULL,
            is_headless INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cursor_account_occurred
            ON cursor_account_usage(occurred_at);

        CREATE TABLE IF NOT EXISTS cursor_account_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS cursor_sessions (
            source_file TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            project TEXT NOT NULL,
            turn_count INTEGER NOT NULL,
            success_count INTEGER NOT NULL,
            error_count INTEGER NOT NULL,
            aborted_count INTEGER NOT NULL,
            user_prompt_count INTEGER NOT NULL DEFAULT 0,
            subagent_count INTEGER NOT NULL DEFAULT 0,
            tool_calls_json TEXT NOT NULL,
            models_json TEXT NOT NULL DEFAULT '[]',
            sources_json TEXT NOT NULL DEFAULT '[]',
            extensions_json TEXT NOT NULL DEFAULT '{}',
            first_seen_at TEXT,
            last_seen_at TEXT,
            files_touched INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_cursor_sessions_project ON cursor_sessions(project);
        CREATE INDEX IF NOT EXISTS idx_cursor_sessions_last_seen ON cursor_sessions(last_seen_at);

        CREATE TABLE IF NOT EXISTS cursor_session_files (
            path TEXT PRIMARY KEY,
            mtime_ms INTEGER NOT NULL,
            size INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS cursor_session_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS conversation_sessions (
            source TEXT NOT NULL,
            session_id TEXT NOT NULL,
            title TEXT NOT NULL,
            project TEXT NOT NULL,
            model TEXT NOT NULL,
            started_at TEXT NOT NULL,
            ended_at TEXT NOT NULL,
            source_file TEXT NOT NULL,
            capabilities_json TEXT NOT NULL DEFAULT '[]',
            support_status TEXT NOT NULL DEFAULT 'experimental',
            file_available INTEGER NOT NULL DEFAULT 1,
            source_file_mtime_ms INTEGER NOT NULL DEFAULT 0,
            source_file_mtime_ns INTEGER NOT NULL DEFAULT 0,
            source_file_size INTEGER NOT NULL DEFAULT 0,
            adapter_version INTEGER NOT NULL DEFAULT 0,
            source_revision TEXT NOT NULL DEFAULT '',
            is_top_level INTEGER NOT NULL DEFAULT 1,
            -- Reconstructable relationship IDs only; event bodies live in conversation_events (ADR 0011).
            agent_metadata_json TEXT NOT NULL DEFAULT '{}',
            event_index_generation INTEGER,
            PRIMARY KEY(source, session_id)
        );
        CREATE INDEX IF NOT EXISTS idx_conversation_sessions_ended
            ON conversation_sessions(ended_at DESC);
        CREATE INDEX IF NOT EXISTS idx_conversation_sessions_source_file
            ON conversation_sessions(source, source_file);

        CREATE TABLE IF NOT EXISTS conversation_session_files (
            source TEXT NOT NULL,
            session_id TEXT NOT NULL,
            source_file TEXT NOT NULL,
            source_file_mtime_ns INTEGER NOT NULL DEFAULT 0,
            source_file_size INTEGER NOT NULL DEFAULT 0,
            adapter_version INTEGER NOT NULL DEFAULT 0,
            source_revision TEXT NOT NULL DEFAULT '',
            indexed_byte_offset INTEGER NOT NULL DEFAULT 0,
            indexed_line INTEGER NOT NULL DEFAULT 0,
            max_sequence INTEGER,
            PRIMARY KEY(source, session_id, source_file)
        );
        CREATE INDEX IF NOT EXISTS idx_conversation_session_files_session
            ON conversation_session_files(source, session_id);

        CREATE TABLE IF NOT EXISTS conversation_events (
            source TEXT NOT NULL,
            session_id TEXT NOT NULL,
            event_id TEXT NOT NULL,
            sequence INTEGER,
            source_file TEXT NOT NULL,
            source_sequence INTEGER NOT NULL,
            kind TEXT NOT NULL,
            actor TEXT,
            name TEXT,
            occurred_at TEXT,
            occurred_at_sort TEXT,
            text TEXT,
            attachments_json TEXT NOT NULL DEFAULT '[]',
            capability_status TEXT NOT NULL,
            content_status TEXT NOT NULL,
            identity_hash TEXT NOT NULL,
            identity_occurrence INTEGER NOT NULL,
            index_generation INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_conversation_events_session_gen
            ON conversation_events(source, session_id, index_generation, sequence);
        CREATE INDEX IF NOT EXISTS idx_conversation_events_session_kind_name
            ON conversation_events(source, session_id, index_generation, kind, actor, name);

        CREATE TABLE IF NOT EXISTS official_quota (
            provider TEXT PRIMARY KEY,
            windows_json TEXT NOT NULL,
            captured_at TEXT NOT NULL,
            error TEXT,
            plan TEXT,
            prev_windows_json TEXT,
            prev_captured_at TEXT
        );
        "#,
    )
    .map_err(|e| e.to_string())?;
    ensure_column(conn, "official_quota", "plan", "TEXT")?;
    ensure_column(conn, "official_quota", "prev_windows_json", "TEXT")?;
    ensure_column(conn, "official_quota", "prev_captured_at", "TEXT")?;
    ensure_column(conn, "ingested_files", "source", "TEXT NOT NULL DEFAULT ''")?;
    ensure_column(
        conn,
        "ingested_files",
        "fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "ingested_files",
        "adapter_version",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    // 源文件被工具自身清理后不再物理删除历史记录，只打时间戳归档（ADR 0004）。
    ensure_column(conn, "usage_records", "archived_at", "TEXT")?;
    ensure_column(
        conn,
        "conversation_sessions",
        "file_available",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "conversation_sessions",
        "source_file_mtime_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "conversation_sessions",
        "source_file_mtime_ns",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "conversation_sessions",
        "source_file_size",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "conversation_sessions",
        "adapter_version",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "conversation_sessions",
        "source_revision",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "conversation_sessions",
        "is_top_level",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "conversation_sessions",
        "agent_metadata_json",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(
        conn,
        "conversation_sessions",
        "event_index_generation",
        "INTEGER",
    )?;
    ensure_column(
        conn,
        "conversation_session_files",
        "source_file_mtime_ns",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "conversation_session_files",
        "source_file_size",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "conversation_session_files",
        "adapter_version",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "conversation_session_files",
        "source_revision",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "conversation_session_files",
        "indexed_byte_offset",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "conversation_session_files",
        "indexed_line",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "conversation_session_files",
        "max_sequence",
        "INTEGER",
    )?;
    migrate_conversation_session_files_key(conn)?;
    ensure_column(
        conn,
        "cursor_sessions",
        "user_prompt_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "cursor_sessions",
        "subagent_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "cursor_sessions",
        "sources_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        conn,
        "cursor_sessions",
        "extensions_json",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    // 必须放在上面的 ensure_column 之后：老版本缓存库的 ingested_files 表可能还没有
    // source 列，若把这条建索引语句挪进最上面的初始 CREATE TABLE batch，会在旧库上先于
    // ALTER TABLE 执行而报错。
    // reconcile_source 每个来源每轮 ingest 都要 "SELECT path FROM ingested_files WHERE
    // source = ?"，没有这个索引就是全表扫描。
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_ingested_files_source ON ingested_files(source);",
    )
    .map_err(|e| e.to_string())?;
    conn.execute_batch(
        r#"
        UPDATE ingested_files
        SET source = COALESCE(
            (SELECT source FROM usage_records WHERE source_file = ingested_files.path LIMIT 1),
            ''
        )
        WHERE source = '';
        "#,
    )
    .map_err(|e| e.to_string())?;
    ensure_conversation_events_fts(conn)?;
    migrate_lowercase_model(conn)
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![name],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

/// 正文全文索引是 `conversation_events` 的派生缓存：源文件仍是权威，重建事件表后可再 rebuild。
/// trigram 按子串匹配，对应原先目录 LIKE 的「关键字」预期；短于 3 个字符的查询只走标题。
fn ensure_conversation_events_fts(conn: &Connection) -> Result<(), String> {
    let existed = table_exists(conn, "conversation_events_fts")?;
    conn.execute_batch(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS conversation_events_fts USING fts5(
            text,
            name,
            content='conversation_events',
            content_rowid='rowid',
            tokenize='trigram'
        );
        CREATE TRIGGER IF NOT EXISTS conversation_events_ai AFTER INSERT ON conversation_events BEGIN
            INSERT INTO conversation_events_fts(rowid, text, name)
            VALUES (new.rowid, COALESCE(new.text, ''), COALESCE(new.name, ''));
        END;
        CREATE TRIGGER IF NOT EXISTS conversation_events_ad AFTER DELETE ON conversation_events BEGIN
            INSERT INTO conversation_events_fts(conversation_events_fts, rowid, text, name)
            VALUES ('delete', old.rowid, COALESCE(old.text, ''), COALESCE(old.name, ''));
        END;
        CREATE TRIGGER IF NOT EXISTS conversation_events_au AFTER UPDATE ON conversation_events BEGIN
            INSERT INTO conversation_events_fts(conversation_events_fts, rowid, text, name)
            VALUES ('delete', old.rowid, COALESCE(old.text, ''), COALESCE(old.name, ''));
            INSERT INTO conversation_events_fts(rowid, text, name)
            VALUES (new.rowid, COALESCE(new.text, ''), COALESCE(new.name, ''));
        END;
        "#,
    )
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

/// `insert_records` 从此保证写入的 `model` 是小写，价格 JOIN 才能直接比较列值。
/// 老库里可能残留大写值，归一化一次让不变量对历史数据同样成立。
///
/// 用 `user_version` 记账而不是每次启动扫全表：`model <> lower(model)` 用不上索引，
/// 库大了以后那是每次开机都要付的全表扫描。
fn migrate_lowercase_model(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if version >= LOWERCASE_MODEL_VERSION {
        return Ok(());
    }
    conn.execute_batch(&format!(
        "UPDATE usage_records SET model = lower(model) WHERE model <> lower(model);
         PRAGMA user_version = {LOWERCASE_MODEL_VERSION};"
    ))
    .map_err(|e| e.to_string())
}

fn migrate_conversation_session_files_key(conn: &Connection) -> Result<(), String> {
    let session_id_pk = conn
        .prepare("PRAGMA table_info(conversation_session_files)")
        .map_err(|error| error.to_string())?
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find_map(|(name, pk)| (name == "session_id").then_some(pk))
        .unwrap_or(0);
    if session_id_pk > 0 {
        return Ok(());
    }

    let migration = conn.execute_batch(
        r#"
        SAVEPOINT migrate_conversation_session_files_key;
        DROP INDEX IF EXISTS idx_conversation_session_files_session;
        DROP TABLE IF EXISTS conversation_session_files_v2;
        CREATE TABLE conversation_session_files_v2 (
            source TEXT NOT NULL,
            session_id TEXT NOT NULL,
            source_file TEXT NOT NULL,
            source_file_mtime_ns INTEGER NOT NULL DEFAULT 0,
            source_file_size INTEGER NOT NULL DEFAULT 0,
            adapter_version INTEGER NOT NULL DEFAULT 0,
            source_revision TEXT NOT NULL DEFAULT '',
            indexed_byte_offset INTEGER NOT NULL DEFAULT 0,
            indexed_line INTEGER NOT NULL DEFAULT 0,
            max_sequence INTEGER,
            PRIMARY KEY(source, session_id, source_file)
        );
        INSERT OR IGNORE INTO conversation_session_files_v2(
            source, session_id, source_file, source_file_mtime_ns, source_file_size,
            adapter_version, source_revision
        )
        SELECT source, session_id, source_file, source_file_mtime_ns, source_file_size,
               adapter_version, source_revision
        FROM conversation_session_files;
        DROP TABLE conversation_session_files;
        ALTER TABLE conversation_session_files_v2 RENAME TO conversation_session_files;
        CREATE INDEX idx_conversation_session_files_session
            ON conversation_session_files(source, session_id);
        RELEASE migrate_conversation_session_files_key;
        "#,
    );
    if let Err(error) = migration {
        let _ = conn.execute_batch(
            "ROLLBACK TO migrate_conversation_session_files_key; RELEASE migrate_conversation_session_files_key;",
        );
        return Err(error.to_string());
    }
    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if !columns.iter().any(|name| name == column) {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition}"
        ))
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
