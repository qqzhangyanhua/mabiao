use crate::conversation;
use crate::domain::ConversationQuery;
use crate::test_support::*;

fn seed_trae(home: &std::path::Path) {
    write_home_fixture(
        home,
        ".trae/cli/sessions/2026/10/17/rollout-trae-conv-1.jsonl",
        "trae-conversation.jsonl",
    );
}

#[test]
fn trae_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_trae(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "trae" && row.session_id == "trae-conv-1")
        .expect("trae session");
    assert_eq!(row.project, "/work/trae");
    assert_eq!(row.model, "gpt-5.6-sol");

    let detail = conversation::load_parsed_detail(&conn, home, "trae", "trae-conv-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Draft the trae notes".to_string(),
            "Notes drafted.".to_string()
        ]
    );

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("trae notes".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "trae" && row.session_id == "trae-conv-1"));
    assert_conversation_index_matches_parse(&conn, home, "trae", "trae-conv-1");
}
