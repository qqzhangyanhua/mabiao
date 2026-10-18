use crate::conversation;
use crate::domain::{ConversationEventKind, ConversationQuery};
use crate::test_support::*;

fn seed_codebuddy(home: &std::path::Path) {
    write_home_fixture(
        home,
        ".codebuddy/projects/-work-cb/sess-cb-1.jsonl",
        "codebuddy.jsonl",
    );
}

#[test]
fn codebuddy_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codebuddy(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "codebuddy" && row.session_id == "sess-cb-1")
        .expect("codebuddy session");
    assert_eq!(row.title, "Buddy notes");
    assert_eq!(row.project, "/work/cb");
    assert_eq!(row.model, "MaaS_Cl_Opus");

    let detail = conversation::load_parsed_detail(&conn, home, "codebuddy", "sess-cb-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Draft the buddy notes".to_string(),
            "Notes drafted.".to_string()
        ]
    );
    assert!(detail
        .events
        .iter()
        .all(|event| event.text.as_deref() != Some("[hook] skip this")));
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolCall && event.name.as_deref() == Some("Read")
    }));
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolCall && event.name.as_deref() == Some("Write")
    }));

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
        .any(|row| row.source == "codebuddy" && row.session_id == "sess-cb-1"));
    assert_conversation_index_matches_parse(&conn, home, "codebuddy", "sess-cb-1");
}

#[test]
fn codebuddy_subagent_is_not_a_catalog_row() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codebuddy(home);
    let child = home.join(".codebuddy/projects/-work-cb/sess-cb-1/subagents/scout.jsonl");
    std::fs::create_dir_all(child.parent().unwrap()).unwrap();
    std::fs::write(
        &child,
        r#"{"id":"u1","timestamp":1790503200000,"type":"message","role":"user","content":[{"type":"input_text","text":"scout the tree"}],"sessionId":"scout","cwd":"/work/cb"}
"#,
    )
    .unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(page.rows.iter().all(|row| row.session_id != "scout"));
    let parent = conversation::load_detail(&conn, home, "codebuddy", "sess-cb-1").unwrap();
    assert!(parent.agent_relations.children.iter().any(|child| {
        child
            .session
            .as_ref()
            .is_some_and(|session| session.session_id == "scout")
    }));
}
