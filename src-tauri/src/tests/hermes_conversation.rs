use rusqlite::params;

use crate::conversation;
use crate::domain::{ConversationEventKind, ConversationQuery};
use crate::test_support::*;

const START: f64 = 1_780_000_000.0;

struct Msg<'a> {
    session: &'a str,
    role: &'a str,
    content: &'a str,
    tool_call_id: Option<&'a str>,
    tool_calls: Option<&'a str>,
    reasoning: Option<&'a str>,
    timestamp: f64,
    active: i64,
}

impl<'a> Msg<'a> {
    fn new(session: &'a str, role: &'a str) -> Self {
        Self {
            session,
            role,
            content: "",
            tool_call_id: None,
            tool_calls: None,
            reasoning: None,
            timestamp: START,
            active: 1,
        }
    }
}

fn seed_hermes(home: &std::path::Path) -> std::path::PathBuf {
    let path = write_hermes_state_db(
        &home.join(".hermes/state.db"),
        &[
            HermesSessionRow {
                id: "hs-1".into(),
                started_at: START,
                cwd: "/work/hermes".into(),
                git_repo_root: "/work/hermes".into(),
            },
            HermesSessionRow {
                id: "hs-child".into(),
                started_at: START + 100.0,
                cwd: String::new(),
                git_repo_root: "/work/hermes".into(),
            },
        ],
        &[],
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        r#"
        ALTER TABLE sessions ADD COLUMN title TEXT;
        ALTER TABLE sessions ADD COLUMN model TEXT;
        ALTER TABLE sessions ADD COLUMN parent_session_id TEXT;
        ALTER TABLE sessions ADD COLUMN ended_at REAL;
        UPDATE sessions SET title = 'Inspect Hermes', model = 'hermes-test-model', ended_at = 1780000040.0
            WHERE id = 'hs-1';
        UPDATE sessions SET parent_session_id = 'hs-1' WHERE id = 'hs-child';
        CREATE TABLE messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT,
            tool_call_id TEXT,
            tool_calls TEXT,
            tool_name TEXT,
            timestamp REAL NOT NULL,
            reasoning_content TEXT,
            active INTEGER DEFAULT 1
        );
        "#,
    )
    .unwrap();
    let tool_calls = serde_json::json!([{
        "id": "call-1",
        "call_id": "call-1",
        "type": "function",
        "function": { "name": "terminal", "arguments": "{\"command\":\"ls\"}" }
    }])
    .to_string();
    let messages = [
        Msg {
            content: "Inspect the rollout",
            timestamp: START + 1.0,
            ..Msg::new("hs-1", "user")
        },
        Msg {
            tool_calls: Some(tool_calls.as_str()),
            reasoning: Some("need to look at files"),
            timestamp: START + 2.0,
            ..Msg::new("hs-1", "assistant")
        },
        Msg {
            content: "Cargo.toml",
            tool_call_id: Some("call-1"),
            timestamp: START + 3.0,
            ..Msg::new("hs-1", "tool")
        },
        Msg {
            content: "The rollout looks fine",
            timestamp: START + 4.0,
            ..Msg::new("hs-1", "assistant")
        },
        Msg {
            content: "rewound question",
            timestamp: START + 5.0,
            active: 0,
            ..Msg::new("hs-1", "user")
        },
        Msg {
            content: "child prompt",
            timestamp: START + 101.0,
            ..Msg::new("hs-child", "user")
        },
        Msg {
            content: "child answer",
            timestamp: START + 102.0,
            ..Msg::new("hs-child", "assistant")
        },
    ];
    for message in messages {
        db.execute(
            "INSERT INTO messages (session_id, role, content, tool_call_id, tool_calls, tool_name, timestamp, reasoning_content, active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                message.session,
                message.role,
                message.content,
                message.tool_call_id,
                message.tool_calls,
                (message.role == "tool").then_some("terminal"),
                message.timestamp,
                message.reasoning,
                message.active
            ],
        )
        .unwrap();
    }
    path
}

#[test]
fn hermes_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_hermes(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");
    assert!(report.conversation_issues.is_empty(), "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "hermes" && row.session_id == "hs-1")
        .expect("hermes session");
    assert_eq!(row.title, "Inspect Hermes");
    assert_eq!(row.project, "/work/hermes");
    assert_eq!(row.model, "hermes-test-model");
    assert!(
        !page
            .rows
            .iter()
            .any(|row| row.source == "hermes" && row.session_id == "hs-child"),
        "带 parent_session_id 的会话不是顶层"
    );

    let detail = conversation::load_parsed_detail(&conn, home, "hermes", "hs-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Inspect the rollout".to_string(),
            "The rollout looks fine".to_string()
        ],
        "active = 0 的消息不进对话记录"
    );
    let call = detail
        .events
        .iter()
        .find(|event| event.kind == ConversationEventKind::ToolCall)
        .expect("tool call");
    assert_eq!(call.name.as_deref(), Some("terminal"));
    assert_eq!(call.details["call_id"], "call-1");
    let result = detail
        .events
        .iter()
        .find(|event| event.kind == ConversationEventKind::ToolResult)
        .expect("tool result");
    assert_eq!(result.text.as_deref(), Some("Cargo.toml"));
    assert_eq!(result.details["call_id"], "call-1");
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == ConversationEventKind::Plan
            && event.text.as_deref() == Some("need to look at files")));

    assert_conversation_index_matches_parse(&conn, home, "hermes", "hs-1");

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("rollout".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "hermes" && row.session_id == "hs-1"));
}

#[test]
fn hermes_catalog_picks_up_new_messages_written_only_to_wal() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_hermes(home);
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();

    let source = rusqlite::Connection::open(&path).unwrap();
    source.pragma_update(None, "journal_mode", "WAL").unwrap();
    source
        .execute(
            "INSERT INTO messages (session_id, role, content, timestamp, active)
             VALUES ('hs-1', 'user', 'follow up in wal', ?1, 1)",
            params![START + 10.0],
        )
        .unwrap();

    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let detail = conversation::load_parsed_detail(&conn, home, "hermes", "hs-1").unwrap();
    assert!(message_texts(&detail).contains(&"follow up in wal".to_string()));
}
