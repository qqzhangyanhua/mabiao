use rusqlite::params;

use crate::conversation;
use crate::domain::{ConversationEventKind, ConversationQuery};
use crate::test_support::*;

fn create_opencode_database(home: &std::path::Path) -> (std::path::PathBuf, rusqlite::Connection) {
    let path = home.join(".local/share/opencode/opencode.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.pragma_update(None, "journal_mode", "WAL").unwrap();
    db.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    db.execute_batch(
        r#"
        CREATE TABLE session (
            id TEXT PRIMARY KEY,
            parent_id TEXT,
            title TEXT,
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
        "INSERT INTO session VALUES(?1, NULL, ?2, ?3, ?4, ?5)",
        params![
            "ses-usage",
            "Inspect OpenCode",
            "/workspace/opencode",
            1_780_000_000_000_i64,
            1_780_000_003_000_i64
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO session VALUES(?1, NULL, ?2, ?3, ?4, ?5)",
        params![
            "ses-no-usage",
            "No usage conversation",
            "/workspace/empty",
            1_780_000_010_000_i64,
            1_780_000_011_000_i64
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO message VALUES(?1, ?2, ?3, ?4)",
        params![
            "msg-user",
            "ses-usage",
            1_780_000_000_000_i64,
            serde_json::json!({"role":"user","time":{"created":1_780_000_000_000_i64}}).to_string()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO message VALUES(?1, ?2, ?3, ?4)",
        params![
            "msg-assistant",
            "ses-usage",
            1_780_000_001_000_i64,
            serde_json::json!({
                "role":"assistant",
                "modelID":"opencode-test-model",
                "providerID":"test-provider",
                "path":{"cwd":"/workspace/opencode","root":"/workspace/opencode"},
                "time":{"created":1_780_000_001_000_i64,"completed":1_780_000_003_000_i64},
                "tokens":{"input":12,"output":5,"reasoning":1,"cache":{"read":2,"write":0}}
            })
            .to_string()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO message VALUES(?1, ?2, ?3, ?4)",
        params![
            "msg-empty",
            "ses-no-usage",
            1_780_000_010_000_i64,
            serde_json::json!({"role":"user","time":{"created":1_780_000_010_000_i64}}).to_string()
        ],
    )
    .unwrap();
    let parts = [
        (
            "part-user",
            "msg-user",
            "ses-usage",
            1_780_000_000_100_i64,
            serde_json::json!({"type":"text","text":"Read the manifest"}),
        ),
        (
            "part-assistant",
            "msg-assistant",
            "ses-usage",
            1_780_000_001_100_i64,
            serde_json::json!({"type":"text","text":"I will inspect it."}),
        ),
        (
            "part-tool",
            "msg-assistant",
            "ses-usage",
            1_780_000_002_000_i64,
            serde_json::json!({"type":"tool","callID":"call-read","tool":"read","state":{"status":"completed","input":{"path":"package.json"},"output":"manifest contents"}}),
        ),
        (
            "part-unknown",
            "msg-assistant",
            "ses-usage",
            1_780_000_002_500_i64,
            serde_json::json!({"type":"future-part","secret_body":"must not enter diagnostics"}),
        ),
        (
            "part-empty",
            "msg-empty",
            "ses-no-usage",
            1_780_000_010_100_i64,
            serde_json::json!({"type":"text","text":"This session has no usage"}),
        ),
    ];
    for (id, message_id, session_id, created, data) in parts {
        db.execute(
            "INSERT INTO part VALUES(?1, ?2, ?3, ?4, ?5)",
            params![id, message_id, session_id, created, data.to_string()],
        )
        .unwrap();
    }
    (path, db)
}

#[test]
fn opencode_database_feeds_catalog_detail_and_wal_refresh_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (db_path, source_db) = create_opencode_database(home);
    let wal_path = std::path::PathBuf::from(format!("{}-wal", db_path.to_string_lossy()));
    let database_before_reads = std::fs::read(&db_path).unwrap();
    let wal_before_reads = std::fs::read(&wal_path).unwrap();
    let schema_version: i64 = source_db
        .pragma_query_value(None, "schema_version", |row| row.get(0))
        .unwrap();
    let conn = store::open_memory().unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let opencode_rows = page
        .rows
        .iter()
        .filter(|row| row.source == "opencode")
        .collect::<Vec<_>>();

    assert!(report.files_failed == 0, "unexpected report: {report:?}");
    assert_eq!(opencode_rows.len(), 2);
    assert!(opencode_rows
        .iter()
        .all(|row| row.support_status == "experimental"));
    assert!(opencode_rows
        .iter()
        .any(|row| row.session_id == "ses-no-usage"));

    let detail = conversation::load_detail(&conn, home, "opencode", "ses-usage").unwrap();
    assert_eq!(detail.session.title, "Inspect OpenCode");
    assert_eq!(detail.session.project, "/workspace/opencode");
    assert_eq!(detail.session.model, "opencode-test-model");
    assert_eq!(usage_rows(&conn, "opencode", "ses-usage").len(), 1);
    assert_eq!(
        message_texts(&detail),
        vec![
            "Read the manifest".to_string(),
            "I will inspect it.".to_string()
        ]
    );
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolCall && event.name.as_deref() == Some("read")
    }));
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == ConversationEventKind::ToolResult));
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == ConversationEventKind::Unadapted));
    let initial_event_ids = detail
        .events
        .iter()
        .map(|event| event.event_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(conversation::build_export(
        &conn,
        home,
        "opencode",
        "ses-usage",
        ConversationExportFormat::Json,
    )
    .is_err());
    assert!(conversation::build_export(
        &conn,
        home,
        "opencode",
        "ses-usage",
        ConversationExportFormat::Markdown,
    )
    .unwrap()
    .default_name
    .ends_with(".md"));
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(
        conversation::sessions_page(&conn, &ConversationQuery::default())
            .unwrap()
            .rows
            .iter()
            .filter(|row| row.source == "opencode")
            .count(),
        2
    );
    let no_usage = conversation::load_detail(&conn, home, "opencode", "ses-no-usage").unwrap();
    assert!(usage_rows(&conn, "opencode", "ses-no-usage").is_empty());
    assert_eq!(message_texts(&no_usage)[0], "This session has no usage");
    assert_eq!(std::fs::read(&db_path).unwrap(), database_before_reads);
    assert_eq!(std::fs::read(&wal_path).unwrap(), wal_before_reads);

    let previous_revision = detail.revision;
    source_db
        .execute(
            "INSERT INTO message VALUES(?1, ?2, ?3, ?4)",
            params![
                "msg-follow-up",
                "ses-usage",
                1_780_000_004_000_i64,
                serde_json::json!({"role":"assistant","time":{"created":1_780_000_004_000_i64,"completed":1_780_000_004_500_i64}}).to_string()
            ],
        )
        .unwrap();
    source_db
        .execute(
            "INSERT INTO part VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                "part-follow-up",
                "msg-follow-up",
                "ses-usage",
                1_780_000_004_100_i64,
                serde_json::json!({"type":"text","text":"WAL follow-up"}).to_string()
            ],
        )
        .unwrap();

    let state =
        conversation::detail_state(&conn, home, "opencode", "ses-usage", &previous_revision)
            .unwrap();
    assert!(state.changed);
    assert!(state.file_available);
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let refreshed = conversation::load_detail(&conn, home, "opencode", "ses-usage").unwrap();
    assert!(message_texts(&refreshed)
        .iter()
        .any(|text| text == "WAL follow-up"));
    let refreshed_event_ids = refreshed
        .events
        .iter()
        .map(|event| event.event_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(initial_event_ids.is_subset(&refreshed_event_ids));

    assert_eq!(
        source_db
            .pragma_query_value(None, "schema_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        schema_version
    );
    source_db
        .execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
        .unwrap();
}

#[test]
fn opencode_schema_degrades_optional_parts_and_preserves_last_good_sessions_on_fatal_change() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (_db_path, source_db) = create_opencode_database(home);
    let conn = store::open_memory().unwrap();

    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(
        conversation::sessions_page(&conn, &ConversationQuery::default())
            .unwrap()
            .rows
            .iter()
            .filter(|row| row.source == "opencode")
            .count(),
        2
    );

    source_db.execute_batch("DROP TABLE part;").unwrap();
    let degraded_report =
        ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let degraded = conversation::load_detail(&conn, home, "opencode", "ses-usage").unwrap();
    assert!(message_texts(&degraded).is_empty());
    assert!(degraded.events.is_empty());
    assert_eq!(degraded.session.capabilities, vec!["usage"]);
    assert!(degraded_report.conversation_issues.iter().any(|issue| {
        issue.source == "opencode" && issue.event_type.as_deref() == Some("part_schema")
    }));
    let repeated_report =
        ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(repeated_report.conversation_issues.iter().any(|issue| {
        issue.source == "opencode" && issue.event_type.as_deref() == Some("part_schema")
    }));

    source_db.execute_batch("DROP TABLE session;").unwrap();
    let fatal_report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let retained = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert_eq!(
        retained
            .rows
            .iter()
            .filter(|row| row.source == "opencode")
            .count(),
        2
    );
    assert!(retained
        .rows
        .iter()
        .filter(|row| row.source == "opencode")
        .all(|row| row.file_available));
    let diagnostics = fatal_report
        .conversation_issues
        .iter()
        .map(|issue| format!("{} {:?}", issue.message, issue.event_type))
        .collect::<String>();
    assert!(diagnostics.contains("session"));
    assert!(!diagnostics.contains("manifest contents"));
    assert!(!diagnostics.contains("Read the manifest"));
}
