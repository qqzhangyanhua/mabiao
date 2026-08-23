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
    session_id: &str,
) {
    let parsed = crate::conversation::load_detail(conn, home, "codex", session_id).unwrap();
    let indexed = crate::conversation::indexed_events(conn, "codex", session_id).unwrap();
    assert!(
        indexed.iter().all(|event| event.details.is_null()),
        "索引不得存 details"
    );
    assert_eq!(
        indexed,
        strip_details(parsed.events),
        "索引事件序列必须与整份解析逐字段一致（不含 details）"
    );
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
    assert_index_matches_parse(&conn, home, "semantic-1");
}

#[test]
fn codex_event_index_matches_full_parse_across_split_source_files() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_fixture(home, "rollout-split-a.jsonl", "codex-split-session-a.jsonl");
    seed_codex_fixture(home, "rollout-split-b.jsonl", "codex-split-session-b.jsonl");
    let conn = store::open_memory().unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();
    assert_index_matches_parse(&conn, home, "split-1");

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
