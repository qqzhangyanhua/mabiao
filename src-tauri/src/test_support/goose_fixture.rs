use std::path::{Path, PathBuf};

const USER_MESSAGE: &str = r#"[{"type":"text","text":"Draft the goose notes"}]"#;
const ASSISTANT_MESSAGE: &str = r#"[{"type":"text","text":"Notes drafted."}]"#;
const TOOL_REQUEST: &str = r#"[{"type":"toolRequest","id":"call-1","toolCall":{"value":{"name":"read_file","arguments":{"path":"README.md"}}}}]"#;
const TOOL_RESPONSE: &str = r#"[{"type":"toolResponse","id":"call-1","toolResult":{"value":{"content":[{"type":"text","text":"readme"}],"isError":false}}}]"#;

/// 在 `path` 建一份含 `sessions` + `messages` 的 mini `sessions.db`。
pub fn write_goose_sessions_db(path: &Path) -> PathBuf {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create goose fixture dir");
    }
    let db = rusqlite::Connection::open(path).expect("open goose fixture db");
    db.execute_batch(
        r#"
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL DEFAULT '',
            session_type TEXT NOT NULL DEFAULT 'user',
            working_dir TEXT NOT NULL DEFAULT '',
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            total_tokens INTEGER,
            input_tokens INTEGER,
            output_tokens INTEGER,
            accumulated_total_tokens INTEGER,
            accumulated_input_tokens INTEGER,
            accumulated_output_tokens INTEGER,
            provider_name TEXT,
            model_config_json TEXT
        );
        CREATE TABLE messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            message_id TEXT,
            session_id TEXT NOT NULL REFERENCES sessions(id),
            role TEXT NOT NULL,
            content_json TEXT NOT NULL,
            created_timestamp INTEGER NOT NULL
        );
        "#,
    )
    .expect("create goose fixture tables");
    db.execute(
        r#"
        INSERT INTO sessions (
            id, name, session_type, working_dir, created_at, updated_at,
            accumulated_input_tokens, accumulated_output_tokens, accumulated_total_tokens,
            provider_name, model_config_json
        ) VALUES (
            '20261017_1', 'Inspect Goose', 'user', '/work/goose',
            '2026-10-17 08:00:00', '2026-10-17 08:05:00',
            1000, 200, 1200, 'anthropic',
            '{"model_name":"claude-sonnet-4-20250514","context_limit":200000}'
        )
        "#,
        [],
    )
    .expect("insert goose user session");
    db.execute(
        r#"
        INSERT INTO sessions (
            id, name, session_type, working_dir, created_at, updated_at,
            accumulated_input_tokens, accumulated_output_tokens, accumulated_total_tokens,
            provider_name, model_config_json
        ) VALUES (
            'hidden_1', 'scratch', 'hidden', '/tmp',
            '2026-10-17 09:00:00', '2026-10-17 09:00:01',
            10, 5, 15, 'anthropic',
            '{"model_name":"claude-sonnet-4-20250514"}'
        )
        "#,
        [],
    )
    .expect("insert goose hidden session");
    let rows = [
        ("20261017_1", "user", USER_MESSAGE, 1_771_653_600_i64),
        ("20261017_1", "assistant", TOOL_REQUEST, 1_771_653_601),
        ("20261017_1", "user", TOOL_RESPONSE, 1_771_653_602),
        ("20261017_1", "assistant", ASSISTANT_MESSAGE, 1_771_653_603),
    ];
    for (session_id, role, content, ts) in rows {
        db.execute(
            "INSERT INTO messages (session_id, role, content_json, created_timestamp) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![session_id, role, content, ts],
        )
        .expect("insert goose message");
    }
    path.to_path_buf()
}

pub fn write_default_goose_home(home: &Path) -> PathBuf {
    write_goose_sessions_db(&home.join(".local/share/goose/sessions/sessions.db"))
}
