use std::path::{Path, PathBuf};

const ROOT_USAGE: &str = r#"{
  "server_conversation_token": "srv-warp-1",
  "conversation_usage_metadata": {
    "was_summarized": false,
    "context_window_usage": 0.12,
    "credits_spent": 1.5,
    "token_usage": [
      {
        "model_id": "claude-4-5-sonnet",
        "warp_tokens": 250,
        "byok_tokens": 0,
        "custom_endpoint_tokens": 0
      }
    ]
  }
}"#;

const EMPTY_USAGE: &str = r#"{
  "conversation_usage_metadata": {
    "was_summarized": false,
    "context_window_usage": 0.0,
    "credits_spent": 0.0,
    "token_usage": []
  }
}"#;

const LEGACY_USAGE: &str = r#"{
  "conversation_usage_metadata": {
    "was_summarized": false,
    "context_window_usage": 0.0,
    "credits_spent": 0.0,
    "token_usage": [
      {
        "model_id": "",
        "total_tokens": 99
      }
    ]
  }
}"#;

/// 在 `path` 建一份含 `agent_conversations` 的 mini `warp.sqlite`。
pub fn write_warp_sqlite(path: &Path) -> PathBuf {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create warp fixture dir");
    }
    let db = rusqlite::Connection::open(path).expect("open warp fixture db");
    db.execute_batch(
        r#"
        CREATE TABLE agent_conversations (
            id INTEGER PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            conversation_data TEXT NOT NULL,
            last_modified_at TIMESTAMP NOT NULL,
            summary TEXT
        );
        "#,
    )
    .expect("create warp fixture table");
    db.execute(
        "INSERT INTO agent_conversations (conversation_id, conversation_data, last_modified_at, summary)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            "545ac14f-66fc-41c6-baa3-b0c114c00607",
            ROOT_USAGE,
            "2026-10-17 08:05:00",
            "Inspect Warp"
        ],
    )
    .expect("insert warp billed conversation");
    db.execute(
        "INSERT INTO agent_conversations (conversation_id, conversation_data, last_modified_at, summary)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            "empty-session",
            EMPTY_USAGE,
            "2026-10-17 08:06:00",
            "empty"
        ],
    )
    .expect("insert warp empty conversation");
    db.execute(
        "INSERT INTO agent_conversations (conversation_id, conversation_data, last_modified_at, summary)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            "redacted-session",
            LEGACY_USAGE,
            "2026-10-17 08:07:00",
            "no model"
        ],
    )
    .expect("insert warp model-less conversation");
    path.to_path_buf()
}

pub fn write_default_warp_home(home: &Path) -> PathBuf {
    write_warp_sqlite(&home.join(".local/state/warp-terminal/warp.sqlite"))
}
