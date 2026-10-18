use crate::conversation;
use crate::domain::ConversationQuery;
use crate::test_support::*;

#[test]
fn codebuddy_ide_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_codebuddy_ide_home(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "codebuddy_ide" && row.session_id == "conv-ide-1")
        .expect("codebuddy ide session");
    assert_eq!(row.project, "/work/cb-ide");
    assert_eq!(row.model, "deepseek-v4-flash");
    assert!(row.title.contains("buddy notes") || row.title.contains("Draft"));

    let detail =
        conversation::load_parsed_detail(&conn, home, "codebuddy_ide", "conv-ide-1").unwrap();
    assert!(message_texts(&detail)
        .iter()
        .any(|text| text.contains("Draft the buddy notes")));

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("buddy".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "codebuddy_ide" && row.session_id == "conv-ide-1"));
    assert_conversation_index_matches_parse(&conn, home, "codebuddy_ide", "conv-ide-1");
}
