use crate::conversation;
use crate::domain::ConversationQuery;
use crate::test_support::*;

#[test]
fn goose_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_default_goose_home(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "goose" && row.session_id == "20261017_1")
        .expect("goose session");
    assert_eq!(row.project, "/work/goose");
    assert_eq!(row.model, "claude-sonnet-4-20250514");
    assert!(
        page.rows.iter().all(|row| row.session_id != "hidden_1"),
        "hidden Goose sessions must stay out of the catalog"
    );

    let detail = conversation::load_parsed_detail(&conn, home, "goose", "20261017_1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Draft the goose notes".to_string(),
            "Notes drafted.".to_string()
        ]
    );
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == crate::domain::ConversationEventKind::ToolCall));

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("goose notes".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "goose" && row.session_id == "20261017_1"));
    assert_conversation_index_matches_parse(&conn, home, "goose", "20261017_1");
}
