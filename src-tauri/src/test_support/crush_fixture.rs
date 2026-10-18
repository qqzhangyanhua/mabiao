use std::path::{Path, PathBuf};

const USER_PARTS: &str = r#"[{"type":"text","data":{"text":"Draft the crush notes"}}]"#;
const TOOL_CALL_PARTS: &str = r#"[{"type":"tool_call","data":{"id":"call-1","name":"read_file","input":"{\"path\":\"README.md\"}","finished":true}}]"#;
const TOOL_RESULT_PARTS: &str = r#"[{"type":"tool_result","data":{"tool_call_id":"call-1","name":"read_file","content":"readme","is_error":false}}]"#;
const ASSISTANT_PARTS: &str = r#"[{"type":"text","data":{"text":"Notes drafted."}},{"type":"reasoning","data":{"thinking":"Keep it short."}}]"#;

/// 在 `path` 建一份含 `sessions` + `messages` 的 mini `crush.db`。
pub fn write_crush_db(path: &Path) -> PathBuf {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create crush fixture dir");
    }
    let db = rusqlite::Connection::open(path).expect("open crush fixture db");
    db.execute_batch(
        r#"
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            parent_session_id TEXT,
            title TEXT NOT NULL,
            message_count INTEGER NOT NULL DEFAULT 0,
            prompt_tokens INTEGER NOT NULL DEFAULT 0,
            completion_tokens INTEGER NOT NULL DEFAULT 0,
            cost REAL NOT NULL DEFAULT 0.0,
            updated_at INTEGER NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE messages (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL,
            parts TEXT NOT NULL DEFAULT '[]',
            model TEXT,
            provider TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        "#,
    )
    .expect("create crush fixture tables");
    db.execute(
        r#"
        INSERT INTO sessions (
            id, parent_session_id, title, prompt_tokens, completion_tokens, cost,
            created_at, updated_at
        ) VALUES (
            'sess-crush-1', NULL, 'Inspect Crush', 200, 50, 0.012,
            1771653600, 1771653900
        )
        "#,
        [],
    )
    .expect("insert crush root session");
    db.execute(
        r#"
        INSERT INTO sessions (
            id, parent_session_id, title, prompt_tokens, completion_tokens, cost,
            created_at, updated_at
        ) VALUES (
            'task-child', 'sess-crush-1', 'subagent', 30, 10, 0.003,
            1771653700, 1771653800
        )
        "#,
        [],
    )
    .expect("insert crush child session");
    let rows = [
        (
            "msg-user",
            "sess-crush-1",
            "user",
            USER_PARTS,
            "",
            "",
            1_771_653_600_i64,
        ),
        (
            "msg-tool-call",
            "sess-crush-1",
            "assistant",
            TOOL_CALL_PARTS,
            "claude-sonnet-4-20250514",
            "anthropic",
            1_771_653_601,
        ),
        (
            "msg-tool-result",
            "sess-crush-1",
            "tool",
            TOOL_RESULT_PARTS,
            "",
            "",
            1_771_653_602,
        ),
        (
            "msg-assistant",
            "sess-crush-1",
            "assistant",
            ASSISTANT_PARTS,
            "claude-sonnet-4-20250514",
            "anthropic",
            1_771_653_603,
        ),
    ];
    for (id, session_id, role, parts, model, provider, ts) in rows {
        db.execute(
            "INSERT INTO messages (id, session_id, role, parts, model, provider, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            rusqlite::params![id, session_id, role, parts, model, provider, ts],
        )
        .expect("insert crush message");
    }
    path.to_path_buf()
}

pub fn write_default_crush_home(home: &Path) -> PathBuf {
    let cwd = home.join("work/crush-demo");
    let data_dir = cwd.join(".crush");
    let db = write_crush_db(&data_dir.join("crush.db"));
    let registry = home.join(".local/share/crush");
    std::fs::create_dir_all(&registry).expect("create crush registry dir");
    let projects = serde_json::json!({
        "projects": [{
            "path": cwd.to_string_lossy(),
            "data_dir": data_dir.to_string_lossy(),
            "last_accessed": "2026-10-17T08:00:00Z"
        }]
    });
    std::fs::write(
        registry.join("projects.json"),
        serde_json::to_vec_pretty(&projects).expect("serialize crush projects.json"),
    )
    .expect("write crush projects.json");
    db
}
