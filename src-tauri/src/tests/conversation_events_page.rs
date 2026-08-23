use crate::domain::ConversationEventAnchor;
use crate::test_support::*;

fn seed_codex_named(
    home: &std::path::Path,
    file_name: &str,
    fixture_name: &str,
) -> std::path::PathBuf {
    write_home_fixture(
        home,
        &format!(".codex/sessions/2026/08/{file_name}"),
        fixture_name,
    )
}

fn mark_session_unready(conn: &rusqlite::Connection, source: &str, session_id: &str) {
    conn.execute(
        "DELETE FROM conversation_events WHERE source = ?1 AND session_id = ?2",
        rusqlite::params![source, session_id],
    )
    .unwrap();
    conn.execute(
        r#"
        UPDATE conversation_sessions
        SET adapter_version = 0, event_index_generation = NULL
        WHERE source = ?1 AND session_id = ?2
        "#,
        rusqlite::params![source, session_id],
    )
    .unwrap();
    conn.execute(
        r#"
        UPDATE conversation_session_files
        SET adapter_version = 0
        WHERE source = ?1 AND session_id = ?2
        "#,
        rusqlite::params![source, session_id],
    )
    .unwrap();
}

fn strip_details(
    mut events: Vec<crate::domain::ConversationEvent>,
) -> Vec<crate::domain::ConversationEvent> {
    for event in &mut events {
        event.details = serde_json::Value::Null;
    }
    events
}

fn kinds_and_sequences(events: &[crate::domain::ConversationEvent]) -> Vec<(&str, u32)> {
    events
        .iter()
        .map(|event| (event.kind.as_str(), event.sequence))
        .collect()
}

fn load_page(
    conn: &rusqlite::Connection,
    home: &std::path::Path,
    anchor: ConversationEventAnchor,
    limit: u32,
) -> crate::domain::ConversationEventPage {
    crate::conversation::load_events(conn, home, "codex", "semantic-1", anchor, limit).unwrap()
}

#[test]
fn event_page_first_and_last_report_neighbors() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_named(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let detail = crate::conversation::load_detail(&conn, home, "codex", "semantic-1").unwrap();
    assert_eq!(detail.event_count, 10);

    let first = load_page(&conn, home, ConversationEventAnchor::First, 3);
    assert_eq!(
        kinds_and_sequences(&first.events),
        vec![("system_status", 0), ("model_change", 1), ("message", 2),]
    );
    assert!(!first.has_more_before);
    assert!(first.has_more_after);

    let last = load_page(&conn, home, ConversationEventAnchor::Last, 3);
    assert_eq!(
        kinds_and_sequences(&last.events),
        vec![("model_change", 7), ("error", 8), ("unadapted", 9),]
    );
    assert!(last.has_more_before);
    assert!(!last.has_more_after);
}

#[test]
fn event_page_before_and_after_walk_by_sequence() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_named(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let earlier = load_page(
        &conn,
        home,
        ConversationEventAnchor::Before { sequence: 7 },
        3,
    );
    assert_eq!(
        kinds_and_sequences(&earlier.events),
        vec![("plan", 4), ("tool_call", 5), ("tool_result", 6)]
    );
    assert!(earlier.has_more_before);
    assert!(earlier.has_more_after);

    let later = load_page(
        &conn,
        home,
        ConversationEventAnchor::After { sequence: 2 },
        3,
    );
    assert_eq!(
        kinds_and_sequences(&later.events),
        vec![("message", 3), ("plan", 4), ("tool_call", 5)]
    );
    assert!(later.has_more_before);
    assert!(later.has_more_after);
}

#[test]
fn event_page_exact_page_and_multiple_of_limit_have_no_trailing_neighbor() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_named(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let whole = load_page(&conn, home, ConversationEventAnchor::First, 10);
    assert_eq!(whole.events.len(), 10);
    assert!(!whole.has_more_before);
    assert!(!whole.has_more_after);

    let first_half = load_page(&conn, home, ConversationEventAnchor::First, 5);
    assert_eq!(
        first_half
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );
    assert!(!first_half.has_more_before);
    assert!(first_half.has_more_after);

    let second_half = load_page(
        &conn,
        home,
        ConversationEventAnchor::After { sequence: 4 },
        5,
    );
    assert_eq!(
        second_half
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![5, 6, 7, 8, 9]
    );
    assert!(second_half.has_more_before);
    assert!(!second_half.has_more_after);
}

#[test]
fn event_page_out_of_range_anchors_are_empty_with_the_other_side_open() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_named(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let before_start = load_page(
        &conn,
        home,
        ConversationEventAnchor::Before { sequence: 0 },
        3,
    );
    assert!(before_start.events.is_empty());
    assert!(!before_start.has_more_before);
    assert!(before_start.has_more_after);

    let after_end = load_page(
        &conn,
        home,
        ConversationEventAnchor::After { sequence: 9 },
        3,
    );
    assert!(after_end.events.is_empty());
    assert!(after_end.has_more_before);
    assert!(!after_end.has_more_after);
}

#[test]
fn event_page_with_no_rows_has_no_neighbors() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_named(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn.execute(
        "DELETE FROM conversation_events WHERE source = 'codex' AND session_id = 'semantic-1'",
        [],
    )
    .unwrap();

    let detail = crate::conversation::load_detail(&conn, home, "codex", "semantic-1").unwrap();
    assert_eq!(detail.event_count, 0);

    let page = crate::conversation::load_events(
        &conn,
        home,
        "codex",
        "semantic-1",
        ConversationEventAnchor::Last,
        200,
    )
    .unwrap();
    assert!(page.events.is_empty());
    assert!(!page.has_more_before);
    assert!(!page.has_more_after);
}

#[test]
fn event_page_fallback_matches_index_after_stripping_details() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_named(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    mark_session_unready(&conn, "codex", "semantic-1");

    let fallback = crate::conversation::load_events(
        &conn,
        home,
        "codex",
        "semantic-1",
        ConversationEventAnchor::Last,
        4,
    )
    .unwrap();
    assert!(
        fallback.events.iter().any(|event| !event.details.is_null()),
        "未就绪时应走整份解析，details 仍在"
    );

    crate::conversation::backfill_event_index(&conn, home).unwrap();
    let indexed = crate::conversation::load_events(
        &conn,
        home,
        "codex",
        "semantic-1",
        ConversationEventAnchor::Last,
        4,
    )
    .unwrap();
    assert!(
        indexed.events.iter().all(|event| event.details.is_null()),
        "就绪后应走索引，details 不在库里"
    );
    assert_eq!(indexed.has_more_before, fallback.has_more_before);
    assert_eq!(indexed.has_more_after, fallback.has_more_after);
    assert_eq!(indexed.events, strip_details(fallback.events));
}
