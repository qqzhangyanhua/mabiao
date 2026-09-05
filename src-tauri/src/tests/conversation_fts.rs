use crate::domain::{ConversationEventAnchor, ConversationMatchField, ConversationQuery};
use crate::test_support::*;

fn search(conn: &rusqlite::Connection, term: &str) -> crate::domain::ConversationPage {
    crate::conversation::sessions_page(
        conn,
        &ConversationQuery {
            search: Some(term.to_string()),
            page: Some(1),
            page_size: Some(20),
            ..Default::default()
        },
    )
    .unwrap()
}

fn write_codex_session(
    home: &std::path::Path,
    file_name: &str,
    session_id: &str,
    title: &str,
    body: &str,
) {
    let records = [
        serde_json::json!({
            "type": "session_meta",
            "timestamp": "2026-08-20T00:00:00Z",
            "payload": {"id": session_id, "cwd": "/workspace/example-project", "model_provider": "openai"}
        }),
        serde_json::json!({
            "type": "turn_context",
            "timestamp": "2026-08-20T00:00:02Z",
            "payload": {"cwd": "/workspace/example-project", "model": "gpt-5.6-sol"}
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-20T00:00:03Z",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": title}]
            }
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-20T00:00:10Z",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": body}]
            }
        }),
    ];
    let path = home.join(".codex/sessions/2026/08").join(file_name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let content = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, format!("{content}\n")).unwrap();
}

fn mark_unready(conn: &rusqlite::Connection, source: &str, session_id: &str) {
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
}

#[test]
fn catalog_search_ranks_title_hits_before_body_hits() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_codex_session(
        home,
        "rollout-title.jsonl",
        "conv-title",
        "UniqueAuthKey helper",
        "unrelated body text here",
    );
    write_codex_session(
        home,
        "rollout-body.jsonl",
        "conv-body",
        "something else entirely",
        "we changed UniqueAuthKey in login",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let page = search(&conn, "UniqueAuthKey");
    assert_eq!(page.total, 2);
    assert_eq!(page.rows[0].session_id, "conv-title");
    assert_eq!(
        page.rows[0].match_field,
        Some(ConversationMatchField::Title)
    );
    assert_eq!(page.rows[1].session_id, "conv-body");
    assert_eq!(page.rows[1].match_field, Some(ConversationMatchField::Body));
    assert!(page.rows[1].match_event_id.is_some());
    assert!(page.rows[1].match_sequence.is_some());
    assert!(page.rows[1]
        .match_snippet
        .as_deref()
        .is_some_and(|snippet| snippet.contains("UniqueAuthKey")));
}

#[test]
fn catalog_search_matches_tool_name_and_output_not_details() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".codex/sessions/2026/08/rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let by_name = search(&conn, "read_file");
    assert_eq!(by_name.total, 1);
    assert_eq!(
        by_name.rows[0].match_field,
        Some(ConversationMatchField::Body)
    );

    let by_output = search(&conn, "fn main");
    assert_eq!(by_output.total, 1);
    assert_eq!(
        by_output.rows[0].match_field,
        Some(ConversationMatchField::Body)
    );
}

#[test]
fn catalog_search_skips_unindexed_bodies_and_marks_title_only() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_codex_session(
        home,
        "rollout-conv-1.jsonl",
        "conv-1",
        "Tray catalog title",
        "secret body phrase xyz",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    mark_unready(&conn, "codex", "conv-1");

    let body = search(&conn, "secret body phrase xyz");
    assert_eq!(body.total, 0);

    let title = search(&conn, "Tray catalog title");
    assert_eq!(title.total, 1);
    assert!(!title.rows[0].event_index_ready);
    assert_eq!(
        title.rows[0].match_field,
        Some(ConversationMatchField::Title)
    );
}

/// 三元组 AND 的召回是子串语义的超集：正文里 `aut` 和 `uth` 各自出现、但不相邻，
/// 也会被 FTS 召回。回表 LIKE 复核就是为了把这类命中挡掉。
#[test]
fn catalog_search_body_hit_requires_real_substring_not_scattered_trigrams() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_codex_session(
        home,
        "rollout-scattered.jsonl",
        "conv-scattered",
        "plain title one",
        "aut zzz uth apart",
    );
    write_codex_session(
        home,
        "rollout-real.jsonl",
        "conv-real",
        "plain title two",
        "we changed auth in login",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let page = search(&conn, "auth");
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].session_id, "conv-real");
    assert_eq!(page.rows[0].match_field, Some(ConversationMatchField::Body));
}

#[test]
fn catalog_search_matches_cjk_substring_in_body() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_codex_session(
        home,
        "rollout-cjk.jsonl",
        "conv-cjk",
        "plain title three",
        "这次重构了鉴权中间件",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();

    let page = search(&conn, "鉴权中间件");
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].match_field, Some(ConversationMatchField::Body));
}

/// 旧库的正文倒排是 `detail=full`。迁移前后搜索结果必须一致——迁移在后台跑，期间旧表
/// 还在服务查询，两种形态只要有一种搜不到就是用户可见的空窗。
#[test]
fn migrating_legacy_fts_keeps_body_search_results() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_codex_session(
        home,
        "rollout-legacy.jsonl",
        "conv-legacy",
        "plain title four",
        "body mentions UniqueAuthKey once",
    );
    let conn = store::open_memory().unwrap();
    conn.execute_batch(
        r#"
        DROP TABLE conversation_events_fts;
        CREATE VIRTUAL TABLE conversation_events_fts USING fts5(
            text,
            name,
            content='conversation_events',
            content_rowid='rowid',
            tokenize='trigram'
        );
        "#,
    )
    .unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    assert!(store::conversation_fts_needs_migration(&conn).unwrap());

    let before = search(&conn, "UniqueAuthKey");
    assert_eq!(before.total, 1);
    assert_eq!(
        before.rows[0].match_field,
        Some(ConversationMatchField::Body)
    );

    store::migrate_conversation_events_fts(&conn).unwrap();
    assert!(!store::conversation_fts_needs_migration(&conn).unwrap());

    let after = search(&conn, "UniqueAuthKey");
    assert_eq!(after.total, 1);
    assert_eq!(
        after.rows[0].match_field,
        Some(ConversationMatchField::Body)
    );
    assert_eq!(after.rows[0].match_event_id, before.rows[0].match_event_id);
    assert_eq!(after.rows[0].match_snippet, before.rows[0].match_snippet);
}

#[test]
fn fresh_cache_needs_no_fts_migration() {
    let conn = store::open_memory().unwrap();
    assert!(!store::conversation_fts_needs_migration(&conn).unwrap());
}

#[test]
fn opening_existing_cache_rebuilds_missing_fts() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let db_path = dir.path().join("usage.sqlite");
    write_codex_session(
        &home,
        "rollout-conv-1.jsonl",
        "conv-1",
        "visible title here",
        "rebuildable body phrase",
    );
    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    crate::conversation::refresh_codex(&conn, &home).unwrap();
    conn.execute_batch("DROP TABLE conversation_events_fts;")
        .unwrap();
    drop(conn);

    let reopened = store::open_db(db_path.to_str().unwrap()).unwrap();
    let page = search(&reopened, "rebuildable body phrase");
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].match_field, Some(ConversationMatchField::Body));
}

#[test]
fn event_page_around_starts_at_matching_sequence() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".codex/sessions/2026/08/rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let page = crate::conversation::load_events(
        &conn,
        home,
        "codex",
        "semantic-1",
        ConversationEventAnchor::Around { sequence: 4 },
        3,
    )
    .unwrap();
    assert_eq!(
        page.events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![4, 5, 6]
    );
    assert!(page.has_more_before);
    assert!(page.has_more_after);
}
