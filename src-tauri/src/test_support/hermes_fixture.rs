use std::path::{Path, PathBuf};

pub struct HermesSessionRow {
    pub id: String,
    pub started_at: f64,
    pub cwd: String,
    pub git_repo_root: String,
}

pub struct HermesModelUsageRow {
    pub session_id: String,
    pub model: String,
    pub billing_provider: String,
    pub billing_base_url: String,
    pub billing_mode: String,
    pub task: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub actual_cost_usd: f64,
    pub cost_source: Option<String>,
}

/// 在 `path` 建一份只含 `sessions` + `session_model_usage` 的 mini `state.db`。
pub fn write_hermes_state_db(
    path: &Path,
    sessions: &[HermesSessionRow],
    usage: &[HermesModelUsageRow],
) -> PathBuf {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create hermes fixture dir");
    }
    let db = rusqlite::Connection::open(path).expect("open hermes fixture db");
    db.execute_batch(
        r#"
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            source TEXT NOT NULL DEFAULT 'cli',
            started_at REAL NOT NULL,
            cwd TEXT,
            git_repo_root TEXT
        );
        CREATE TABLE session_model_usage (
            session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            model TEXT NOT NULL,
            billing_provider TEXT NOT NULL DEFAULT '',
            billing_base_url TEXT NOT NULL DEFAULT '',
            billing_mode TEXT NOT NULL DEFAULT '',
            task TEXT NOT NULL DEFAULT '',
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            cache_write_tokens INTEGER NOT NULL DEFAULT 0,
            reasoning_tokens INTEGER NOT NULL DEFAULT 0,
            estimated_cost_usd REAL NOT NULL DEFAULT 0,
            actual_cost_usd REAL NOT NULL DEFAULT 0,
            cost_status TEXT,
            cost_source TEXT,
            PRIMARY KEY (session_id, model, billing_provider, billing_base_url, billing_mode, task)
        );
        "#,
    )
    .expect("create hermes fixture tables");
    for session in sessions {
        db.execute(
            r#"
            INSERT INTO sessions (id, started_at, cwd, git_repo_root)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            rusqlite::params![
                session.id,
                session.started_at,
                session.cwd,
                session.git_repo_root
            ],
        )
        .expect("insert hermes session");
    }
    for row in usage {
        db.execute(
            r#"
            INSERT INTO session_model_usage (
                session_id, model, billing_provider, billing_base_url, billing_mode, task,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, actual_cost_usd, cost_source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            rusqlite::params![
                row.session_id,
                row.model,
                row.billing_provider,
                row.billing_base_url,
                row.billing_mode,
                row.task,
                row.input_tokens,
                row.output_tokens,
                row.cache_read_tokens,
                row.cache_write_tokens,
                row.reasoning_tokens,
                row.actual_cost_usd,
                row.cost_source,
            ],
        )
        .expect("insert hermes model usage");
    }
    path.to_path_buf()
}

/// 覆盖：原生费用、`cost_source = none`、同 session 多模型、cwd 空回退 git_repo_root。
pub fn write_default_hermes_home(home: &Path) -> PathBuf {
    write_hermes_state_db(
        &home.join(".hermes/state.db"),
        &[
            HermesSessionRow {
                id: "sess-multi".into(),
                started_at: 1_775_376_000.0,
                cwd: "/Users/dev/app".into(),
                git_repo_root: "/Users/dev/app".into(),
            },
            HermesSessionRow {
                id: "sess-repo".into(),
                started_at: 1_775_462_400.0,
                cwd: String::new(),
                git_repo_root: "/Users/dev/other".into(),
            },
        ],
        &[
            HermesModelUsageRow {
                session_id: "sess-multi".into(),
                model: "gpt-5.6".into(),
                billing_provider: "custom".into(),
                billing_base_url: "https://example.test/v1".into(),
                billing_mode: String::new(),
                task: String::new(),
                input_tokens: 100,
                output_tokens: 20,
                cache_read_tokens: 10,
                cache_write_tokens: 5,
                reasoning_tokens: 2,
                actual_cost_usd: 0.0123,
                cost_source: Some("actual".into()),
            },
            HermesModelUsageRow {
                session_id: "sess-multi".into(),
                model: "claude-sonnet-5".into(),
                billing_provider: String::new(),
                billing_base_url: String::new(),
                billing_mode: String::new(),
                task: String::new(),
                input_tokens: 50,
                output_tokens: 10,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                actual_cost_usd: 0.0,
                cost_source: Some("none".into()),
            },
            HermesModelUsageRow {
                session_id: "sess-repo".into(),
                model: "gpt-5.6".into(),
                billing_provider: "custom".into(),
                billing_base_url: String::new(),
                billing_mode: String::new(),
                task: String::new(),
                input_tokens: 30,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                actual_cost_usd: 0.05,
                cost_source: Some("actual".into()),
            },
        ],
    )
}
