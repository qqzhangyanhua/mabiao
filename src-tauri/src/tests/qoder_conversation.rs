use crate::conversation;
use crate::domain::{ConversationEventKind, ConversationQuery};
use crate::test_support::*;

fn seed_qoder(home: &std::path::Path) {
    write_home_fixture(
        home,
        ".qoder/projects/-work-qoder/qoder-session-1.jsonl",
        "qoder-conversation.jsonl",
    );
}

fn seed_qoder_cn(home: &std::path::Path) {
    write_home_fixture(
        home,
        ".qoder-cn/projects/-work-qoder-cn/qoder-cn-session-1.jsonl",
        "qoder-cn-conversation.jsonl",
    );
}

#[test]
fn qoder_and_qoder_cn_sessions_feed_the_unified_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_qoder(home);
    seed_qoder_cn(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "unexpected report: {report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let qoder = page
        .rows
        .iter()
        .find(|row| row.source == "qoder" && row.session_id == "qoder-session-1")
        .expect("qoder session");
    assert_eq!(qoder.title, "Importer check");
    assert_eq!(qoder.project, "/work/qoder");
    assert_eq!(qoder.model, "qwen3-coder-plus");

    let qoder_cn = page
        .rows
        .iter()
        .find(|row| row.source == "qoder_cn" && row.session_id == "qoder-cn-session-1")
        .expect("qoder_cn session");
    assert_eq!(qoder_cn.title, "Translate the changelog");
    assert_eq!(qoder_cn.project, "/work/qoder-cn");
    assert_eq!(qoder_cn.model, "glm-5");
}

#[test]
fn qoder_detail_keeps_messages_and_tool_calls() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_qoder(home);
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();

    let detail = conversation::load_parsed_detail(&conn, home, "qoder", "qoder-session-1").unwrap();
    assert_eq!(detail.session.source, "qoder");
    assert_eq!(
        message_texts(&detail),
        vec![
            "Inspect the importer".to_string(),
            "Checking the importer.".to_string(),
            "The importer is ready.".to_string()
        ]
    );
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolCall && event.name.as_deref() == Some("Read")
    }));
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == ConversationEventKind::ToolResult
            && event.name.as_deref() == Some("Read")));
}

#[test]
fn qoder_body_search_hits_indexed_user_text() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_qoder(home);
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();

    let page = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("importer".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(page
        .rows
        .iter()
        .any(|row| row.source == "qoder" && row.session_id == "qoder-session-1"));
}

#[test]
fn qoder_usage_shaped_lines_still_index_a_session() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".qoder/projects/-work-qoder/11111111-aaaa-4bbb-8ccc-000000000001.jsonl",
        "qoder.jsonl",
    );
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "qoder")
        .expect("usage-shaped qoder still becomes a catalog row");
    assert_eq!(row.session_id, "11111111-aaaa-4bbb-8ccc-000000000001");
    assert_eq!(row.project, "/work/qoder");
    assert_eq!(row.model, "qwen3-coder-plus");
}
