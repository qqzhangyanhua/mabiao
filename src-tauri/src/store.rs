use std::collections::BTreeSet;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::domain::{
    CursorSessionRecord, CursorUsageEvent, OfficialQuotaWindow, Source, UsageRecord,
};

pub const ADAPTER_VERSION: i64 = 8;

/// `user_version` 记账：1 = usage_records.model 已归一化成小写。
const LOWERCASE_MODEL_VERSION: i64 = 1;

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

/// 32 MiB：够装下一轮增量摄取的全部脏页，又不至于让「重建全部」之后留下一个上百 MB 的 WAL。
const WAL_SIZE_LIMIT_BYTES: i64 = 32 * 1024 * 1024;

fn init_schema(conn: &Connection) -> Result<(), String> {
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

        -- 按天预聚合，给不带时间过滤的全量查询用（首屏默认就是「全部」）。
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

        CREATE TABLE IF NOT EXISTS official_quota (
            provider TEXT PRIMARY KEY,
            windows_json TEXT NOT NULL,
            captured_at TEXT NOT NULL,
            error TEXT
        );
        "#,
    )
    .map_err(|e| e.to_string())?;
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
    migrate_lowercase_model(conn)
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

/// 预聚合表是否已经建全，可以拿来回答查询。
///
/// 靠「表非空」推断是不够的：补建要花十几秒，这期间若发生一次摄取，增量重建会往空表里
/// 只写进那一两天——表非空了，内容却只有零头，查询照着它算就会静默少掉全部历史。
/// 所以就绪与否必须是显式状态，由补建完成时才置上。
pub fn rollup_is_ready(conn: &Connection) -> bool {
    conn.query_row("SELECT ready FROM rollup_state WHERE id = 1", [], |row| {
        row.get::<_, i64>(0)
    })
    .map(|ready| ready != 0)
    .unwrap_or(false)
}

/// 需要补建吗——原始表有数据而预聚合表还没就绪。
pub fn rollup_needs_backfill(conn: &Connection) -> Result<bool, String> {
    if rollup_is_ready(conn) {
        return Ok(false);
    }
    conn.query_row("SELECT EXISTS(SELECT 1 FROM usage_records)", [], |row| {
        row.get(0)
    })
    .map_err(|e| e.to_string())
}

/// 整表补建并置为就绪。
///
/// 老库第一次升到带 `usage_rollup` 的版本、或从不含该表的旧备份恢复时都要跑一次。
/// 350 万行要十几秒，所以调用方应当放到后台——补建期间 `rollup_is_ready` 为假，
/// 查询会自动回退原始表，慢一点但数字是对的。
pub fn backfill_rollup(conn: &Connection) -> Result<u64, String> {
    let written = rebuild_rollup(conn)?;
    conn.execute("UPDATE rollup_state SET ready = 1 WHERE id = 1", [])
        .map_err(|e| e.to_string())?;
    Ok(written)
}

/// 某个源文件的记录落在哪些 UTC 日期上。
///
/// 摄取要替换一个文件时，得先问清它原来占了哪几天——那几天的预聚合行在记录删掉后就失效了。
/// 走 `idx_usage_source_file`，几千个文件的库上也是索引查找。
pub fn days_for_file(conn: &Connection, source_file: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT substr(occurred_at, 1, 10) FROM usage_records WHERE source_file = ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![source_file], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// 只重算指定几天的预聚合行。
///
/// 全量重建在 350 万行的库上要十几秒，而一次摄取通常只动到今天这一两天。按���重算把
/// 这份开销压到跟改动量成正比，摄取才不会随历史数据一起变慢。
///
/// 用 `occurred_at >= day AND occurred_at < day+1` 而不是 `substr(...) = day`：
/// 前者能走 `idx_usage_occurred`，后者对每行调函数，退化成全表扫描。
/// 日期边界用字符串比较即可——`occurred_at` 是 ISO 8601，字典序就是时间序；
/// 上界取 `day` 后缀 `~`（ASCII 126）是因为同一天的时间戳第 11 位只会是 `T`（84），
/// 一定小于 `~`，而下一天的日期部分已经变大，落不进这个区间。
pub fn rebuild_rollup_days(conn: &Connection, days: &BTreeSet<String>) -> Result<(), String> {
    for day in days {
        conn.execute("DELETE FROM usage_rollup WHERE day = ?1", params![day])
            .map_err(|e| e.to_string())?;
        // 空 day 对应 occurred_at 本身为空的脏数据，范围比较框不住，单独按 substr 兜。
        let (predicate, bounds): (&str, Vec<String>) = if day.is_empty() {
            ("substr(r.occurred_at, 1, 10) = ''", Vec::new())
        } else {
            (
                "r.occurred_at >= ?1 AND r.occurred_at < ?2",
                vec![day.clone(), format!("{day}~")],
            )
        };
        let sql = format!(
            r#"
            INSERT INTO usage_rollup (
                day, source, model, provider, project, session_id, has_native,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                reasoning_tokens, total_tokens, native_cost, record_count,
                first_at, last_at, file_key
            )
            SELECT
                substr(r.occurred_at, 1, 10),
                r.source, r.model, r.provider, r.project, r.session_id,
                CASE WHEN r.native_cost IS NOT NULL THEN 1 ELSE 0 END,
                SUM(r.input_tokens), SUM(r.output_tokens), SUM(r.cache_read_tokens),
                SUM(r.cache_creation_tokens), SUM(r.reasoning_tokens), SUM(r.total_tokens),
                COALESCE(SUM(r.native_cost), 0),
                COUNT(*),
                MIN(r.occurred_at), MAX(r.occurred_at),
                MAX(CASE WHEN r.source_file != '' THEN r.occurred_at || char(31) || r.source_file END)
            FROM usage_records r
            WHERE {predicate}
            GROUP BY 1, 2, 3, 4, 5, 6, 7
            "#
        );
        conn.execute(&sql, rusqlite::params_from_iter(bounds.iter()))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 从 `usage_records` 整表重建 `usage_rollup`。
///
/// 刻意做成全量重建而不是增量维护：预聚合表一旦和原始表对不上，界面会显示错误数字
/// 且很难察觉，而增量维护的边界（删文件、改文件、跨天会话）正是最容易漏的地方。
/// 实测 17 万行重建约 0.2s，摄取本身要扫几千个文件，这点开销可以忽略；调用方只在
/// 真有记录写入或删除时才调，缓存全命中的摄取不会触发。
pub fn rebuild_rollup(conn: &Connection) -> Result<u64, String> {
    conn.execute("DELETE FROM usage_rollup", [])
        .map_err(|e| e.to_string())?;
    let written = conn
        .execute(
            r#"
            INSERT INTO usage_rollup (
                day, source, model, provider, project, session_id, has_native,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                reasoning_tokens, total_tokens, native_cost, record_count,
                first_at, last_at, file_key
            )
            SELECT
                substr(r.occurred_at, 1, 10),
                r.source, r.model, r.provider, r.project, r.session_id,
                CASE WHEN r.native_cost IS NOT NULL THEN 1 ELSE 0 END,
                SUM(r.input_tokens), SUM(r.output_tokens), SUM(r.cache_read_tokens),
                SUM(r.cache_creation_tokens), SUM(r.reasoning_tokens), SUM(r.total_tokens),
                COALESCE(SUM(r.native_cost), 0),
                COUNT(*),
                MIN(r.occurred_at), MAX(r.occurred_at),
                MAX(CASE WHEN r.source_file != '' THEN r.occurred_at || char(31) || r.source_file END)
            FROM usage_records r
            GROUP BY 1, 2, 3, 4, 5, 6, 7
            "#,
            [],
        )
        .map_err(|e| e.to_string())?;
    Ok(written as u64)
}

/// 写入前把 `model` 归一化成小写，与 `install_prices` 装载的价目表同口径。
/// 有了这个不变量，价格 JOIN 才能直接比较列值，不必对 17 万行逐行调 `lower()`。
/// `provider` 不动：历史值里有 `cpaApi` 这类混合大小写，归一化会改到界面上的显示。
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

pub fn upsert_official_quota(
    conn: &Connection,
    provider: &str,
    windows: &[OfficialQuotaWindow],
    captured_at: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let windows_json = serde_json::to_string(windows).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO official_quota(provider, windows_json, captured_at, error)
         VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(provider) DO UPDATE SET
            windows_json = excluded.windows_json,
            captured_at = excluded.captured_at,
            error = excluded.error",
        params![provider, windows_json, captured_at, error],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_official_quota_error(
    conn: &Connection,
    provider: &str,
    error: &str,
) -> Result<(), String> {
    let updated = conn
        .execute(
            "UPDATE official_quota SET error = ?2 WHERE provider = ?1",
            params![provider, error],
        )
        .map_err(|e| e.to_string())?;
    if updated == 0 {
        conn.execute(
            "INSERT INTO official_quota(provider, windows_json, captured_at, error)
             VALUES(?1, '[]', '', ?2)",
            params![provider, error],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

type OfficialQuotaRow = (Vec<OfficialQuotaWindow>, String, Option<String>);

pub fn load_official_quota_row(
    conn: &Connection,
    provider: &str,
) -> Result<Option<OfficialQuotaRow>, String> {
    let row = conn
        .query_row(
            "SELECT windows_json, captured_at, error FROM official_quota WHERE provider = ?1",
            params![provider],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((windows_json, captured_at, error)) = row else {
        return Ok(None);
    };
    let windows: Vec<OfficialQuotaWindow> =
        serde_json::from_str(&windows_json).map_err(|e| format!("官方额度缓存损坏：{e}"))?;
    Ok(Some((windows, captured_at, error)))
}

pub fn cursor_session_has_source_file(conn: &Connection, path: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT 1 FROM cursor_sessions WHERE source_file = ?1",
        params![path],
        |_| Ok(()),
    )
    .optional()
    .map(|row| row.is_some())
    .map_err(|e| e.to_string())
}

pub fn cached_cursor_session_file_stats(
    conn: &Connection,
) -> Result<Vec<(String, i64, i64)>, String> {
    let mut stmt = conn
        .prepare("SELECT path, mtime_ms, size FROM cursor_session_files")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn cursor_session_file_fingerprint(
    conn: &Connection,
    path: &str,
) -> Result<Option<(i64, i64)>, String> {
    conn.query_row(
        "SELECT mtime_ms, size FROM cursor_session_files WHERE path = ?1",
        params![path],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn upsert_cursor_session(
    conn: &Connection,
    record: &CursorSessionRecord,
) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO cursor_sessions (
            source_file, session_id, project, turn_count, success_count, error_count, aborted_count,
            user_prompt_count, subagent_count, tool_calls_json, models_json, sources_json,
            extensions_json, first_seen_at, last_seen_at, files_touched
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
        ON CONFLICT(source_file) DO UPDATE SET
            session_id = excluded.session_id,
            project = excluded.project,
            turn_count = excluded.turn_count,
            success_count = excluded.success_count,
            error_count = excluded.error_count,
            aborted_count = excluded.aborted_count,
            user_prompt_count = excluded.user_prompt_count,
            subagent_count = excluded.subagent_count,
            tool_calls_json = excluded.tool_calls_json,
            models_json = excluded.models_json,
            sources_json = excluded.sources_json,
            extensions_json = excluded.extensions_json,
            first_seen_at = COALESCE(cursor_sessions.first_seen_at, excluded.first_seen_at),
            last_seen_at = excluded.last_seen_at,
            files_touched = excluded.files_touched
        "#,
        params![
            record.source_file,
            record.session_id,
            record.project,
            record.turn_count,
            record.success_count,
            record.error_count,
            record.aborted_count,
            record.user_prompt_count,
            record.subagent_count,
            record.tool_calls_json,
            record.models_json,
            record.sources_json,
            record.extensions_json,
            record.first_seen_at,
            record.last_seen_at,
            record.files_touched,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn upsert_cursor_session_file(
    conn: &Connection,
    path: &str,
    mtime_ms: i64,
    size: i64,
) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO cursor_session_files(path, mtime_ms, size) VALUES(?1,?2,?3)
        ON CONFLICT(path) DO UPDATE SET mtime_ms = excluded.mtime_ms, size = excluded.size
        "#,
        params![path, mtime_ms, size],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_cursor_sessions(conn: &Connection) -> Result<Vec<CursorSessionRecord>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT source_file, session_id, project, turn_count, success_count, error_count, aborted_count,
                   user_prompt_count, subagent_count, tool_calls_json, models_json, sources_json,
                   extensions_json, first_seen_at, last_seen_at, files_touched
            FROM cursor_sessions
            ORDER BY last_seen_at ASC, source_file ASC
            "#,
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CursorSessionRecord {
                source_file: row.get(0)?,
                session_id: row.get(1)?,
                project: row.get(2)?,
                turn_count: row.get(3)?,
                success_count: row.get(4)?,
                error_count: row.get(5)?,
                aborted_count: row.get(6)?,
                user_prompt_count: row.get(7)?,
                subagent_count: row.get(8)?,
                tool_calls_json: row.get(9)?,
                models_json: row.get(10)?,
                sources_json: row.get(11)?,
                extensions_json: row.get(12)?,
                first_seen_at: row.get(13)?,
                last_seen_at: row.get(14)?,
                files_touched: row.get(15)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn load_cursor_session(
    conn: &Connection,
    source_file: &str,
) -> Result<Option<CursorSessionRecord>, String> {
    conn.query_row(
        r#"
        SELECT source_file, session_id, project, turn_count, success_count, error_count, aborted_count,
               user_prompt_count, subagent_count, tool_calls_json, models_json, sources_json,
               extensions_json, first_seen_at, last_seen_at, files_touched
        FROM cursor_sessions
        WHERE source_file = ?1
        "#,
        params![source_file],
        |row| {
            Ok(CursorSessionRecord {
                source_file: row.get(0)?,
                session_id: row.get(1)?,
                project: row.get(2)?,
                turn_count: row.get(3)?,
                success_count: row.get(4)?,
                error_count: row.get(5)?,
                aborted_count: row.get(6)?,
                user_prompt_count: row.get(7)?,
                subagent_count: row.get(8)?,
                tool_calls_json: row.get(9)?,
                models_json: row.get(10)?,
                sources_json: row.get(11)?,
                extensions_json: row.get(12)?,
                first_seen_at: row.get(13)?,
                last_seen_at: row.get(14)?,
                files_touched: row.get(15)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn reconcile_cursor_sessions(
    conn: &Connection,
    seen_paths: &BTreeSet<String>,
) -> Result<u64, String> {
    let cached: Vec<String> = conn
        .prepare("SELECT source_file FROM cursor_sessions")
        .map_err(|e| e.to_string())?
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut removed = 0u64;
    for path in cached {
        if seen_paths.contains(&path) {
            continue;
        }
        conn.execute(
            "DELETE FROM cursor_sessions WHERE source_file = ?1",
            params![path],
        )
        .map_err(|e| e.to_string())?;
        removed += 1;
    }
    Ok(removed)
}

pub fn reconcile_cursor_session_files(
    conn: &Connection,
    seen_paths: &BTreeSet<String>,
) -> Result<u64, String> {
    let cached: Vec<String> = conn
        .prepare("SELECT path FROM cursor_session_files")
        .map_err(|e| e.to_string())?
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut removed = 0u64;
    for path in cached {
        if seen_paths.contains(&path) {
            continue;
        }
        conn.execute(
            "DELETE FROM cursor_session_files WHERE path = ?1",
            params![path],
        )
        .map_err(|e| e.to_string())?;
        removed += 1;
    }
    Ok(removed)
}

pub const CURSOR_SESSION_SCHEMA_VERSION: &str = "2";

pub fn cursor_session_schema_version(conn: &Connection) -> Result<String, String> {
    conn.query_row(
        "SELECT value FROM cursor_session_meta WHERE key = 'schema_version'",
        [],
        |row| row.get(0),
    )
    .optional()
    .map(|value| value.unwrap_or_default())
    .map_err(|e| e.to_string())
}

pub fn set_cursor_session_schema_version(conn: &Connection, version: &str) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO cursor_session_meta(key, value) VALUES('schema_version', ?1)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
        params![version],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_cursor_session_as_of(conn: &Connection, as_of: &str) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO cursor_session_meta(key, value) VALUES('as_of', ?1)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
        params![as_of],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn cursor_session_as_of(conn: &Connection) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM cursor_session_meta WHERE key = 'as_of'",
        [],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn set_cursor_tracking_fingerprint(conn: &Connection, fingerprint: &str) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO cursor_session_meta(key, value) VALUES('tracking_fingerprint', ?1)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
        params![fingerprint],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn cursor_tracking_fingerprint(conn: &Connection) -> Result<String, String> {
    conn.query_row(
        "SELECT value FROM cursor_session_meta WHERE key = 'tracking_fingerprint'",
        [],
        |row| row.get(0),
    )
    .optional()
    .map(|value| value.unwrap_or_default())
    .map_err(|e| e.to_string())
}
