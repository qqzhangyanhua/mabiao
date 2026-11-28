use crate::conversation;
use crate::domain::{ConversationEventAnchor, ConversationEventKind, ConversationQuery};
use crate::test_support::*;

fn seed_alma_conversation(home: &std::path::Path) -> std::path::PathBuf {
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
    path
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

#[test]
fn alma_conversation_refresh_detects_uncheckpointed_wal_writes() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let db_path = seed_alma_conversation(home);

    // 切到 WAL 且关闭自动 checkpoint，保持写连接打开，让后续写入只落 -wal。
    let writer = rusqlite::Connection::open(&db_path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();

    let conn = store::open_memory().unwrap();
    let first = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(first.files_failed, 0, "{first:?}");
    let before = conversation::load_events(
        &conn,
        home,
        "alma",
        "thA",
        ConversationEventAnchor::First,
        50,
    )
    .unwrap();
    let before_messages: Vec<&str> = before
        .events
        .iter()
        .filter(|event| event.kind == ConversationEventKind::Message)
        .filter_map(|event| event.text.as_deref())
        .collect();
    assert_eq!(before_messages.len(), 2);

    // 写入新轮次到 -wal（主库文件保持不动）。
    writer
        .execute(
            "INSERT INTO chat_messages VALUES(?1, ?2, ?3, ?4, '{}', NULL)",
            rusqlite::params![
                "m3",
                "thA",
                serde_json::json!({"role":"user","parts":[{"type":"text","text":"Follow up in WAL"}]}).to_string(),
                "2026-10-01T10:01:00.000Z"
            ],
        )
        .unwrap();
    let wal_path = std::path::PathBuf::from(format!("{}-wal", db_path.to_string_lossy()));
    assert!(wal_path.exists(), "新写入应保留在未 checkpoint 的 -wal 中");

    // 验证主库确实尚未 checkpoint（immutable=1 仍只看到 2 条消息）。
    let immutable = rusqlite::Connection::open_with_flags(
        format!("file:{}?immutable=1", db_path.to_string_lossy()),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();
    let main_count: i64 = immutable
        .query_row("SELECT COUNT(*) FROM chat_messages", [], |row| row.get(0))
        .unwrap();
    assert_eq!(main_count, 2, "测试前提：m3 必须留在 -wal 未刷进主库");

    let roots = ingest::source_scan_dirs(home, Source::Alma);
    let issues = conversation::refresh_catalog(&conn, Source::Alma, &roots).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    assert!(
        conversation::session_needs_event_backfill(&conn, "alma", "thA").unwrap(),
        "未 checkpoint 的 -wal 写入必须触发 Alma 目录刷新并标记待补建"
    );

    let after = conversation::load_events(
        &conn,
        home,
        "alma",
        "thA",
        ConversationEventAnchor::First,
        50,
    )
    .unwrap();
    let after_messages: Vec<&str> = after
        .events
        .iter()
        .filter(|event| event.kind == ConversationEventKind::Message)
        .filter_map(|event| event.text.as_deref())
        .collect();
    assert!(
        after_messages.contains(&"Follow up in WAL"),
        "刷新后事件读取必须包含未 checkpoint 的 WAL 消息：{after_messages:?}"
    );
}
