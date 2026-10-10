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

/// rowid、库内 event_id、序号、种类、正文指纹、代号。
type StoredRow = (i64, String, Option<i64>, String, Option<i64>, i64);

/// 只看库里的行：外置正文的源文件被改坏后读不回正文，但上一代索引本身必须原样保留。
fn stored_generation(
    conn: &rusqlite::Connection,
    source: &str,
    session_id: &str,
) -> Vec<StoredRow> {
    let mut statement = conn
        .prepare(
            "SELECT e.rowid, e.event_id, e.sequence, e.kind, e.text_hash, e.index_generation
             FROM conversation_events AS e
             JOIN conversation_sessions AS s
               ON s.source = e.source
              AND s.session_id = e.session_id
              AND s.event_index_generation = e.index_generation
             WHERE e.source = ?1 AND e.session_id = ?2
             ORDER BY e.rowid",
        )
        .unwrap();
    statement
        .query_map(rusqlite::params![source, session_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
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
    assert!(
        !crate::conversation::indexed_events(&conn, "codex", "split-1")
            .unwrap()
            .is_empty()
    );
    let before = stored_generation(&conn, "codex", "split-1");

    std::fs::write(&second, "{not-json\n").unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    assert_eq!(
        stored_generation(&conn, "codex", "split-1"),
        before,
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
    assert!(
        !crate::conversation::indexed_events(&conn, "codex", "split-1")
            .unwrap()
            .is_empty()
    );
    let split_before = stored_generation(&conn, "codex", "split-1");
    let semantic_before =
        crate::conversation::indexed_events(&conn, "codex", "semantic-1").unwrap();
    assert!(!semantic_before.is_empty());

    std::fs::write(&failing, "{not-json\n").unwrap();
    let mut rewritten = std::fs::read_to_string(&intact).unwrap();
    rewritten.push_str(
        r#"{"type":"response_item","timestamp":"2026-08-21T00:00:20Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"reindexed"}]}}
"#,
    );
    std::fs::write(&intact, rewritten).unwrap();

    crate::conversation::refresh_codex(&conn, home).unwrap();

    assert_eq!(
        stored_generation(&conn, "codex", "split-1"),
        split_before,
        "失败会话必须保留上一代"
    );
    let semantic_after = crate::conversation::indexed_events(&conn, "codex", "semantic-1").unwrap();
    assert_eq!(
        semantic_after[..semantic_before.len()]
            .iter()
            .map(|event| (event.event_id.clone(), event.sequence))
            .collect::<Vec<_>>(),
        semantic_before
            .iter()
            .map(|event| (event.event_id.clone(), event.sequence))
            .collect::<Vec<_>>(),
        "另一会话失败时，成功会话仍应增量追加且不得重排已有序号"
    );
    assert_eq!(
        semantic_after
            .last()
            .and_then(|event| event.text.as_deref()),
        Some("reindexed"),
        "成功会话必须发布本次追加的新事件"
    );
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
    let reversed = crate::conversation::indexed_events(&conn, "codex", "mixed-ts-1").unwrap();
    assert!(
        reversed.first().is_some_and(
            |event| event.sequence == 0 && event.name.as_deref() == Some("future_event")
        ),
        "启动摄取不得因适配器版本过期整份重解析"
    );

    crate::conversation::backfill_event_index(&conn, home).unwrap();
    assert_index_matches_parse(&conn, home, "codex", "mixed-ts-1");
}
