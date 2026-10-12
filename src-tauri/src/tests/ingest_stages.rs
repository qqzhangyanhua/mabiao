use crate::domain::{ConversationQuery, ConversationSessionRow, IngestIssue};
use crate::ingest::{CatalogMode, CatalogProgress, PathOverrides};
use crate::test_support::*;
use rusqlite::Connection;

fn seed(home: &std::path::Path, relative_path: &str, contents: &str) -> std::path::PathBuf {
    let path = home.join(relative_path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
    path
}

fn seed_codex_usage_and_conversation(home: &std::path::Path) {
    seed(home, ".codex/sessions/one.jsonl", &fixture("codex.jsonl"));
    seed(
        home,
        ".codex/sessions/2026/08/rollout-conv-1.jsonl",
        &fixture("codex-conversation.jsonl"),
    );
}

fn catalog(conn: &Connection) -> Vec<ConversationSessionRow> {
    let query = ConversationQuery {
        page_size: Some(200),
        ..ConversationQuery::default()
    };
    let mut rows = crate::conversation::sessions_page(conn, &query)
        .unwrap()
        .rows;
    rows.sort_by(|a, b| (&a.source, &a.session_id).cmp(&(&b.source, &b.session_id)));
    rows
}

fn identities(rows: &[ConversationSessionRow]) -> Vec<(String, String)> {
    rows.iter()
        .map(|row| (row.source.clone(), row.session_id.clone()))
        .collect()
}

fn refresh_catalogs(
    conn: &Connection,
    home: &std::path::Path,
    mode: CatalogMode,
) -> (Vec<IngestIssue>, Vec<CatalogProgress>) {
    let mut progress = Vec::new();
    let issues = ingest::refresh_conversation_catalogs_with_overrides(
        || Ok::<_, String>(conn),
        home,
        &PathOverrides::new(),
        mode,
        |step| {
            // 回调发生在来源之间：上一个来源必须已经提交并释放，不能还挂着事务。
            assert!(
                conn.is_autocommit(),
                "对话目录阶段在来源之间仍持有事务：{step:?}"
            );
            progress.push(step);
        },
    )
    .unwrap();
    (issues, progress)
}

fn rollup_rows(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM usage_rollup", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn usage_phase_commits_records_and_rollup_but_leaves_the_conversation_catalog_alone() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_codex_usage_and_conversation(home);
    let conn = store::open_memory().unwrap();
    store::backfill_rollup(&conn).unwrap();

    let report = ingest::ingest_usage_with_overrides(&conn, home, &PathOverrides::new()).unwrap();

    assert!(conn.is_autocommit(), "用量阶段返回前必须已提交");
    assert_eq!(report.files_failed, 0);
    assert!(report.conversation_issues.is_empty());
    assert!(!report.partial_success);
    let records = store::load_all(&conn).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records.iter().map(|r| r.total_tokens).sum::<i64>(), 19113);
    assert!(rollup_rows(&conn) > 0, "用量阶段要同步预聚合");
    assert!(catalog(&conn).is_empty(), "用量阶段不该写对话目录");
}

#[test]
fn usage_phase_report_ignores_conversation_problems() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed(
        home,
        ".codex/sessions/2026/08/rollout-broken.jsonl",
        "{not-json\n",
    );
    let conn = store::open_memory().unwrap();

    let full = ingest::ingest_all_with_overrides(&conn, home, &PathOverrides::new()).unwrap();
    assert!(
        !full.conversation_issues.is_empty(),
        "前提：完整摄取应报出对话记录问题"
    );

    let conn = store::open_memory().unwrap();
    let usage = ingest::ingest_usage_with_overrides(&conn, home, &PathOverrides::new()).unwrap();
    assert!(usage.conversation_issues.is_empty());
    assert_eq!(
        usage.partial_success,
        usage.files_failed > 0 || !usage.issues.is_empty()
    );
}

#[test]
fn catalog_phase_after_usage_phase_publishes_new_conversation_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_codex_usage_and_conversation(home);
    let conn = store::open_memory().unwrap();
    ingest::ingest_usage_with_overrides(&conn, home, &PathOverrides::new()).unwrap();
    assert!(catalog(&conn).is_empty());

    let (issues, progress) = refresh_catalogs(&conn, home, CatalogMode::Full);

    assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    let rows = catalog(&conn);
    let conv = rows
        .iter()
        .find(|row| row.source == "codex" && row.session_id == "conv-1")
        .expect("新会话应出现在对话目录里");
    assert_eq!(conv.title, "发布 Tray 客户端版本支持图片编辑透传");
    assert_eq!(conv.model, "gpt-5.6-sol");
    assert!(!conv.started_at.is_empty());
    assert!(!conv.ended_at.is_empty());

    let total = progress.last().unwrap().total;
    assert!(total > 1);
    let expected: Vec<CatalogProgress> = (0..total)
        .map(|done| CatalogProgress {
            done,
            total,
            source: Some(progress[done].source.unwrap()),
            finished: false,
        })
        .chain(std::iter::once(CatalogProgress {
            done: total,
            total,
            source: None,
            finished: true,
        }))
        .collect();
    assert_eq!(progress, expected);
    let sources: std::collections::BTreeSet<&str> = progress
        .iter()
        .filter_map(|step| step.source.map(Source::as_str))
        .collect();
    assert_eq!(sources.len(), total, "每个来源只处理一次");
    assert!(sources.contains("codex"));
    assert!(sources.contains("claude"));
}

#[test]
fn catalog_phase_failure_in_one_source_keeps_its_catalog_and_commits_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed(
        home,
        ".codex/sessions/2026/08/rollout-conv-1.jsonl",
        &fixture("codex-conversation.jsonl"),
    );
    seed(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1.jsonl",
        &fixture("claude-conversation.jsonl"),
    );
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &PathOverrides::new()).unwrap();
    let before = catalog(&conn);
    let claude_before: Vec<_> = before
        .iter()
        .filter(|row| row.source == "claude")
        .cloned()
        .collect();
    assert!(!claude_before.is_empty(), "前提：Claude 会话已在目录里");

    seed(
        home,
        ".codex/sessions/2026/08/rollout-conv-2.jsonl",
        &fixture("codex-conversation.jsonl").replace("\"id\":\"conv-1\"", "\"id\":\"conv-2\""),
    );
    // 扫描根变成普通文件，Claude 的目录发现会整体失败。
    std::fs::rename(home.join(".claude/projects"), home.join(".claude/moved")).unwrap();
    std::fs::write(home.join(".claude/projects"), "not a directory").unwrap();

    let (issues, progress) = refresh_catalogs(&conn, home, CatalogMode::Full);

    assert!(progress.last().unwrap().finished);
    assert!(
        issues.iter().any(|issue| issue.source == "claude"),
        "Claude 失败要记成对话记录问题：{issues:?}"
    );
    assert!(issues.iter().all(|issue| issue.source == "claude"));
    let after = catalog(&conn);
    let ids = identities(&after);
    assert!(ids.contains(&("codex".to_string(), "conv-1".to_string())));
    assert!(
        ids.contains(&("codex".to_string(), "conv-2".to_string())),
        "其他来源照常提交"
    );
    let claude_after: Vec<_> = after
        .iter()
        .filter(|row| row.source == "claude")
        .cloned()
        .collect();
    assert_eq!(claude_after, claude_before, "失败来源保留上次的目录");
}

#[test]
fn ingest_all_matches_usage_phase_followed_by_catalog_phase() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    write_all_source_fixtures(home);
    let overrides = PathOverrides::new();

    let combined = store::open_memory().unwrap();
    let staged = store::open_memory().unwrap();
    for round in 0..2 {
        let combined_report =
            ingest::ingest_all_with_overrides(&combined, home, &overrides).unwrap();

        let mut staged_report =
            ingest::ingest_usage_with_overrides(&staged, home, &overrides).unwrap();
        let (issues, _) = refresh_catalogs(&staged, home, CatalogMode::Full);
        staged_report.partial_success = staged_report.partial_success || !issues.is_empty();
        staged_report.conversation_issues = issues;

        assert_eq!(combined_report, staged_report, "round {round}");
        assert_eq!(
            store::load_all(&combined).unwrap(),
            store::load_all(&staged).unwrap(),
            "round {round}"
        );
        assert_eq!(catalog(&combined), catalog(&staged), "round {round}");
        assert_eq!(
            rollup_rows(&combined),
            rollup_rows(&staged),
            "round {round}"
        );
    }
    assert!(!catalog(&combined).is_empty());
}

#[test]
fn deferred_catalog_mode_publishes_the_same_catalog_rows() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    write_all_source_fixtures(home);

    let full = store::open_memory().unwrap();
    ingest::ingest_usage_with_overrides(&full, home, &PathOverrides::new()).unwrap();
    let (full_issues, _) = refresh_catalogs(&full, home, CatalogMode::Full);

    let deferred = store::open_memory().unwrap();
    ingest::ingest_usage_with_overrides(&deferred, home, &PathOverrides::new()).unwrap();
    let (deferred_issues, _) = refresh_catalogs(&deferred, home, CatalogMode::Deferred);

    assert_eq!(full_issues, deferred_issues);
    let strip = |rows: Vec<ConversationSessionRow>| -> Vec<ConversationSessionRow> {
        rows.into_iter()
            .map(|row| ConversationSessionRow {
                event_index_ready: false,
                ..row
            })
            .collect()
    };
    assert_eq!(strip(catalog(&full)), strip(catalog(&deferred)));
}
