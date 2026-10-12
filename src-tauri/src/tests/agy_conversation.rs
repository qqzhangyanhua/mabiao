use crate::conversation;
use crate::domain::{ConversationEventKind, ConversationQuery};
use crate::test_support::*;

const T0: u64 = 1_775_376_000;
const SESSION: &str = "11111111-2222-3333-4444-555555555555";

fn seed_agy(home: &std::path::Path) -> std::path::PathBuf {
    let path = home
        .join(".gemini/antigravity-cli/conversations")
        .join(format!("{SESSION}.db"));
    let steps = vec![
        (14, user_input_step_payload("Inspect the rollout", T0)),
        (
            15,
            planner_step_payload(
                None,
                Some("need to read a file"),
                &[("call-1", "view_file", r#"{"AbsolutePath":"/work/AI/a.rs"}"#)],
                T0 + 1,
                Some("req-a"),
            ),
        ),
        (132, opaque_tool_step_payload(T0 + 2)),
        (
            15,
            planner_step_payload(
                Some("The rollout looks fine"),
                None,
                &[],
                T0 + 3,
                Some("req-a"),
            ),
        ),
    ];
    write_agy_payload_db(
        &path,
        &steps,
        &[GEN_HIGH],
        Some(TRAJECTORY_URI_AI_WITH_REMOTE),
    )
}

#[test]
fn agy_session_feeds_catalog_detail_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_agy(home);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert_eq!(report.files_failed, 0, "{report:?}");
    assert!(report.conversation_issues.is_empty(), "{report:?}");

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source == "agy" && row.session_id == SESSION)
        .expect("agy session");
    assert_eq!(row.title, "Inspect the rollout");
    assert_eq!(row.project, "AI");
    assert_eq!(row.model, "gemini-3.8-flash-high");

    let detail = conversation::load_parsed_detail(&conn, home, "agy", SESSION).unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Inspect the rollout".to_string(),
            "The rollout looks fine".to_string()
        ]
    );
    let call = detail
        .events
        .iter()
        .find(|event| event.kind == ConversationEventKind::ToolCall)
        .expect("tool call");
    assert_eq!(call.name.as_deref(), Some("view_file"));
    assert_eq!(call.details["call_id"], "call-1");
    assert_eq!(call.details["input"]["AbsolutePath"], "/work/AI/a.rs");
    assert!(detail
        .events
        .iter()
        .any(|event| event.kind == ConversationEventKind::Plan
            && event.text.as_deref() == Some("need to read a file")));
    assert!(
        !detail
            .events
            .iter()
            .any(|event| event.kind == ConversationEventKind::ToolResult),
        "工具结果字段未摸清，不解析"
    );
    assert!(
        detail
            .events
            .iter()
            .filter_map(|event| event.text.as_deref())
            .all(|text| !text.contains("must stay ignored")),
        "未适配的工具步骤正文不得漏进事件"
    );
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::SystemStatus
            && event.name.as_deref() == Some("capability_degraded")
            && event.text.as_deref() == Some("tool_result")
    }));

    assert_conversation_index_matches_parse(&conn, home, "agy", SESSION);

    let search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("rollout".to_string()),
            ..ConversationQuery::default()
        },
    )
    .unwrap();
    assert!(search
        .rows
        .iter()
        .any(|row| row.source == "agy" && row.session_id == SESSION));
}

#[test]
fn agy_database_without_step_payload_is_not_a_conversation() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = home
        .join(".gemini/antigravity-cli/conversations")
        .join(format!("{SESSION}.db"));
    write_agy_conversation_db(&path, &[STEP_SIX_TUPLE], &[GEN_HIGH]);
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(report.conversation_issues.is_empty(), "{report:?}");
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(!page.rows.iter().any(|row| row.source == "agy"));
}
