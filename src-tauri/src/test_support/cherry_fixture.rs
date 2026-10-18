use std::path::{Path, PathBuf};

pub const CHERRY_TOPIC_ID: &str = "topic-cherry-1";
pub const CHERRY_TOKENS: i64 = 430;
pub const CHERRY_CREATED_AT_MS: i64 = 1_771_653_600_000;

const USER_DATA: &str = r#"{"parts":[{"type":"text","text":"Draft the cherry notes"}]}"#;
const ASSISTANT_DATA: &str = r#"{"parts":[{"type":"reasoning","text":"planning"},{"type":"text","text":"Notes drafted."},{"type":"tool-call","toolCallId":"call-1","toolName":"read_file","input":{"path":"README.md"}},{"type":"tool-result","toolCallId":"call-1","output":"readme"}]}"#;

/// 在 `path` 建一份含 `ai_usage_record` + `topic` + `message` 的 mini `cherrystudio.sqlite`。
pub fn write_cherry_studio_db(path: &Path) -> PathBuf {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create cherry fixture dir");
    }
    let db = rusqlite::Connection::open(path).expect("open cherry fixture db");
    db.execute_batch(
        r#"
        CREATE TABLE ai_usage_record (
            id TEXT PRIMARY KEY,
            request_id TEXT NOT NULL,
            record_kind TEXT NOT NULL,
            request_count INTEGER NOT NULL,
            message_kind TEXT,
            message_id TEXT,
            provider_id TEXT,
            provider_name TEXT,
            model_id TEXT,
            model_name TEXT,
            source_type TEXT,
            source_id TEXT,
            source_name TEXT,
            modality TEXT NOT NULL,
            api_key_attribution TEXT NOT NULL,
            input_tokens INTEGER,
            output_tokens INTEGER,
            total_tokens INTEGER,
            reasoning_tokens INTEGER,
            no_cache_tokens INTEGER,
            cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,
            cost REAL,
            cost_currency TEXT,
            cost_source TEXT,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE topic (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL DEFAULT '',
            last_activity_at INTEGER NOT NULL,
            created_at INTEGER,
            deleted_at INTEGER
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY,
            parent_id TEXT,
            topic_id TEXT NOT NULL,
            role TEXT NOT NULL,
            data TEXT NOT NULL,
            searchable_text TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL,
            created_at INTEGER,
            deleted_at INTEGER
        );
        "#,
    )
    .expect("create cherry fixture tables");
    db.execute(
        r#"
        INSERT INTO ai_usage_record (
            id, request_id, record_kind, request_count, message_kind, message_id,
            provider_id, provider_name, model_id, model_name, source_name, modality,
            api_key_attribution, no_cache_tokens, output_tokens, cache_read_tokens,
            cache_write_tokens, reasoning_tokens, cost, cost_currency, cost_source, created_at
        ) VALUES (
            'u1', 'req-1', 'invocation', 1, 'chat', 'msg-a',
            'anthropic', 'Anthropic', 'claude-sonnet-4', 'Claude Sonnet 4', '/work/cherry', 'language',
            'unknown', 100, 50, 200, 40, 10, 0.02, 'USD', 'provider', ?1
        )
        "#,
        rusqlite::params![CHERRY_CREATED_AT_MS],
    )
    .expect("insert cherry usd usage");
    db.execute(
        r#"
        INSERT INTO ai_usage_record (
            id, request_id, record_kind, request_count, message_kind, message_id,
            provider_id, model_id, modality, api_key_attribution,
            input_tokens, output_tokens, cost, cost_currency, cost_source, created_at
        ) VALUES (
            'u2', 'req-2', 'invocation', 1, 'chat', 'msg-b',
            'openai', 'gpt-4.1', 'language', 'unknown',
            20, 10, 1.0, 'CNY', 'provider', ?1
        )
        "#,
        rusqlite::params![CHERRY_CREATED_AT_MS + 60_000],
    )
    .expect("insert cherry cny usage");
    db.execute(
        r#"
        INSERT INTO ai_usage_record (
            id, request_id, record_kind, request_count, modality, api_key_attribution,
            input_tokens, output_tokens, created_at
        ) VALUES (
            'u3', 'req-3', 'invocation', 1, 'language', 'unknown',
            9, 1, ?1
        )
        "#,
        rusqlite::params![CHERRY_CREATED_AT_MS],
    )
    .expect("insert cherry usage missing model");
    db.execute(
        r#"
        INSERT INTO topic (id, name, last_activity_at, created_at)
        VALUES (?1, 'Inspect Cherry', ?2, ?2)
        "#,
        rusqlite::params![CHERRY_TOPIC_ID, CHERRY_CREATED_AT_MS],
    )
    .expect("insert cherry topic");
    db.execute(
        "INSERT INTO message (id, parent_id, topic_id, role, data, searchable_text, status, created_at)
         VALUES ('root-1', NULL, ?1, 'root', '{\"parts\":[]}', '', 'success', ?2)",
        rusqlite::params![CHERRY_TOPIC_ID, CHERRY_CREATED_AT_MS],
    )
    .expect("insert cherry root message");
    db.execute(
        "INSERT INTO message (id, parent_id, topic_id, role, data, searchable_text, status, created_at)
         VALUES ('msg-user', 'root-1', ?1, 'user', ?2, 'Draft the cherry notes', 'success', ?3)",
        rusqlite::params![CHERRY_TOPIC_ID, USER_DATA, CHERRY_CREATED_AT_MS],
    )
    .expect("insert cherry user message");
    db.execute(
        "INSERT INTO message (id, parent_id, topic_id, role, data, searchable_text, status, created_at)
         VALUES ('msg-a', 'msg-user', ?1, 'assistant', ?2, 'Notes drafted.', 'success', ?3)",
        rusqlite::params![CHERRY_TOPIC_ID, ASSISTANT_DATA, CHERRY_CREATED_AT_MS + 5_000],
    )
    .expect("insert cherry assistant message");
    path.to_path_buf()
}

pub fn write_default_cherry_home(home: &Path) -> PathBuf {
    write_cherry_studio_db(&home.join(".config/CherryStudio/cherrystudio.sqlite"))
}
