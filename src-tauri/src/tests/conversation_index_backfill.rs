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

fn strip_details(
    mut events: Vec<crate::domain::ConversationEvent>,
) -> Vec<crate::domain::ConversationEvent> {
    for event in &mut events {
        event.details = serde_json::Value::Null;
    }
    events
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

#[test]
fn load_detail_falls_back_until_backfill_then_matches_the_index_path() {
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

    let fallback = crate::conversation::load_detail(&conn, home, "codex", "semantic-1").unwrap();
    assert!(
        fallback.events.iter().any(|event| !event.details.is_null()),
        "未就绪时应走整份解析，details 仍在"
    );

    crate::conversation::backfill_event_index(&conn, home).unwrap();
    let indexed = crate::conversation::load_detail(&conn, home, "codex", "semantic-1").unwrap();
    assert!(
        indexed.events.iter().all(|event| event.details.is_null()),
        "就绪后应走索引，details 不在库里"
    );
    assert_eq!(indexed.revision, fallback.revision);
    assert_eq!(indexed.session, fallback.session);
    assert_eq!(indexed.agent_relations, fallback.agent_relations);
    assert_eq!(indexed.events, strip_details(fallback.events));
}

#[test]
fn event_index_backfill_newest_first_then_resumes_without_redoing() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex_named(home, "rollout-conv-1.jsonl", "codex-conversation.jsonl");
    std::fs::write(
        home.join(".codex/sessions/2026/08/rollout-conv-2.jsonl"),
        fixture("codex-conversation.jsonl").replace("conv-1", "conv-2"),
    )
    .unwrap();
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET ended_at = '2026-08-20T00:03:00Z' WHERE session_id = 'conv-1'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET ended_at = '2026-08-22T00:03:00Z' WHERE session_id = 'conv-2'",
        [],
    )
    .unwrap();
    mark_session_unready(&conn, "codex", "conv-1");
    mark_session_unready(&conn, "codex", "conv-2");

    let progress = crate::conversation::event_index_progress(&conn).unwrap();
    assert_eq!(progress.indexed, 0);
    assert_eq!(progress.total, 2);

    assert!(crate::conversation::backfill_event_index_step(&conn, home).unwrap());
    let after_one = crate::conversation::event_index_progress(&conn).unwrap();
    assert_eq!(after_one.indexed, 1);
    assert_eq!(after_one.total, 2);

    let newer_index = crate::conversation::indexed_events(&conn, "codex", "conv-2").unwrap();
    let older_index = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();
    assert!(!newer_index.is_empty(), "结束更晚的会话应先被补建");
    assert!(older_index.is_empty(), "更早的会话此时还没有索引");
    let newer = crate::conversation::load_detail(&conn, home, "codex", "conv-2").unwrap();
    let older = crate::conversation::load_detail(&conn, home, "codex", "conv-1").unwrap();
    assert_eq!(newer.events, newer_index, "已补建会话的详情应走索引");
    assert_ne!(older.events, older_index, "未补建会话的详情应回退整份解析");

    conn.execute(
        "UPDATE conversation_events SET sequence = sequence + 100 WHERE session_id = 'conv-2'",
        [],
    )
    .unwrap();
    crate::conversation::backfill_event_index(&conn, home).unwrap();

    let progress = crate::conversation::event_index_progress(&conn).unwrap();
    assert_eq!(progress.indexed, 2);
    assert_eq!(progress.total, 2);
    let newer_again = crate::conversation::indexed_events(&conn, "codex", "conv-2").unwrap();
    assert!(
        newer_again.iter().any(|event| event.sequence >= 100),
        "已完成的会话不得被补建再跑一遍"
    );
    assert_conversation_index_matches_parse(&conn, home, "codex", "conv-1");
}
