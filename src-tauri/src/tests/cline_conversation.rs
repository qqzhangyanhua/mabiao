use crate::conversation;
use crate::domain::{ConversationEventKind, ConversationQuery};
use crate::test_support::*;

fn seed_cline(home: &std::path::Path) {
    write_home_fixture(
        home,
        ".cline/data/sessions/sess-cline-1/sess-cline-1.messages.json",
        "cline.messages.json",
    );
    write_home_fixture(
        home,
        ".cline/data/sessions/sess-cline-1/sess-cline-1.json",
        "cline.manifest.json",
    );
}

#[test]
fn cline_session_feeds_catalog_detail_and_body_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_cline(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "cline" && row.session_id == "sess-cline-1")
        .expect("cline session");
    assert_eq!(row.title, "Importer speed-up");
    assert_eq!(row.project, "/work/cline");
    assert_eq!(row.model, "claude-haiku-4");

    let detail = conversation::load_parsed_detail(&conn, home, "cline", "sess-cline-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Speed up the importer".to_string(),
            "checking".to_string(),
            "done".to_string()
        ]
    );
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == ConversationEventKind::ModelChange));

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("importer".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "cline" && row.session_id == "sess-cline-1"));
}

#[test]
fn cline_fork_skips_copied_messages() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".cline/data/sessions/sess-cline-fork/sess-cline-fork.messages.json",
        "cline-fork.messages.json",
    );
    write_home_fixture(
        home,
        ".cline/data/sessions/sess-cline-fork/sess-cline-fork.json",
        "cline-fork.manifest.json",
    );
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let detail = conversation::load_parsed_detail(&conn, home, "cline", "sess-cline-fork").unwrap();
    assert_eq!(message_texts(&detail), vec!["new work".to_string()]);
    assert!(detail
        .events
        .iter()
        .all(|event| event.text.as_deref() != Some("copied")));
}

#[test]
fn cline_subagent_is_not_a_catalog_row() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_cline(home);
    let child = home.join(".cline/data/sessions/sess-cline-1/agent-scout.messages.json");
    std::fs::write(
        &child,
        r#"{"sessionId":"agent-scout","messages":[{"role":"user","content":"scout the tree","ts":1790503400000}]}"#,
    )
    .unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(page.rows.iter().all(|row| row.session_id != "agent-scout"));
    let parent = conversation::load_detail(&conn, home, "cline", "sess-cline-1").unwrap();
    assert!(parent.agent_relations.children.iter().any(|child| child
        .session
        .as_ref()
        .is_some_and(|session| session.session_id == "agent-scout")));
}
