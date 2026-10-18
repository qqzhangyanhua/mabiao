use crate::conversation;
use crate::domain::ConversationQuery;
use crate::test_support::*;

#[test]
fn cherry_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_default_cherry_home(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "cherry" && row.session_id == CHERRY_TOPIC_ID)
        .expect("cherry session");
    assert_eq!(row.title, "Inspect Cherry");

    let detail = conversation::load_parsed_detail(&conn, home, "cherry", CHERRY_TOPIC_ID).unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Draft the cherry notes".to_string(),
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
        .any(|event| event.kind == crate::domain::ConversationEventKind::ToolResult));

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("cherry notes".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "cherry" && row.session_id == CHERRY_TOPIC_ID));
    assert_conversation_index_matches_parse(&conn, home, "cherry", CHERRY_TOPIC_ID);
}
