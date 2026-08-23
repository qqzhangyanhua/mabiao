use crate::test_support::*;

fn seed_codex_fixture(
    home: &std::path::Path,
    file_name: &str,
    fixture_name: &str,
) -> std::path::PathBuf {
    let path = home.join(".codex/sessions/2026/08").join(file_name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, fixture(fixture_name)).unwrap();
    path
}

fn strip_details(
    mut events: Vec<crate::domain::ConversationEvent>,
) -> Vec<crate::domain::ConversationEvent> {
    for event in &mut events {
        event.details = serde_json::Value::Null;
    }
    events
}

fn assert_index_matches_parse(
    conn: &rusqlite::Connection,
    home: &std::path::Path,
    source: &str,
    session_id: &str,
) {
    assert_conversation_index_matches_parse(conn, home, source, session_id);
}

#[test]
fn codex_event_index_matches_full_parse_on_a_single_source_file() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    assert_index_matches_parse(&conn, home, "codex", "semantic-1");
}

#[test]
fn codex_event_index_matches_full_parse_across_split_source_files() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(home, "rollout-split-a.jsonl", "codex-split-session-a.jsonl");
    seed_codex_fixture(home, "rollout-split-b.jsonl", "codex-split-session-b.jsonl");
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    assert_index_matches_parse(&conn, home, "codex", "split-1");

    let indexed = crate::conversation::indexed_events(&conn, "codex", "split-1").unwrap();
    let texts = indexed
        .iter()
        .filter_map(|event| event.text.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(texts, vec!["early", "shared", "late"]);
}

#[test]
fn codex_event_index_keeps_the_previous_generation_when_a_source_file_fails() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(home, "rollout-split-a.jsonl", "codex-split-session-a.jsonl");
    let second = seed_codex_fixture(home, "rollout-split-b.jsonl", "codex-split-session-b.jsonl");
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let before = crate::conversation::indexed_events(&conn, "codex", "split-1").unwrap();
    assert!(!before.is_empty());

    std::fs::write(&second, "{not-json\n").unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let after = crate::conversation::indexed_events(&conn, "codex", "split-1").unwrap();
    assert_eq!(
        strip_details(after),
        strip_details(before),
        "解析失败不得用残缺结果覆盖上一代索引"
    );
}

#[test]
fn codex_event_index_stays_empty_when_the_first_ingest_has_a_failing_split_file() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(home, "rollout-split-a.jsonl", "codex-split-session-a.jsonl");
    let second = seed_codex_fixture(home, "rollout-split-b.jsonl", "codex-split-session-b.jsonl");
    std::fs::write(&second, "{not-json\n").unwrap();
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();

    let indexed = crate::conversation::indexed_events(&conn, "codex", "split-1").unwrap();
    assert!(
        indexed.is_empty(),
        "首次摄取有源文件解析失败时不得发布残缺一代"
    );
}

#[test]
fn codex_event_index_matches_full_parse_when_timestamps_are_mixed() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(
        home,
        "rollout-mixed-ts.jsonl",
        "codex-mixed-timestamps.jsonl",
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    assert_index_matches_parse(&conn, home, "codex", "mixed-ts-1");
}

#[test]
fn codex_event_index_clears_when_the_source_file_disappears() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex_fixture(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    assert!(
        !crate::conversation::indexed_events(&conn, "codex", "semantic-1")
            .unwrap()
            .is_empty()
    );

    std::fs::remove_file(&path).unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let indexed = crate::conversation::indexed_events(&conn, "codex", "semantic-1").unwrap();
    assert!(indexed.is_empty(), "源文件消失后读回不得残留事件");
}

#[test]
fn codex_event_index_still_publishes_a_successful_session_when_another_fails() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let intact = seed_codex_fixture(
        home,
        "rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    seed_codex_fixture(home, "rollout-split-a.jsonl", "codex-split-session-a.jsonl");
    let failing = seed_codex_fixture(home, "rollout-split-b.jsonl", "codex-split-session-b.jsonl");
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    let split_before = crate::conversation::indexed_events(&conn, "codex", "split-1").unwrap();
    assert!(!split_before.is_empty());

    std::fs::write(&failing, "{not-json\n").unwrap();
    let mut rewritten = std::fs::read_to_string(&intact).unwrap();
    rewritten.push_str(
        r#"{"type":"response_item","timestamp":"2026-08-21T00:00:20Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"reindexed"}]}}
"#,
    );
    std::fs::write(&intact, rewritten).unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();

    assert_eq!(
        crate::conversation::indexed_events(&conn, "codex", "split-1").unwrap(),
        split_before,
        "失败会话必须保留上一代"
    );
    assert_index_matches_parse(&conn, home, "codex", "semantic-1");
}

#[test]
fn codex_event_index_reparses_unchanged_files_after_sequence_order_changes() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(
        home,
        "rollout-mixed-ts.jsonl",
        "codex-mixed-timestamps.jsonl",
    );
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn.execute_batch(
        r#"
        UPDATE conversation_events
        SET sequence = (
            SELECT MAX(sequence) FROM conversation_events
            WHERE source = 'codex' AND session_id = 'mixed-ts-1'
        ) - sequence
        WHERE source = 'codex' AND session_id = 'mixed-ts-1';
        UPDATE conversation_sessions
        SET adapter_version = 8
        WHERE source = 'codex' AND session_id = 'mixed-ts-1';
        UPDATE conversation_session_files
        SET adapter_version = 8
        WHERE source = 'codex' AND session_id = 'mixed-ts-1';
        "#,
    )
    .unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    assert_index_matches_parse(&conn, home, "codex", "mixed-ts-1");
}
