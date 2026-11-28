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

    let page = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            sources: vec!["roo".into()],
            ..ConversationQuery::default()
        },
    )
    .unwrap();
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

#[test]
fn roo_conversation_reports_application_name_on_non_array_history() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let history = home.join(
        ".config/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks/task-bad/api_conversation_history.json",
    );
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    std::fs::write(&history, "{}").unwrap();

    let conn = store::open_memory().unwrap();
    let roots = ingest::source_scan_dirs(home, Source::Roo);
    let issues = conversation::refresh(&conn, Source::Roo, &roots).unwrap();
    assert_eq!(issues.len(), 1);
    assert!(
        issues[0].message.contains("Roo Code"),
        "expected Roo Code label in issue: {:?}",
        issues[0]
    );
}
