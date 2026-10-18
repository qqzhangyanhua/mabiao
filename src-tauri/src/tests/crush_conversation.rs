use crate::conversation;
use crate::domain::ConversationQuery;
use crate::test_support::*;

#[test]
fn crush_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_default_crush_home(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "crush" && row.session_id == "sess-crush-1")
        .expect("crush session");
    assert_eq!(
        row.project,
        home.join("work/crush-demo").to_string_lossy().as_ref()
    );
    assert_eq!(row.model, "claude-sonnet-4-20250514");
    assert!(
        page.rows.iter().all(|row| row.session_id != "task-child"),
        "Crush 子会话不得进目录"
    );

    let detail = conversation::load_parsed_detail(&conn, home, "crush", "sess-crush-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Draft the crush notes".to_string(),
            "Notes drafted.".to_string()
        ]
    );
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == crate::domain::ConversationEventKind::ToolCall));
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == crate::domain::ConversationEventKind::Plan));

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("crush notes".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "crush" && row.session_id == "sess-crush-1"));
    assert_conversation_index_matches_parse(&conn, home, "crush", "sess-crush-1");
}
