use crate::conversation;
use crate::domain::ConversationQuery;
use crate::test_support::*;

#[test]
fn roo_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_all_source_fixtures(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "roo" && row.session_id == "task-roo-1")
        .expect("roo session");
    assert_eq!(row.project, "/work/roo");
    assert_eq!(row.model, "openai/gpt-4.1");
    assert_eq!(row.title, "Implement the adapter");

    let detail = conversation::load_parsed_detail(&conn, home, "roo", "task-roo-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Implement the adapter".to_string(),
            "Working on it".to_string(),
            "Done".to_string()
        ]
    );
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == crate::domain::ConversationEventKind::ToolCall));
    assert!(detail.events.iter().any(|event| {
        event.kind == crate::domain::ConversationEventKind::ModelChange
            && event.name.as_deref() == Some("anthropic/claude-sonnet-4")
    }));

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("Implement the adapter".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "roo" && row.session_id == "task-roo-1"));
    assert_conversation_index_matches_parse(&conn, home, "roo", "task-roo-1");
}
