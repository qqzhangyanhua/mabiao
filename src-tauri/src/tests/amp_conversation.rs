use crate::conversation;
use crate::domain::ConversationQuery;
use crate::test_support::*;

#[test]
fn amp_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = home.join(".local/share/amp/threads/T-amp-1.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, fixture("amp.thread.json")).unwrap();
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "amp" && row.session_id == "T-amp-1")
        .expect("amp session");
    assert_eq!(row.project, "/work/amp");
    assert_eq!(row.title, "Inspect Amp usage");
    assert_eq!(row.model, "gpt-5");

    let detail = conversation::load_parsed_detail(&conn, home, "amp", "T-amp-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Count Amp tokens".to_string(),
            "Tokens counted.".to_string(),
            "Done.".to_string()
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
            search: Some("Count Amp tokens".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "amp" && row.session_id == "T-amp-1"));
    assert_conversation_index_matches_parse(&conn, home, "amp", "T-amp-1");
}
