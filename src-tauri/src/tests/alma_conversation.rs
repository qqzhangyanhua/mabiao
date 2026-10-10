use crate::conversation;
use crate::domain::ConversationQuery;
use crate::test_support::*;

fn seed_alma_conversation(home: &std::path::Path) {
    let path = crate::test_support::write_alma_ingest_db(home);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS chat_messages (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            message TEXT NOT NULL,
            timestamp TEXT,
            metadata TEXT DEFAULT '{}',
            parent_tool_call_id TEXT
        );
        "#,
    )
    .unwrap();
    db.execute(
        "INSERT INTO chat_messages VALUES(?1, ?2, ?3, ?4, '{}', NULL)",
        rusqlite::params![
            "m1",
            "thA",
            serde_json::json!({"role":"user","parts":[{"type":"text","text":"Fix the flaky build"}]}).to_string(),
            "2026-10-01T10:00:01.000Z"
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO chat_messages VALUES(?1, ?2, ?3, ?4, '{}', NULL)",
        rusqlite::params![
            "m2",
            "thA",
            serde_json::json!({"role":"assistant","parts":[{"type":"text","text":"I will inspect the test first."},{"type":"tool-Read","toolName":"Read","input":{"path":"src/lib.rs"}}]}).to_string(),
            "2026-10-01T10:00:05.000Z"
        ],
    )
    .unwrap();
}

#[test]
fn alma_thread_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_alma_conversation(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "alma" && row.session_id == "thA")
        .expect("alma session");
    assert_eq!(row.title, "Fix the build");
    assert_eq!(row.project, "/work/alma");

    let detail = conversation::load_parsed_detail(&conn, home, "alma", "thA").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Fix the flaky build".to_string(),
            "I will inspect the test first.".to_string()
        ]
    );
    assert!(detail
        .events
        .iter()
        .any(|event| event.name.as_deref() == Some("Read")));
    assert_conversation_index_matches_parse(&conn, home, "alma", "thA");

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("flaky".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "alma" && row.session_id == "thA"));
}
