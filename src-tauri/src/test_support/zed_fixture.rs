use std::path::{Path, PathBuf};

pub const ZED_THREAD_ID: &str = "thread-zed-1";
pub const ZED_TOKENS: i64 = 820;

fn thread_json() -> serde_json::Value {
    serde_json::json!({
        "title": "Inspect Zed",
        "messages": [
            {
                "User": {
                    "id": "um-1",
                    "content": [{"Text": "Draft the zed notes"}]
                }
            },
            {
                "Agent": {
                    "content": [
                        {"Thinking": {"text": "planning", "signature": null}},
                        {"Text": "Notes drafted."},
                        {
                            "ToolUse": {
                                "id": "call-1",
                                "name": "read_file",
                                "input": {"path": "README.md"}
                            }
                        }
                    ],
                    "tool_results": {
                        "call-1": {
                            "tool_use_id": "call-1",
                            "is_error": false,
                            "content": "readme"
                        }
                    }
                }
            }
        ],
        "updated_at": "2026-10-17T08:05:00Z",
        "cumulative_token_usage": {
            "input_tokens": 150,
            "output_tokens": 70,
            "cache_creation_input_tokens": 200,
            "cache_read_input_tokens": 400
        },
        "request_token_usage": {
            "um-1": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_creation_input_tokens": 200,
                "cache_read_input_tokens": 400
            }
        },
        "model": {
            "provider": "anthropic",
            "model": "claude-sonnet-4"
        },
        "initial_project_snapshot": {
            "worktrees": [{"abs_path": "/work/zed"}]
        }
    })
}

/// 在 `path` 建一份含 zstd 线程、明文 JSON 空用量、未知 data_type 的 mini `threads.db`。
pub fn write_zed_threads_db(path: &Path) -> PathBuf {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create zed fixture dir");
    }
    let db = rusqlite::Connection::open(path).expect("open zed fixture db");
    db.execute_batch(
        r#"
        CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            summary TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            data_type TEXT NOT NULL,
            data BLOB NOT NULL,
            parent_id TEXT,
            created_at TEXT
        );
        "#,
    )
    .expect("create zed fixture table");
    let packed =
        zstd::encode_all(thread_json().to_string().as_bytes(), 0).expect("zstd zed thread");
    db.execute(
        r#"
        INSERT INTO threads (id, summary, updated_at, created_at, data_type, data, parent_id)
        VALUES (?1, 'Inspect Zed', '2026-10-17T08:05:00Z', '2026-10-17T08:00:00Z', 'zstd', ?2, NULL)
        "#,
        rusqlite::params![ZED_THREAD_ID, packed],
    )
    .expect("insert zed zstd thread");
    let empty_json = serde_json::json!({
        "title": "empty",
        "messages": [],
        "model": {"model": "claude-sonnet-4"}
    })
    .to_string();
    db.execute(
        r#"
        INSERT INTO threads (id, summary, updated_at, created_at, data_type, data)
        VALUES ('thread-empty', 'empty', '2026-10-17T09:00:00Z', '2026-10-17T09:00:00Z', 'json', ?1)
        "#,
        rusqlite::params![empty_json.as_bytes()],
    )
    .expect("insert zed empty json thread");
    db.execute(
        r#"
        INSERT INTO threads (id, summary, updated_at, created_at, data_type, data)
        VALUES ('thread-unknown', 'skip', '2026-10-17T10:00:00Z', '2026-10-17T10:00:00Z', 'protobuf', x'00')
        "#,
        [],
    )
    .expect("insert zed unknown data_type thread");
    path.to_path_buf()
}

pub fn write_default_zed_home(home: &Path) -> PathBuf {
    write_zed_threads_db(&home.join(".local/share/zed/threads/threads.db"))
}
