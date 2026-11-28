use rusqlite::params;

use crate::conversation;
use crate::domain::{ConversationEventAnchor, ConversationEventKind, ConversationQuery};
use crate::test_support::*;

fn seed_zcode(home: &std::path::Path) -> std::path::PathBuf {
    let path = home.join(".zcode/cli/db/db.sqlite");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        r#"
        CREATE TABLE session (
            id TEXT PRIMARY KEY,
            parent_id TEXT,
            title TEXT,
            title_source TEXT,
            directory TEXT,
            time_created INTEGER,
            time_updated INTEGER
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            time_created INTEGER,
            data TEXT NOT NULL
        );
        CREATE TABLE part (
            id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            time_created INTEGER,
            data TEXT NOT NULL
        );
        "#,
    )
    .unwrap();
    db.execute(
        "INSERT INTO session VALUES(?1, NULL, ?2, 'generated', ?3, ?4, ?5)",
        params![
            "ses-zcode-1",
            "Inspect ZCode",
            "/work/zcode",
            1_780_000_000_000_i64,
            1_780_000_003_000_i64
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO message VALUES(?1, ?2, ?3, ?4)",
        params![
            "msg-user",
            "ses-zcode-1",
            1_780_000_000_000_i64,
            serde_json::json!({"role":"user","semantics":{"origin":"real_user"},"time":{"created":1_780_000_000_000_i64}}).to_string()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO part VALUES(?1, ?2, ?3, ?4, ?5)",
        params![
            "part-user",
            "msg-user",
            "ses-zcode-1",
            1_780_000_000_100_i64,
            serde_json::json!({"type":"text","text":"Inspect the rollout"}).to_string()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO message VALUES(?1, ?2, ?3, ?4)",
        params![
            "msg-assistant",
            "ses-zcode-1",
            1_780_000_001_000_i64,
            serde_json::json!({
                "role":"assistant",
                "modelID":"zcode-test-model",
                "path":{"cwd":"/work/zcode","root":"/work/zcode"},
                "time":{"created":1_780_000_001_000_i64,"completed":1_780_000_003_000_i64}
            })
            .to_string()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO part VALUES(?1, ?2, ?3, ?4, ?5)",
        params![
            "part-assistant",
            "msg-assistant",
            "ses-zcode-1",
            1_780_000_001_100_i64,
            serde_json::json!({"type":"text","text":"The rollout looks fine"}).to_string()
        ],
    )
    .unwrap();
    path
}

#[test]
fn zcode_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_zcode(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "zcode" && row.session_id == "ses-zcode-1")
        .expect("zcode session");
    assert_eq!(row.title, "Inspect ZCode");
    assert_eq!(row.project, "/work/zcode");
    assert_eq!(row.model, "zcode-test-model");

    let detail = conversation::load_parsed_detail(&conn, home, "zcode", "ses-zcode-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Inspect the rollout".to_string(),
            "The rollout looks fine".to_string()
        ]
    );
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == ConversationEventKind::Message));

    assert_conversation_index_matches_parse(&conn, home, "zcode", "ses-zcode-1");

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
        .any(|row| row.source == "zcode" && row.session_id == "ses-zcode-1"));
}

#[test]
fn zcode_conversation_refresh_detects_uncheckpointed_wal_writes() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let db_path = seed_zcode(home);

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
        "zcode",
        "ses-zcode-1",
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
            "INSERT INTO message VALUES(?1, ?2, ?3, ?4)",
            params![
                "msg-wal-user",
                "ses-zcode-1",
                1_780_000_010_000_i64,
                serde_json::json!({"role":"user","semantics":{"origin":"real_user"},"time":{"created":1_780_000_010_000_i64}}).to_string()
            ],
        )
        .unwrap();
    writer
        .execute(
            "INSERT INTO part VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                "part-wal-user",
                "msg-wal-user",
                "ses-zcode-1",
                1_780_000_010_100_i64,
                serde_json::json!({"type":"text","text":"Follow up in ZCode WAL"}).to_string()
            ],
        )
        .unwrap();
    let wal_path = std::path::PathBuf::from(format!("{}-wal", db_path.to_string_lossy()));
    assert!(wal_path.exists(), "新写入应保留在未 checkpoint 的 -wal 中");

    // 验证主库确实尚未 checkpoint（immutable=1 仍只看到 2 条消息）。
    let immutable = rusqlite::Connection::open_with_flags(
        crate::source_sqlite::sqlite_file_uri(&db_path, "immutable=1"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();
    let main_count: i64 = immutable
        .query_row("SELECT COUNT(*) FROM message", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        main_count, 2,
        "测试前提：msg-wal-user 必须留在 -wal 未刷进主库"
    );

    let roots = ingest::source_scan_dirs(home, Source::Zcode);
    let issues = conversation::refresh_catalog(&conn, Source::Zcode, &roots).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    assert!(
        conversation::session_needs_event_backfill(&conn, "zcode", "ses-zcode-1").unwrap(),
        "未 checkpoint 的 -wal 写入必须触发 ZCode 目录刷新并标记待补建"
    );

    let after = conversation::load_events(
        &conn,
        home,
        "zcode",
        "ses-zcode-1",
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
        after_messages.contains(&"Follow up in ZCode WAL"),
        "刷新后事件读取必须包含未 checkpoint 的 WAL 消息：{after_messages:?}"
    );
}
