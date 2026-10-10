use crate::conversation;
use crate::domain::{ConversationEventKind, ConversationQuery};
use crate::test_support::*;

fn seed_workbuddy(home: &std::path::Path) {
    write_home_fixture(
        home,
        ".workbuddy/projects/-work-wb/sess-wb-1.jsonl",
        "workbuddy.jsonl",
    );
}

#[test]
fn workbuddy_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_workbuddy(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "workbuddy" && row.session_id == "sess-wb-1")
        .expect("workbuddy session");
    assert_eq!(row.title, "Launch notes");
    assert_eq!(row.project, "/work/wb");
    assert_eq!(row.model, "hy3");

    let detail = conversation::load_parsed_detail(&conn, home, "workbuddy", "sess-wb-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec!["Draft the launch notes".to_string()]
    );
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolCall && event.name.as_deref() == Some("Read")
    }));
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolCall && event.name.as_deref() == Some("Write")
    }));

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("launch".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "workbuddy" && row.session_id == "sess-wb-1"));
}

#[test]
fn workbuddy_subagent_is_not_a_catalog_row() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_workbuddy(home);
    let child = home.join(".workbuddy/projects/-work-wb/sess-wb-1/subagents/scout.jsonl");
    std::fs::create_dir_all(child.parent().unwrap()).unwrap();
    std::fs::write(
        &child,
        r#"{"id":"u1","timestamp":1790503200000,"type":"message","role":"user","content":[{"type":"input_text","text":"scout the tree"}],"sessionId":"scout","cwd":"/work/wb"}
"#,
    )
    .unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(page.rows.iter().all(|row| row.session_id != "scout"));
    let parent = conversation::load_detail(&conn, home, "workbuddy", "sess-wb-1").unwrap();
    assert!(parent.agent_relations.children.iter().any(|child| {
        child
            .session
            .as_ref()
            .is_some_and(|session| session.session_id == "scout")
    }));
}
