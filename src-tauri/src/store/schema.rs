use rusqlite::{params, Connection, OptionalExtension};

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
    ensure_conversation_event_tables(conn)?;
    ensure_conversation_events_fts(conn)?;
    migrate_lowercase_model(conn)
}

/// 事件表、路径字典与工具汇总表建在一起：三者是同一份派生缓存的三个部分，列定义要被
/// `migrate_conversation_events_layout` 复用，所以拿出来单独建，不混在上面的大 batch 里。
const CONVERSATION_EVENTS_COLUMNS: &str = r#"
    source TEXT NOT NULL,
    session_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    sequence INTEGER,
    file_id INTEGER NOT NULL,
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
"#;

pub(crate) const CONVERSATION_EVENT_COLUMN_LIST: &str = "source, session_id, event_id, sequence, \
    file_id, source_sequence, kind, actor, name, occurred_at, occurred_at_sort, text, \
    attachments_json, capability_status, content_status, identity_hash, identity_occurrence, \
    index_generation";

fn ensure_conversation_event_tables(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(&format!(
        r#"
        -- 事件表原先每行都带一条会话源文件的绝对路径，而路径的去重基数只有几千条：
        -- 实测 106 万行里 source_file 一列就占 123MB。这里只存 file_id，路径留一份。
        CREATE TABLE IF NOT EXISTS conversation_files (
            file_id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS conversation_events ({CONVERSATION_EVENTS_COLUMNS});
        CREATE INDEX IF NOT EXISTS idx_conversation_events_session_gen
            ON conversation_events(source, session_id, index_generation, sequence);

        -- 目录按工具名筛选问的是「这个会话用过哪个工具」，本质是每会话每工具一行的事实。
        -- 原先靠事件表上一条 (source, session_id, generation, kind, actor, name) 的宽索引
        -- 回答，等于为几万条事实在百万行上养了 83MB 索引。
        CREATE TABLE IF NOT EXISTS conversation_session_tools (
            source TEXT NOT NULL,
            session_id TEXT NOT NULL,
            index_generation INTEGER NOT NULL,
            -- 失败的 tool_result 可能没有工具名，用空串占位，别让它掉出汇总。
            name TEXT NOT NULL,
            is_tool_event INTEGER NOT NULL,
            is_call INTEGER NOT NULL,
            is_failure INTEGER NOT NULL,
            PRIMARY KEY(source, session_id, index_generation, name)
        ) WITHOUT ROWID;
        "#
    ))
    .map_err(|e| e.to_string())
}

/// 工具汇总表的唯一定义：迁移、整份重建、增量追加都走这段 SQL，语义不会各写一份然后漂掉。
/// `predicate` 限定重建范围，聚合始终覆盖该范围内的全部事件，所以重跑是幂等的。
///
/// 三个标志位与 `conversation::catalog` 的筛选一一对应：`is_tool_event` = tool_call/tool_result，
/// `is_call` = 仅 tool_call（工具名下拉用），`is_failure` = kind=error 且 actor=tool
/// （失败的 tool_result 在摄取时就是记成这个形状的）。
pub(crate) fn conversation_session_tools_sql(predicate: &str) -> String {
    format!(
        r#"
        INSERT OR REPLACE INTO conversation_session_tools(
            source, session_id, index_generation, name, is_tool_event, is_call, is_failure
        )
        SELECT source, session_id, index_generation, COALESCE(name, ''),
               MAX(kind IN ('tool_call', 'tool_result')),
               MAX(kind = 'tool_call'),
               MAX(kind = 'error' AND actor = 'tool')
        FROM conversation_events
        WHERE ({predicate})
          AND (kind IN ('tool_call', 'tool_result') OR (kind = 'error' AND actor = 'tool'))
        GROUP BY source, session_id, index_generation, COALESCE(name, '')
        "#
    )
}

/// 老库的事件表是每行存整条 `source_file`、并且没有工具汇总表。判据就看那一列还在不在。
pub(crate) fn conversation_events_needs_layout_migration(
    conn: &Connection,
) -> Result<bool, String> {
    table_columns(conn, "conversation_events")
        .map(|columns| columns.iter().any(|c| c == "source_file"))
}

/// 把事件表换成 file_id 形态，顺带把工具汇总表灌起来。1.1GB 表要整份复制、倒排要重灌，
/// 百万行量级要几分钟，所以和正文索引迁移一样由后台线程调用。
///
/// 倒排是外置 content 的，`rowid` 在复制过程中全部重排，必须在同一个事务里连带重建，
/// 否则中途的查询会读到对不上号的行。
pub(crate) fn migrate_conversation_events_layout(conn: &Connection) -> Result<(), String> {
    if !conversation_events_needs_layout_migration(conn)? {
        return Ok(());
    }
    let migration = conn.execute_batch(&format!(
        r#"
        SAVEPOINT migrate_conversation_events_layout;
        DROP TABLE IF EXISTS conversation_events_fts;
        DROP TRIGGER IF EXISTS conversation_events_ai;
        DROP TRIGGER IF EXISTS conversation_events_ad;
        DROP TRIGGER IF EXISTS conversation_events_au;
        DROP TABLE IF EXISTS conversation_events_v2;
        INSERT OR IGNORE INTO conversation_files(path)
            SELECT DISTINCT source_file FROM conversation_events;
        CREATE TABLE conversation_events_v2 ({CONVERSATION_EVENTS_COLUMNS});
        INSERT INTO conversation_events_v2({CONVERSATION_EVENT_COLUMN_LIST})
        SELECT e.source, e.session_id, e.event_id, e.sequence, f.file_id, e.source_sequence,
               e.kind, e.actor, e.name, e.occurred_at, e.occurred_at_sort, e.text,
               e.attachments_json, e.capability_status, e.content_status, e.identity_hash,
               e.identity_occurrence, e.index_generation
        FROM conversation_events e
        JOIN conversation_files f ON f.path = e.source_file;
        DROP TABLE conversation_events;
        ALTER TABLE conversation_events_v2 RENAME TO conversation_events;
        DROP INDEX IF EXISTS idx_conversation_events_session_kind_name;
        CREATE INDEX IF NOT EXISTS idx_conversation_events_session_gen
            ON conversation_events(source, session_id, index_generation, sequence);
        DELETE FROM conversation_session_tools;
        {tools};
        CREATE VIRTUAL TABLE conversation_events_fts USING fts5({CONVERSATION_FTS_DEFINITION});
        {triggers}
        INSERT INTO conversation_events_fts(conversation_events_fts) VALUES('rebuild');
        RELEASE migrate_conversation_events_layout;
        "#,
        tools = conversation_session_tools_sql("1 = 1"),
        triggers = CONVERSATION_FTS_TRIGGERS,
    ));
    if let Err(error) = migration {
        let _ = conn.execute_batch(
            "ROLLBACK TO migrate_conversation_events_layout; RELEASE migrate_conversation_events_layout;",
        );
        return Err(error.to_string());
    }
    Ok(())
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![name],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

/// `detail=none` 只记「哪一行含这个三元组」，不记它出现在什么位置。位置表在这里是纯开销：
/// 检索侧不用 `bm25()`/`snippet()`，排序键是手写的 0/1，片段由 Rust 从正文切。106 万事件、
/// 400MB 正文的真实库实测倒排从 2732MB 降到 294MB，查询还快一倍。代价是 FTS5 不再接受短语
/// 查询，调用方要自己把关键词切成三元组用 AND 连接、再回表用 LIKE 剔假阳性，
/// 见 `conversation::catalog_search`。
const CONVERSATION_FTS_DEFINITION: &str = r#"
    text,
    name,
    content='conversation_events',
    content_rowid='rowid',
    tokenize='trigram',
    detail='none',
    columnsize=0
"#;

const CONVERSATION_FTS_TRIGGERS: &str = r#"
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
"#;

fn conversation_fts_sql(conn: &Connection) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'conversation_events_fts'",
        [],
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
/// 后台线程（见 `lib.rs::spawn_conversation_fts_migration`）。迁移期间旧表继续服务查询：
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

/// 正文全文索引是 `conversation_events` 的派生缓存：源文件仍是权威，重建事件表后可再 rebuild。
/// trigram 按子串匹配，对应原先目录 LIKE 的「关键字」预期；短于 3 个字符的查询只走标题。
///
/// 这里只负责「没有就建」。已存在的旧形态表不在这条路径上换——那要几十秒，不能挡启动。
fn ensure_conversation_events_fts(conn: &Connection) -> Result<(), String> {
    let existed = table_exists(conn, "conversation_events_fts")?;
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

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(columns)
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let columns = table_columns(conn, table)?;
    if !columns.iter().any(|name| name == column) {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition}"
        ))
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
