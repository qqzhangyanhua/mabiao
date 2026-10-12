use std::collections::BTreeSet;
use std::io::Write;

use crate::domain::{ConversationEventAnchor, ConversationMatchField, ConversationQuery};
use crate::test_support::*;

const CODEX_DIR: &str = ".codex/sessions/2026/08";

fn seed_codex(home: &std::path::Path, session_id: &str) -> std::path::PathBuf {
    let path = home
        .join(CODEX_DIR)
        .join(format!("rollout-{session_id}.jsonl"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        fixture("codex-conversation.jsonl").replace("conv-1", session_id),
    )
    .unwrap();
    path
}

/// 改掉中间与结尾的内容并让文件变短：不是追加，只能整份重建。
fn rewrite_not_append(path: &std::path::Path, last_reply: &str, ended_at: &str) {
    let original = std::fs::read_to_string(path).unwrap();
    let rewritten = original
        .replace("已完成提交。", last_reply)
        .replace("2026-08-20T00:03:00Z", ended_at);
    assert!(rewritten.len() < original.len());
    std::fs::write(path, rewritten).unwrap();
}

fn append_follow_up(path: &std::path::Path) {
    writeln!(
        std::fs::OpenOptions::new().append(true).open(path).unwrap(),
        r#"{{"type":"response_item","timestamp":"2026-08-20T00:04:00Z","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"follow-up"}}]}}}}"#
    )
    .unwrap();
}

fn refresh_full(conn: &rusqlite::Connection, home: &std::path::Path) {
    crate::conversation::refresh_codex(conn, home).unwrap();
}

fn refresh_catalog(conn: &rusqlite::Connection, home: &std::path::Path) {
    let roots = ingest::source_scan_dirs(home, Source::Codex);
    let issues = crate::conversation::refresh_catalog(conn, Source::Codex, &roots).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
}

fn needs_backfill(conn: &rusqlite::Connection, session_id: &str) -> bool {
    crate::conversation::session_needs_event_backfill(conn, "codex", session_id).unwrap()
}

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

fn catalog_row(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> crate::domain::ConversationSessionRow {
    crate::conversation::sessions_page(conn, &ConversationQuery::default())
        .unwrap()
        .rows
        .into_iter()
        .find(|row| row.session_id == session_id)
        .expect("session listed in catalog")
}

fn events(
    conn: &rusqlite::Connection,
    home: &std::path::Path,
    session_id: &str,
) -> Vec<crate::domain::ConversationEvent> {
    crate::conversation::load_events(
        conn,
        home,
        "codex",
        session_id,
        ConversationEventAnchor::First,
        200,
    )
    .unwrap()
    .events
}

fn strip_details(
    mut events: Vec<crate::domain::ConversationEvent>,
) -> Vec<crate::domain::ConversationEvent> {
    for event in &mut events {
        event.details = serde_json::Value::Null;
    }
    events
}

fn key(session_id: &str) -> (String, String) {
    ("codex".to_string(), session_id.to_string())
}

#[test]
fn catalog_refresh_lists_new_sessions_like_full_refresh_and_leaves_events_to_backfill() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex(home, "conv-1");
    let deferred = store::open_memory().unwrap();
    let full = store::open_memory().unwrap();
    refresh_catalog(&deferred, home);
    refresh_full(&full, home);

    let deferred_row = catalog_row(&deferred, "conv-1");
    let full_row = catalog_row(&full, "conv-1");
    assert_eq!(deferred_row.title, full_row.title);
    assert_eq!(deferred_row.project, full_row.project);
    assert_eq!(deferred_row.model, full_row.model);
    assert_eq!(deferred_row.started_at, full_row.started_at);
    assert_eq!(deferred_row.ended_at, full_row.ended_at);
    assert_eq!(deferred_row.source_files, full_row.source_files);
    assert!(full_row.event_index_ready);
    assert!(!deferred_row.event_index_ready, "目录阶段只建会话行");

    let progress = crate::conversation::event_index_progress(&deferred).unwrap();
    assert_eq!((progress.indexed, progress.total), (0, 1));
    assert!(needs_backfill(&deferred, "conv-1"));
    assert!(
        !needs_backfill(&deferred, "missing"),
        "不存在的会话不需要补建"
    );
    assert!(
        events(&deferred, home, "conv-1")
            .iter()
            .any(|event| !event.details.is_null()),
        "补建前详情走整份解析"
    );

    assert!(crate::conversation::backfill_event_index_step(&deferred, home).unwrap());
    assert!(!needs_backfill(&deferred, "conv-1"));
    assert!(catalog_row(&deferred, "conv-1").event_index_ready);
    let progress = crate::conversation::event_index_progress(&deferred).unwrap();
    assert_eq!((progress.indexed, progress.total), (1, 1));
    assert_conversation_index_matches_parse(&deferred, home, "codex", "conv-1");
}

#[test]
fn rewritten_session_keeps_old_events_searchable_until_backfill_rewrites_them() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex(home, "conv-1");
    let conn = store::open_memory().unwrap();
    refresh_full(&conn, home);
    assert!(!needs_backfill(&conn, "conv-1"));

    rewrite_not_append(&path, "改完了。", "2026-08-20T00:02:30Z");
    refresh_catalog(&conn, home);

    let row = catalog_row(&conn, "conv-1");
    assert_eq!(row.ended_at, "2026-08-20T00:02:30Z", "会话行跟上源文件");
    assert!(!row.event_index_ready);
    assert!(needs_backfill(&conn, "conv-1"));
    let progress = crate::conversation::event_index_progress(&conn).unwrap();
    assert_eq!((progress.indexed, progress.total), (0, 1));

    let stale = search(&conn, "已完成提交");
    assert_eq!(stale.total, 1, "补建前正文搜索仍命中旧一代");
    assert_eq!(
        stale.rows[0].match_field,
        Some(ConversationMatchField::Body)
    );
    assert_eq!(search(&conn, "改完了").total, 0, "新正文要等补建");

    let fallback = events(&conn, home, "conv-1");
    assert!(fallback.iter().any(|event| !event.details.is_null()));
    assert!(fallback
        .iter()
        .any(|event| event.text.as_deref() == Some("改完了。")));
    let fallback_detail = crate::conversation::load_detail(&conn, home, "codex", "conv-1").unwrap();

    crate::conversation::backfill_event_index(&conn, home).unwrap();
    assert!(!needs_backfill(&conn, "conv-1"));
    let indexed = events(&conn, home, "conv-1");
    assert!(indexed.iter().all(|event| event.details.is_null()));
    assert_eq!(indexed, strip_details(fallback));
    let indexed_detail = crate::conversation::load_detail(&conn, home, "codex", "conv-1").unwrap();
    assert_eq!(indexed_detail.revision, fallback_detail.revision);
    assert_eq!(indexed_detail.session, fallback_detail.session);
    assert_eq!(indexed_detail.event_count, fallback_detail.event_count);
    assert_eq!(search(&conn, "改完了").total, 1);
    assert_eq!(search(&conn, "已完成提交").total, 0);
}

#[test]
fn unchanged_dirty_session_is_not_reparsed_by_the_next_catalog_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex(home, "conv-1");
    let conn = store::open_memory().unwrap();
    refresh_full(&conn, home);
    rewrite_not_append(&path, "改完了。", "2026-08-20T00:02:30Z");
    refresh_catalog(&conn, home);
    conn.execute(
        "UPDATE conversation_sessions SET title = 'kept' WHERE session_id = 'conv-1'",
        [],
    )
    .unwrap();

    refresh_catalog(&conn, home);

    assert_eq!(
        catalog_row(&conn, "conv-1").title,
        "kept",
        "源文件没再变化，目录阶段应跳过而不是再整份解析"
    );
    assert!(needs_backfill(&conn, "conv-1"));
}

#[test]
fn catalog_refresh_appends_to_clean_sessions_and_keeps_them_ready() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex(home, "conv-1");
    let conn = store::open_memory().unwrap();
    refresh_full(&conn, home);

    append_follow_up(&path);
    refresh_catalog(&conn, home);

    assert!(!needs_backfill(&conn, "conv-1"), "追加走增量，保持就绪");
    assert_eq!(search(&conn, "follow-up").total, 1);
    let indexed = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();
    assert_eq!(
        indexed.last().and_then(|event| event.text.as_deref()),
        Some("follow-up")
    );
    assert_conversation_index_matches_parse(&conn, home, "codex", "conv-1");
}

#[test]
fn dirty_session_does_not_append_until_backfill_rewrites_it() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex(home, "conv-1");
    let conn = store::open_memory().unwrap();
    refresh_full(&conn, home);
    rewrite_not_append(&path, "改完了。", "2026-08-20T00:02:30Z");
    refresh_catalog(&conn, home);

    append_follow_up(&path);
    refresh_catalog(&conn, home);

    assert!(needs_backfill(&conn, "conv-1"));
    assert_eq!(
        search(&conn, "follow-up").total,
        0,
        "待补建会话不得在旧一代事件后面接着追加"
    );
    assert_eq!(
        catalog_row(&conn, "conv-1").ended_at,
        "2026-08-20T00:04:00Z"
    );

    crate::conversation::backfill_event_index(&conn, home).unwrap();
    assert!(!needs_backfill(&conn, "conv-1"));
    assert_eq!(search(&conn, "follow-up").total, 1);
    assert_conversation_index_matches_parse(&conn, home, "codex", "conv-1");
}

#[test]
fn full_refresh_rewrites_a_dirty_session_whose_file_changed_again() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = seed_codex(home, "conv-1");
    let conn = store::open_memory().unwrap();
    refresh_full(&conn, home);
    rewrite_not_append(&path, "改完了。", "2026-08-20T00:02:30Z");
    refresh_catalog(&conn, home);
    assert!(needs_backfill(&conn, "conv-1"));

    let shorter = std::fs::read_to_string(&path)
        .unwrap()
        .replace("改完了。", "好。");
    std::fs::write(&path, shorter).unwrap();
    refresh_full(&conn, home);

    assert!(!needs_backfill(&conn, "conv-1"));
    assert_conversation_index_matches_parse(&conn, home, "codex", "conv-1");
}

#[test]
fn prioritized_backfill_takes_requested_sessions_before_newer_ones() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    for session_id in ["conv-1", "conv-2", "conv-3"] {
        seed_codex(home, session_id);
    }
    let conn = store::open_memory().unwrap();
    refresh_catalog(&conn, home);
    for (session_id, ended_at) in [
        ("conv-1", "2026-08-20T00:03:00Z"),
        ("conv-2", "2026-08-21T00:03:00Z"),
        ("conv-3", "2026-08-22T00:03:00Z"),
    ] {
        conn.execute(
            "UPDATE conversation_sessions SET ended_at = ?1 WHERE session_id = ?2",
            rusqlite::params![ended_at, session_id],
        )
        .unwrap();
    }
    let none = BTreeSet::new();
    let unready = |conn: &rusqlite::Connection| {
        ["conv-1", "conv-2", "conv-3"]
            .into_iter()
            .filter(|session_id| needs_backfill(conn, session_id))
            .collect::<Vec<_>>()
    };

    assert!(crate::conversation::backfill_event_index_step_prioritized(
        &conn,
        home,
        &none,
        &[key("missing"), key("conv-1")],
    )
    .unwrap());
    assert_eq!(unready(&conn), ["conv-2", "conv-3"], "优先队列里的会话先补");

    let skipped = BTreeSet::from([key("conv-2")]);
    assert!(crate::conversation::backfill_event_index_step_prioritized(
        &conn,
        home,
        &skipped,
        &[key("conv-2")],
    )
    .unwrap());
    assert_eq!(
        unready(&conn),
        ["conv-2"],
        "队列里的会话被跳过时按 ended_at 倒序取"
    );

    assert!(crate::conversation::backfill_event_index_step_prioritized(
        &conn,
        home,
        &none,
        &[key("conv-1")],
    )
    .unwrap());
    assert!(unready(&conn).is_empty(), "已就绪的队列项不再重复补建");
    assert!(
        !crate::conversation::backfill_event_index_step_prioritized(&conn, home, &none, &[])
            .unwrap()
    );
    for session_id in ["conv-1", "conv-2", "conv-3"] {
        assert_conversation_index_matches_parse(&conn, home, "codex", session_id);
    }
}

#[test]
fn backfill_without_priority_goes_newest_first_over_dirty_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let older = seed_codex(home, "conv-1");
    let newer = seed_codex(home, "conv-2");
    let conn = store::open_memory().unwrap();
    refresh_full(&conn, home);
    rewrite_not_append(&older, "改完了。", "2026-08-20T00:02:30Z");
    rewrite_not_append(&newer, "改完了。", "2026-08-22T00:02:30Z");
    refresh_catalog(&conn, home);
    assert!(needs_backfill(&conn, "conv-1") && needs_backfill(&conn, "conv-2"));

    assert!(crate::conversation::backfill_event_index_step(&conn, home).unwrap());
    assert!(!needs_backfill(&conn, "conv-2"), "结束更晚的会话先补");
    assert!(needs_backfill(&conn, "conv-1"));
}

fn claude_events(
    conn: &rusqlite::Connection,
    home: &std::path::Path,
    session_id: &str,
) -> Vec<crate::domain::ConversationEvent> {
    crate::conversation::load_events(
        conn,
        home,
        "claude",
        session_id,
        ConversationEventAnchor::First,
        200,
    )
    .unwrap()
    .events
}

/// 目录阶段建出的待补建会话，补建前走整份解析回退，补建后走索引，两条路逐条一致。
fn assert_claude_backfill_matches_fallback(home: &std::path::Path, session_id: &str) {
    let conn = store::open_memory().unwrap();
    let roots = ingest::source_scan_dirs(home, Source::Claude);
    let issues = crate::conversation::refresh_catalog(&conn, Source::Claude, &roots).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    assert!(
        crate::conversation::session_needs_event_backfill(&conn, "claude", session_id).unwrap()
    );

    let fallback = claude_events(&conn, home, session_id);
    assert!(!fallback.is_empty());
    assert!(fallback.iter().any(|event| !event.details.is_null()));
    let fallback_detail =
        crate::conversation::load_detail(&conn, home, "claude", session_id).unwrap();

    crate::conversation::backfill_event_index(&conn, home).unwrap();
    assert!(
        !crate::conversation::session_needs_event_backfill(&conn, "claude", session_id).unwrap()
    );
    let indexed = claude_events(&conn, home, session_id);
    assert!(indexed.iter().all(|event| event.details.is_null()));
    assert_eq!(indexed, strip_details(fallback));
    let indexed_detail =
        crate::conversation::load_detail(&conn, home, "claude", session_id).unwrap();
    assert_eq!(indexed_detail.revision, fallback_detail.revision);
    assert_eq!(indexed_detail.session, fallback_detail.session);
    assert_eq!(indexed_detail.event_count, fallback_detail.event_count);
    assert_conversation_index_matches_parse(&conn, home, "claude", session_id);
}

#[test]
fn claude_backfilled_events_match_the_fallback_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1.jsonl",
        "claude-conversation.jsonl",
    );
    assert_claude_backfill_matches_fallback(home, "claude-parent-1");
}

#[test]
fn claude_backfilled_events_match_the_fallback_parse_across_split_files() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".claude/projects/-workspace-split/claude-split-a.jsonl",
        "claude-split-session-a.jsonl",
    );
    write_home_fixture(
        home,
        ".claude/projects/-workspace-split/claude-split-b.jsonl",
        "claude-split-session-b.jsonl",
    );
    assert_claude_backfill_matches_fallback(home, "claude-split-1");
}
