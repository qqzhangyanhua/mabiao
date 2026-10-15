use std::path::Path;

use rusqlite::params;

use crate::conversation;
use crate::domain::{ConversationEventKind, ConversationQuery};
use crate::test_support::*;

fn write_text(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn gemini_jsonl_session_honors_rewind_and_feeds_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".gemini/tmp/gemini-project/chats/session-sess-g-jsonl.jsonl",
        "gemini-session-conversation.jsonl",
    );
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "gemini" && row.session_id == "sess-g-jsonl")
        .expect("gemini jsonl session");
    assert_eq!(row.model, "gemini-flash");

    let detail = conversation::load_parsed_detail(&conn, home, "gemini", "sess-g-jsonl").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Inspect the jsonl session".to_string(),
            "Looking at the session now.".to_string()
        ]
    );
    assert!(detail
        .events
        .iter()
        .all(|event| event.text.as_deref() != Some("This later turn is rewound.")));
    assert_conversation_index_matches_parse(&conn, home, "gemini", "sess-g-jsonl");

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("jsonl session".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "gemini" && row.session_id == "sess-g-jsonl"));
}

#[test]
fn codex_jsonl_zst_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = home.join(".codex/sessions/2026/08/rollout-conv-1.jsonl.zst");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let compressed = zstd::encode_all(fixture("codex-conversation.jsonl").as_bytes(), 1).unwrap();
    std::fs::write(&path, compressed).unwrap();
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "codex" && row.session_id == "conv-1")
        .expect("codex zst session");
    assert_eq!(row.project, "/workspace/example-project");
    assert_eq!(row.model, "gpt-5.6-sol");

    let detail = conversation::load_parsed_detail(&conn, home, "codex", "conv-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "发布 Tray 客户端版本支持图片编辑透传".to_string(),
            "我先检查现有实现。".to_string(),
            "已完成提交。".to_string()
        ]
    );
    assert_conversation_index_matches_parse(&conn, home, "codex", "conv-1");
    let referenced: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM conversation_events WHERE text_hash IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(referenced, 0, "压缩会话的正文不得外置（ADR 0025）");

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("Tray".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "codex" && row.session_id == "conv-1"));
}

#[test]
fn factory_calling_session_hides_child_from_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let root = home.join(".factory/sessions/-workspace-project");
    write_text(
        &root.join("parent-id.jsonl"),
        concat!(
            r#"{"type":"session_start","id":"parent-id","title":"Parent task"}"#,
            "\n",
            r#"{"role":"user","timestamp":"2026-08-23T00:00:00Z","content":[{"type":"text","text":"Inspect the parent"}]}"#,
            "\n",
        ),
    );
    write_text(&root.join("parent-id.settings.json"), "{}");
    write_text(
        &root.join("child.jsonl"),
        concat!(
            r#"{"type":"session_start","id":"child","callingSessionId":"parent-id","title":"Scout"}"#,
            "\n",
            r#"{"role":"user","timestamp":"2026-08-23T00:00:01Z","content":[{"type":"text","text":"Scout the tree"}]}"#,
            "\n",
        ),
    );
    write_text(&root.join("child.settings.json"), "{}");
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(page
        .rows
        .iter()
        .any(|row| row.source == "factory" && row.session_id == "parent-id"));
    assert!(page.rows.iter().all(|row| row.session_id != "child"));

    let parent = conversation::load_detail(&conn, home, "factory", "parent-id").unwrap();
    assert!(parent.agent_relations.children.iter().any(|child| {
        child
            .session
            .as_ref()
            .is_some_and(|session| session.session_id == "child")
    }));
}

#[test]
fn opencode_v2_session_feeds_catalog_and_hides_child() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = home.join(".local/share/opencode/opencode.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        r#"
        CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT);
        CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL);
        CREATE TABLE session_v2 (
            id TEXT PRIMARY KEY,
            parent_id TEXT,
            title TEXT,
            directory TEXT,
            time_created INTEGER,
            time_updated INTEGER
        );
        CREATE TABLE session_message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            type TEXT NOT NULL,
            seq INTEGER NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT NOT NULL
        );
        CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        INSERT INTO kv VALUES ('migration.v1-v2', '{"phase":"completed"}');
        INSERT INTO session VALUES ('ses-gone', 'Migrated away');
        INSERT INTO message VALUES (
            'msg-gone',
            'ses-gone',
            '{"role":"assistant","modelID":"old-model"}'
        );
        "#,
    )
    .unwrap();
    db.execute(
        "INSERT INTO session_v2 VALUES(?1, NULL, ?2, ?3, ?4, ?5)",
        params![
            "ses-v2",
            "Flaky test hunt",
            "/work/oc",
            1_790_416_800_000_i64,
            1_790_416_830_000_i64
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO session_v2 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            "ses-child",
            "ses-v2",
            "Scout",
            "/work/oc",
            1_790_416_860_000_i64,
            1_790_416_890_000_i64
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO session_message VALUES(?1, ?2, 'user', 1, ?3, ?3, ?4)",
        params![
            "msg-user",
            "ses-v2",
            1_790_416_805_000_i64,
            serde_json::json!({"text":"Why does the login test flake?"}).to_string()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO session_message VALUES(?1, ?2, 'assistant', 2, ?3, ?4, ?5)",
        params![
            "msg-assistant",
            "ses-v2",
            1_790_416_806_000_i64,
            1_790_416_830_000_i64,
            serde_json::json!({
                "model":{"id":"gpt-6-astra","providerID":"openai"},
                "content":[
                    {"type":"reasoning","text":"check the clock"},
                    {"type":"text","text":"It races the clock."}
                ]
            })
            .to_string()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO session_message VALUES(?1, ?2, 'assistant', 3, ?3, ?3, ?4)",
        params![
            "msg-child",
            "ses-child",
            1_790_416_860_000_i64,
            serde_json::json!({
                "model":{"id":"claude-opus-5-5"},
                "content":[{"type":"text","text":"scouting"}]
            })
            .to_string()
        ],
    )
    .unwrap();
    drop(db);

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "opencode" && row.session_id == "ses-v2")
        .expect("opencode v2 session");
    assert_eq!(row.title, "Flaky test hunt");
    assert_eq!(row.project, "/work/oc");
    assert_eq!(row.model, "gpt-6-astra");
    assert!(page.rows.iter().all(|row| row.session_id != "ses-child"));
    assert!(page.rows.iter().all(|row| row.session_id != "ses-gone"));

    let detail = conversation::load_parsed_detail(&conn, home, "opencode", "ses-v2").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Why does the login test flake?".to_string(),
            "It races the clock.".to_string()
        ]
    );
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == ConversationEventKind::Plan));
    assert_conversation_index_matches_parse(&conn, home, "opencode", "ses-v2");

    let parent = conversation::load_detail(&conn, home, "opencode", "ses-v2").unwrap();
    assert!(parent.agent_relations.children.iter().any(|child| {
        child
            .session
            .as_ref()
            .is_some_and(|session| session.session_id == "ses-child")
    }));
}
