use crate::conversation;
use crate::domain::ConversationQuery;
use crate::test_support::*;

pub const OPENCLAW_SESSION_ID: &str = "sess-openclaw-1";

#[test]
fn openclaw_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".openclaw/agents/main/sessions/sess-openclaw-1.jsonl",
        "openclaw.jsonl",
    );
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "openclaw" && row.session_id == OPENCLAW_SESSION_ID)
        .expect("openclaw session");
    assert_eq!(row.project, "/work/openclaw");

    let detail =
        conversation::load_parsed_detail(&conn, home, "openclaw", OPENCLAW_SESSION_ID).unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Draft the openclaw notes".to_string(),
            "Notes drafted.".to_string(),
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
            search: Some("openclaw notes".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "openclaw" && row.session_id == OPENCLAW_SESSION_ID));
    assert_conversation_index_matches_parse(&conn, home, "openclaw", OPENCLAW_SESSION_ID);
}
