use rusqlite::params;

use crate::conversation;
use crate::domain::{ConversationEventKind, ConversationQuery};
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
