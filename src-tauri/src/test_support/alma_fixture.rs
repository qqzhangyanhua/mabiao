use std::path::{Path, PathBuf};

const SCHEMA: &str = r#"
CREATE TABLE workspaces (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE chat_threads (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    model TEXT,
    metadata TEXT DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    is_incognito INTEGER DEFAULT 0,
    workspace_id TEXT,
    parent_thread_id TEXT
);
CREATE TABLE usage_records (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    model TEXT,
    provider_id TEXT,
    date TEXT NOT NULL,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cached_input_tokens INTEGER DEFAULT 0,
    cache_write_input_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    total_tokens INTEGER DEFAULT 0,
    timestamp TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE aux_usage_records (
    id TEXT PRIMARY KEY,
    purpose TEXT NOT NULL,
    model TEXT,
    provider_id TEXT,
    thread_id TEXT,
    date TEXT NOT NULL,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cached_input_tokens INTEGER DEFAULT 0,
    cache_write_input_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    total_tokens INTEGER DEFAULT 0,
    timestamp TEXT NOT NULL,
    created_at TEXT NOT NULL
);
"#;

/// 全量摄取夹具：Linux 默认根 `.config/alma/chat_threads.db`，两行可入账 usage。
pub fn write_alma_ingest_db(home: &Path) -> PathBuf {
    let path = home.join(".config/alma/chat_threads.db");
    write_alma_db(&path, false);
    path
}

/// 适配器夹具：可入账行 + 隐身 / cron / 空会话，用来断言跳过规则。
pub fn write_alma_full_db(path: &Path) -> PathBuf {
    write_alma_db(path, true);
    path.to_path_buf()
}

fn write_alma_db(path: &Path, include_skipped: bool) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create alma fixture dir");
    }
    let db = rusqlite::Connection::open(path).expect("open alma fixture db");
    db.execute_batch(SCHEMA)
        .expect("create alma fixture tables");
    db.execute(
        "INSERT INTO workspaces VALUES ('ws1', '/work/alma', 'alma', '2026-10-01T09:00:00.000Z', '2026-10-01T09:00:00.000Z')",
        [],
    )
    .expect("insert alma workspace");
    db.execute(
        "INSERT INTO chat_threads (id, title, model, created_at, updated_at, workspace_id) VALUES
         ('thA', 'Fix the build', 'claude-subscription:claude-opus-5-5', '2026-10-01T10:00:00.000Z', '2026-10-01T10:00:30.000Z', 'ws1')",
        [],
    )
    .expect("insert alma thread A");
    db.execute(
        "INSERT INTO usage_records (id, message_id, thread_id, model, provider_id, date, input_tokens, output_tokens, cached_input_tokens, cache_write_input_tokens, reasoning_tokens, total_tokens, timestamp, created_at) VALUES
         ('u1', 'thA--a1', 'thA', 'plugin:openai-codex-auth:openai-codex:gpt-6-astra', 'plugin', '2026-10-01', 1000, 50, 600, 0, 10, 1050, '2026-10-01T10:00:05.000Z', '2026-10-01T10:00:05.000Z'),
         ('u2', 'thA--a1', 'thA', 'claude-subscription:claude-opus-5-5', 'claude-subscription', '2026-10-01', 500, 20, 100, 300, 0, 520, '2026-10-01T10:00:06.000Z', '2026-10-01T10:00:06.000Z')",
        [],
    )
    .expect("insert alma usage");
    if !include_skipped {
        return;
    }
    db.execute(
        "INSERT INTO chat_threads (id, title, model, created_at, updated_at) VALUES
         ('thB', 'New Chat', 'gemini:gemini-3.1-pro-preview', '2026-10-01T11:00:00.000Z', '2026-10-01T11:00:00.000Z'),
         ('thC', 'New Chat', NULL, '2026-10-01T12:00:00.000Z', '2026-10-01T12:00:00.000Z')",
        [],
    )
    .expect("insert alma named/empty threads");
    db.execute(
        "INSERT INTO chat_threads (id, title, created_at, updated_at, is_incognito) VALUES
         ('thD', 'Secret', '2026-10-01T13:00:00.000Z', '2026-10-01T13:00:00.000Z', 1)",
        [],
    )
    .expect("insert alma incognito thread");
    db.execute(
        "INSERT INTO chat_threads (id, title, created_at, updated_at, metadata) VALUES
         ('thE', 'Daily digest', '2026-10-01T14:00:00.000Z', '2026-10-01T14:00:00.000Z', '{\"isCron\":true}'),
         ('thF', '⏰ Cron: daily digest (Retry 1)', '2026-10-01T15:00:00.000Z', '2026-10-01T15:00:00.000Z', '{}')",
        [],
    )
    .expect("insert alma cron threads");
    db.execute(
        "INSERT INTO aux_usage_records (id, purpose, model, provider_id, thread_id, date, input_tokens, output_tokens, timestamp, created_at) VALUES
         ('x1', 'title', 'gemini:gemini-3.1-flash-lite-preview', 'gemini', 'thB', '2026-10-01', 30, 5, '2026-10-01T11:00:02.000Z', '2026-10-01T11:00:02.000Z')",
        [],
    )
    .expect("insert alma aux usage");
    db.execute(
        "INSERT INTO usage_records (id, message_id, thread_id, model, provider_id, date, input_tokens, output_tokens, timestamp, created_at) VALUES
         ('skip-d', 'thD--a', 'thD', 'gemini:hidden', 'gemini', '2026-10-01', 9, 1, '2026-10-01T13:00:02.000Z', '2026-10-01T13:00:02.000Z'),
         ('skip-e', 'thE--a', 'thE', 'gemini:cron', 'gemini', '2026-10-01', 9, 1, '2026-10-01T14:00:02.000Z', '2026-10-01T14:00:02.000Z'),
         ('skip-f', 'thF--a', 'thF', 'gemini:cron-title', 'gemini', '2026-10-01', 9, 1, '2026-10-01T15:00:02.000Z', '2026-10-01T15:00:02.000Z')",
        [],
    )
    .expect("insert alma skipped usage");
}
