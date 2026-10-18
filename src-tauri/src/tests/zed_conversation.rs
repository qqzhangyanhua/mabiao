use crate::conversation;
use crate::domain::ConversationQuery;
use crate::test_support::*;

#[test]
fn zed_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_default_zed_home(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "zed" && row.session_id == ZED_THREAD_ID)
        .expect("zed session");
    assert_eq!(row.project, "/work/zed");
    assert_eq!(row.model, "claude-sonnet-4");
    assert!(
        page.rows
            .iter()
            .all(|row| row.session_id != "thread-unknown"),
        "unknown Zed data_type must stay out of the catalog"
    );

    let detail = conversation::load_parsed_detail(&conn, home, "zed", ZED_THREAD_ID).unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Draft the zed notes".to_string(),
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
    assert!(detail.events.iter().any(|event| {
        event.kind == crate::domain::ConversationEventKind::Plan
            && event.text.as_deref() == Some("planning")
    }));

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("zed notes".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "zed" && row.session_id == ZED_THREAD_ID));
    assert_conversation_index_matches_parse(&conn, home, "zed", ZED_THREAD_ID);
}
