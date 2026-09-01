use crate::test_support::*;

#[test]
fn ingest_skips_unchanged_file_on_second_pass() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::write(session_dir.join("one.jsonl"), fixture("codex.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    let first = ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(first.files_parsed, 1);
    assert_eq!(first.files_skipped, 0);
    assert_eq!(first.records_written, 2);
    let second = ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(second.files_parsed, 0);
    assert_eq!(second.files_skipped, 1);
    assert_eq!(second.records_written, 0);
    let records = store::load_all(&conn).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records.iter().map(|r| r.total_tokens).sum::<i64>(), 19113);
}

#[test]
fn ingest_rewrites_changed_file_without_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    let path = session_dir.join("one.jsonl");
    std::fs::write(&path, fixture("codex.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();

    let mut changed = fixture("codex.jsonl");
    changed.push('\n');
    std::fs::write(&path, changed).unwrap();
    let second = ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(second.files_parsed, 1);
    assert_eq!(second.files_skipped, 0);
    assert_eq!(second.records_written, 2);

    let records = store::load_all(&conn).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records.iter().map(|r| r.total_tokens).sum::<i64>(), 19113);
}

#[test]
fn ingest_keeps_last_good_records_when_changed_jsonl_has_a_bad_line() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    let path = session_dir.join("one.jsonl");
    std::fs::write(&path, fixture("codex.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();

    let broken = format!("{}\n{{not-json", fixture("codex.jsonl"));
    std::fs::write(&path, broken).unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();

    assert_eq!(report.files_failed, 1);
    assert_eq!(report.files_parsed, 0);
    assert!(report.partial_success);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].source, "codex");
    let records = store::load_all(&conn).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records.iter().map(|r| r.total_tokens).sum::<i64>(), 19113);
}

#[test]
fn ingest_keeps_last_good_records_when_valid_jsonl_loses_usage_events() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    let path = session_dir.join("one.jsonl");
    let original = fixture("codex.jsonl");
    std::fs::write(&path, &original).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();

    let partial = original.lines().take(4).collect::<Vec<_>>().join("\n");
    std::fs::write(&path, partial).unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();

    assert_eq!(report.files_failed, 1);
    let records = store::load_all(&conn).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records
            .iter()
            .map(|record| record.total_tokens)
            .sum::<i64>(),
        19113
    );
}

#[test]
fn ingest_allows_record_count_drop_when_adapter_version_is_stale() {
    // ADAPTER_VERSION 升级后新适配器可能产出更少记录；条数下降保护只防截断，
    // 不该挡住版本升级触发的合法覆盖，否则会每轮重解析却永远卡在旧 version。
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    let path = session_dir.join("one.jsonl");
    std::fs::write(&path, fixture("codex.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(store::load_all(&conn).unwrap().len(), 2);

    let source_file = path.to_string_lossy().to_string();
    let mut extra = rec(
        "2025-11-18T16:35:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "codex",
        "legacy-extra",
        "legacy-session",
        2,
    );
    extra.source_file = source_file.clone();
    store::insert_records(&conn, &[extra]).unwrap();
    assert_eq!(store::record_count_for_file(&conn, &source_file).unwrap(), 3);
    conn.execute(
        "UPDATE ingested_files SET adapter_version = ?1 WHERE path = ?2",
        rusqlite::params![store::ADAPTER_VERSION - 1, source_file],
    )
    .unwrap();

    let report = ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(report.files_failed, 0, "issues={:?}", report.issues);
    assert_eq!(report.files_parsed, 1);
    assert_eq!(store::load_all(&conn).unwrap().len(), 2);
    let cached = store::cached_ingested_files(&conn).unwrap();
    let row = cached
        .iter()
        .find(|row| row.path == source_file)
        .expect("ingested file row");
    assert_eq!(row.adapter_version, store::ADAPTER_VERSION);

    let second = ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(second.files_skipped, 1);
    assert_eq!(second.files_parsed, 0);
    assert_eq!(second.files_failed, 0);
}

#[test]
fn ingest_keeps_last_good_records_when_changed_file_has_no_usage_records() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    let path = session_dir.join("one.jsonl");
    std::fs::write(&path, fixture("codex.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();

    std::fs::write(&path, "{}\n").unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();

    assert_eq!(report.files_failed, 1);
    assert_eq!(store::load_all(&conn).unwrap().len(), 2);
}

#[test]
fn source_with_a_failed_file_defers_deleted_file_reconciliation() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    let first = session_dir.join("one.jsonl");
    let second = session_dir.join("two.jsonl");
    std::fs::write(&first, fixture("codex.jsonl")).unwrap();
    std::fs::write(&second, fixture("codex.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(store::load_all(&conn).unwrap().len(), 4);

    std::fs::write(&first, "{not-json").unwrap();
    std::fs::remove_file(second).unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();

    assert_eq!(report.files_failed, 1);
    assert_eq!(report.records_removed, 0);
    assert_eq!(store::load_all(&conn).unwrap().len(), 4);
}

#[test]
fn ingest_archives_records_after_a_source_file_is_deleted() {
    // ADR 0004：源文件消失（工具自身清理/轮转）不再物理删除历史记录，只归档；
    // 归档记录仍然计入统计，直到用户显式清理。
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    let path = session_dir.join("one.jsonl");
    std::fs::write(&path, fixture("codex.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();

    std::fs::remove_file(path).unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();

    assert_eq!(report.records_removed, 0);
    assert_eq!(report.records_archived, 2);
    let records = store::load_all(&conn).unwrap();
    assert_eq!(records.len(), 2, "archived records still count in totals");
    assert_eq!(records.iter().map(|r| r.total_tokens).sum::<i64>(), 19113);

    // 幂等：再摄取一次不会重复归档同一批记录。
    let second = ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(second.records_archived, 0);
    assert_eq!(store::load_all(&conn).unwrap().len(), 2);

    let diagnostics = ingest::source_diagnostics(&conn, home).unwrap();
    let codex = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source == "codex")
        .unwrap();
    assert_eq!(codex.archived_record_count, 2);
    assert_eq!(codex.record_count, 2);
}

#[test]
fn ingest_replaces_archived_records_when_the_same_path_reappears() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    let path = session_dir.join("one.jsonl");
    std::fs::write(&path, fixture("codex.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();

    std::fs::remove_file(&path).unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(store::load_all(&conn).unwrap().len(), 2);

    // 文件在同一路径重新出现（比如从备份恢复），不应和归档快照重复计数。
    std::fs::write(&path, fixture("codex.jsonl")).unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(report.files_parsed, 1);
    let records = store::load_all(&conn).unwrap();
    assert_eq!(
        records.len(),
        2,
        "reappearing file replaces its archived snapshot"
    );
    assert!(records.iter().all(|r| r.total_tokens > 0));
}

#[test]
fn purge_archived_permanently_deletes_only_archived_records() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let codex_dir = home.join(".codex/sessions");
    let claude_dir = home.join(".claude/projects/project");
    std::fs::create_dir_all(&codex_dir).unwrap();
    std::fs::create_dir_all(&claude_dir).unwrap();
    let codex_path = codex_dir.join("one.jsonl");
    std::fs::write(&codex_path, fixture("codex.jsonl")).unwrap();
    std::fs::write(claude_dir.join("one.jsonl"), fixture("claude.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();

    std::fs::remove_file(&codex_path).unwrap();
    ingest::ingest_all(&conn, home).unwrap();

    // 按来源清理：只删 codex 的归档记录，claude 的活跃记录不受影响。
    let removed = store::purge_archived(&conn, Some(Source::Codex)).unwrap();
    assert_eq!(removed, 2);
    let records = store::load_all(&conn).unwrap();
    assert!(records.iter().all(|r| r.source == Source::Claude));

    let removed_again = store::purge_archived(&conn, Some(Source::Codex)).unwrap();
    assert_eq!(removed_again, 0);

    let removed_all = store::purge_archived(&conn, None).unwrap();
    assert_eq!(removed_all, 0, "claude records were never archived");
}

#[test]
fn kimi_sidecar_change_invalidates_unchanged_session_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_id = "bd1ab6fc-768d-4cff-b4c4-221a583c3af8";
    let wire = home.join(format!(".kimi/sessions/hash/{session_id}/wire.jsonl"));
    std::fs::create_dir_all(wire.parent().unwrap()).unwrap();
    std::fs::write(&wire, fixture("kimi-wire.jsonl")).unwrap();
    std::fs::write(
        home.join(".kimi/kimi.json"),
        format!(r#"{{"work_dirs":[{{"last_session_id":"{session_id}","path":"/project/one"}}]}}"#),
    )
    .unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert!(store::load_all(&conn)
        .unwrap()
        .iter()
        .all(|record| record.project == "/project/one"));

    std::fs::write(
        home.join(".kimi/kimi.json"),
        format!(r#"{{"work_dirs":[{{"last_session_id":"{session_id}","path":"/project/two"}}]}}"#),
    )
    .unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();

    assert_eq!(report.files_parsed, 1);
    assert!(store::load_all(&conn)
        .unwrap()
        .iter()
        .all(|record| record.project == "/project/two"));
}

#[test]
fn invalid_kimi_sidecar_keeps_last_good_project_mapping() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_id = "bd1ab6fc-768d-4cff-b4c4-221a583c3af8";
    let wire = home.join(format!(".kimi/sessions/hash/{session_id}/wire.jsonl"));
    let sidecar = home.join(".kimi/kimi.json");
    std::fs::create_dir_all(wire.parent().unwrap()).unwrap();
    std::fs::write(&wire, fixture("kimi-wire.jsonl")).unwrap();
    std::fs::write(
        &sidecar,
        format!(r#"{{"work_dirs":[{{"last_session_id":"{session_id}","path":"/project/good"}}]}}"#),
    )
    .unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();

    std::fs::write(&sidecar, "{not-json").unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    let records = store::load_all(&conn).unwrap();

    assert_eq!(report.files_failed, 1);
    assert_eq!(report.records_archived, 0);
    assert!(
        report.issues.iter().any(|issue| {
            issue.source == "kimi"
                && issue.path.ends_with("kimi.json")
                && issue.message.contains("Kimi 项目映射无效")
        }),
        "派生上下文失败应记为来源级诊断，实际 {:?}",
        report.issues
    );
    assert!(records
        .iter()
        .all(|record| record.project == "/project/good"));
}

#[test]
fn invalid_kimi_sidecar_defers_deleted_file_reconciliation() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let first_id = "bd1ab6fc-768d-4cff-b4c4-221a583c3af8";
    let second_id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let first = home.join(format!(".kimi/sessions/hash/{first_id}/wire.jsonl"));
    let second = home.join(format!(".kimi/sessions/hash/{second_id}/wire.jsonl"));
    let sidecar = home.join(".kimi/kimi.json");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::create_dir_all(second.parent().unwrap()).unwrap();
    std::fs::write(&first, fixture("kimi-wire.jsonl")).unwrap();
    std::fs::write(&second, fixture("kimi-wire.jsonl")).unwrap();
    std::fs::write(
        &sidecar,
        format!(r#"{{"work_dirs":[{{"last_session_id":"{first_id}","path":"/project/good"}}]}}"#),
    )
    .unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(store::load_all(&conn).unwrap().len(), 4);

    std::fs::write(&sidecar, "{not-json").unwrap();
    std::fs::remove_file(&second).unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();

    assert_eq!(report.files_failed, 1);
    assert_eq!(report.records_archived, 0);
    assert_eq!(store::load_all(&conn).unwrap().len(), 4);
}

#[test]
fn invalid_kimi_sidecar_fails_once_per_scan_dir() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let first = home.join(".kimi/sessions/hash/sess-a/wire.jsonl");
    let second = home.join(".kimi/sessions/hash/sess-b/wire.jsonl");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::create_dir_all(second.parent().unwrap()).unwrap();
    std::fs::write(&first, fixture("kimi-wire.jsonl")).unwrap();
    std::fs::write(&second, fixture("kimi-wire.jsonl")).unwrap();
    std::fs::write(home.join(".kimi/kimi.json"), "{not-json").unwrap();

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    let kimi = report
        .sources
        .iter()
        .find(|entry| entry.source == Source::Kimi.as_str())
        .unwrap();

    assert_eq!(report.files_failed, 1);
    assert_eq!(kimi.files_seen, 0);
    assert_eq!(kimi.files_failed, 1);
    assert!(report
        .issues
        .iter()
        .all(|issue| issue.path.ends_with("kimi.json")));
}

#[test]
fn invalid_kimi_sidecar_does_not_abort_other_sources() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_id = "bd1ab6fc-768d-4cff-b4c4-221a583c3af8";
    let wire = home.join(format!(".kimi/sessions/hash/{session_id}/wire.jsonl"));
    std::fs::create_dir_all(wire.parent().unwrap()).unwrap();
    std::fs::write(&wire, fixture("kimi-wire.jsonl")).unwrap();
    std::fs::write(home.join(".kimi/kimi.json"), "{not-json").unwrap();
    let session_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::write(session_dir.join("one.jsonl"), fixture("codex.jsonl")).unwrap();

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    let records = store::load_all(&conn).unwrap();

    assert_eq!(report.files_failed, 1);
    assert!(records.iter().any(|record| record.source == Source::Codex));
    assert!(records.iter().all(|record| record.source != Source::Kimi));
}

#[test]
fn grok_summary_model_change_rewrites_cached_records() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let updates = write_grok_session(home, "sess-a", "grok-one");
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert!(store::load_all(&conn)
        .unwrap()
        .iter()
        .all(|record| record.model == "grok-one"));

    std::fs::write(
        updates.parent().unwrap().join("summary.json"),
        r#"{"current_model_id":"grok-two"}"#,
    )
    .unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();

    assert_eq!(report.files_parsed, 1);
    assert!(store::load_all(&conn)
        .unwrap()
        .iter()
        .all(|record| record.model == "grok-two"));
}

#[test]
fn invalid_grok_summary_keeps_last_good_model_and_skips_current_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let updates = write_grok_session(home, "sess-a", "grok-good");
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert!(store::load_all(&conn)
        .unwrap()
        .iter()
        .all(|record| record.model == "grok-good"));

    std::fs::write(updates.parent().unwrap().join("summary.json"), "{not-json").unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    let records = store::load_all(&conn).unwrap();
    let grok = report
        .sources
        .iter()
        .find(|entry| entry.source == Source::Grok.as_str())
        .unwrap();

    assert_eq!(report.files_failed, 1);
    assert_eq!(grok.files_failed, 1);
    assert_eq!(grok.files_parsed, 0);
    assert_eq!(report.records_archived, 0);
    assert!(
        report.issues.iter().any(|issue| {
            issue.source == "grok" && issue.message.contains("Grok 模型摘要无效")
        }),
        "摘要失败应记为来源级诊断，实际 {:?}",
        report.issues
    );
    assert!(records.iter().all(|record| record.model == "grok-good"));
}

#[test]
fn invalid_grok_summary_is_diagnosed_on_sidecar_and_not_counted_as_seen() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let updates = write_grok_session(home, "sess-a", "grok-good");
    std::fs::write(updates.parent().unwrap().join("summary.json"), "{not-json").unwrap();

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    let grok = report
        .sources
        .iter()
        .find(|entry| entry.source == Source::Grok.as_str())
        .unwrap();

    assert_eq!(grok.files_seen, 0);
    assert_eq!(grok.files_failed, 1);
    assert!(
        report.issues.iter().any(|issue| {
            issue.source == "grok"
                && issue.path.ends_with("summary.json")
                && issue.message.contains("Grok 模型摘要无效")
        }),
        "摘要失败应记在 summary.json 上，实际 {:?}",
        report.issues
    );
}

#[test]
fn invalid_grok_summary_skips_current_file_without_failing_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let bad = write_grok_session(home, "sess-bad", "grok-bad");
    write_grok_session(home, "sess-good", "grok-good");
    std::fs::write(bad.parent().unwrap().join("summary.json"), "{not-json").unwrap();

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    let records = store::load_all(&conn).unwrap();
    let grok = report
        .sources
        .iter()
        .find(|entry| entry.source == Source::Grok.as_str())
        .unwrap();

    assert_eq!(report.files_failed, 1);
    assert_eq!(grok.files_failed, 1);
    assert_eq!(grok.files_seen, 1);
    assert_eq!(grok.files_parsed, 1);
    assert!(records.iter().all(|record| record.source == Source::Grok));
    assert!(records.iter().all(|record| record.model == "grok-good"));
    assert!(records
        .iter()
        .all(|record| record.session_id == "sess-good"));
}

#[test]
fn invalid_grok_summary_defers_deleted_file_reconciliation() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let first = write_grok_session(home, "sess-a", "grok-a");
    let second = write_grok_session(home, "sess-b", "grok-b");
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(store::load_all(&conn).unwrap().len(), 2);

    std::fs::write(first.parent().unwrap().join("summary.json"), "{not-json").unwrap();
    std::fs::remove_file(&second).unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();

    assert_eq!(report.files_failed, 1);
    assert_eq!(report.records_archived, 0);
    assert_eq!(store::load_all(&conn).unwrap().len(), 2);
}

#[test]
fn invalid_grok_summary_does_not_abort_other_sources() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let updates = write_grok_session(home, "sess-a", "grok-good");
    std::fs::write(updates.parent().unwrap().join("summary.json"), "{not-json").unwrap();
    let session_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::write(session_dir.join("one.jsonl"), fixture("codex.jsonl")).unwrap();

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    let records = store::load_all(&conn).unwrap();

    assert_eq!(report.files_failed, 1);
    assert!(records.iter().any(|record| record.source == Source::Codex));
    assert!(records.iter().all(|record| record.source != Source::Grok));
}

#[test]
fn grok_ingest_ignores_jsonl_that_is_not_updates() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_dir = home.join(".grok/sessions/proj/sess-a");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::write(
        session_dir.join("updates.jsonl"),
        fixture("grok-updates.jsonl"),
    )
    .unwrap();
    std::fs::write(
        session_dir.join("other.jsonl"),
        fixture("grok-updates.jsonl"),
    )
    .unwrap();
    std::fs::write(
        home.join(".grok/sessions/orphan.jsonl"),
        fixture("grok-updates.jsonl"),
    )
    .unwrap();

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    let records = store::load_all(&conn).unwrap();
    let grok = report
        .sources
        .iter()
        .find(|entry| entry.source == Source::Grok.as_str())
        .unwrap();

    assert_eq!(grok.files_seen, 1);
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .all(|record| record.source_file.ends_with("updates.jsonl")));
}

#[test]
fn grok_record_count_drop_is_not_treated_as_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let updates = home.join(".grok/sessions/proj/sess-a/updates.jsonl");
    std::fs::create_dir_all(updates.parent().unwrap()).unwrap();
    std::fs::write(&updates, fixture("grok-updates.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(store::load_all(&conn).unwrap().len(), 2);

    let partial = fixture("grok-updates.jsonl")
        .lines()
        .take(3)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&updates, partial).unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    let records = store::load_all(&conn).unwrap();

    assert_eq!(report.files_failed, 0);
    assert_eq!(report.files_parsed, 1);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].total_tokens, 26857);
}

fn write_grok_session(home: &std::path::Path, session: &str, model: &str) -> PathBuf {
    let updates = home.join(format!(".grok/sessions/proj/{session}/updates.jsonl"));
    std::fs::create_dir_all(updates.parent().unwrap()).unwrap();
    // 不带 modelUsage / modelId，消耗记录的模型名只能来自 summary.json。
    std::fs::write(
        &updates,
        r#"{"timestamp":1785938172913,"method":"_x.ai/session/update","params":{"update":{"sessionUpdate":"turn_completed","prompt_id":"p1","usage":{"inputTokens":10,"outputTokens":2,"totalTokens":12}}}}
"#,
    )
    .unwrap();
    std::fs::write(
        updates.parent().unwrap().join("summary.json"),
        format!(r#"{{"current_model_id":"{model}"}}"#),
    )
    .unwrap();
    updates
}

#[test]
fn kimi_ingest_ignores_jsonl_that_is_not_wire() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let session_id = "bd1ab6fc-768d-4cff-b4c4-221a583c3af8";
    let session_dir = home.join(format!(".kimi/sessions/hash/{session_id}"));
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::write(session_dir.join("wire.jsonl"), fixture("kimi-wire.jsonl")).unwrap();
    std::fs::write(session_dir.join("other.jsonl"), fixture("kimi-wire.jsonl")).unwrap();
    std::fs::write(
        home.join(".kimi/sessions/orphan.jsonl"),
        fixture("kimi-wire.jsonl"),
    )
    .unwrap();

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    let records = store::load_all(&conn).unwrap();
    let kimi = report
        .sources
        .iter()
        .find(|entry| entry.source == Source::Kimi.as_str())
        .unwrap();

    assert_eq!(kimi.files_seen, 1);
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .all(|record| record.source_file.ends_with("wire.jsonl")));
}

#[test]
fn rebuilding_one_source_keeps_other_sources_and_reparses_target() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let codex_dir = home.join(".codex/sessions");
    let claude_dir = home.join(".claude/projects/project");
    std::fs::create_dir_all(&codex_dir).unwrap();
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(codex_dir.join("one.jsonl"), fixture("codex.jsonl")).unwrap();
    std::fs::write(claude_dir.join("one.jsonl"), fixture("claude.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();

    let report = ingest::rebuild_cache(&conn, home, Some(Source::Codex)).unwrap();
    let records = store::load_all(&conn).unwrap();

    assert_eq!(report.files_parsed, 1);
    assert!(records.iter().any(|record| record.source == Source::Codex));
    assert!(records.iter().any(|record| record.source == Source::Claude));
}

#[test]
fn rebuild_keeps_last_good_records_when_target_file_is_broken() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let codex_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&codex_dir).unwrap();
    let path = codex_dir.join("one.jsonl");
    std::fs::write(&path, fixture("codex.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    std::fs::write(&path, "{not-json").unwrap();

    let report = ingest::rebuild_cache(&conn, home, Some(Source::Codex)).unwrap();
    let records = store::load_all(&conn).unwrap();

    assert_eq!(report.files_failed, 1);
    assert_eq!(records.len(), 2);
    assert_eq!(
        records
            .iter()
            .map(|record| record.total_tokens)
            .sum::<i64>(),
        19113
    );
}

#[test]
fn rebuilding_all_removes_unknown_source_records() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let conn = store::open_memory().unwrap();
    conn.execute_batch(
        r#"
        INSERT INTO usage_records (
            occurred_at, source, model, provider, project, session_id, source_file,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            reasoning_tokens, total_tokens, native_cost
        ) VALUES ('2026-01-01T00:00:00Z', 'future-source', '', '', '', 's', '/future', 1, 0, 0, 0, 0, 1, NULL);
        INSERT INTO ingested_files(path, mtime_ms, size, source, fingerprint, adapter_version)
        VALUES('/future', 1, 1, 'future-source', '', 1);
        "#,
    )
    .unwrap();
    assert!(store::load_all(&conn).is_err());

    let report = ingest::rebuild_cache(&conn, home, None).unwrap();

    assert_eq!(report.records_removed, 1);
    assert!(store::load_all(&conn).unwrap().is_empty());
}

#[test]
fn remove_unknown_sources_keeps_every_registered_source() {
    let conn = store::open_memory().unwrap();
    for source in Source::ALL {
        conn.execute(
            r#"
            INSERT INTO usage_records (
                occurred_at, source, model, provider, project, session_id, source_file,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                reasoning_tokens, total_tokens, native_cost
            ) VALUES ('2026-01-01T00:00:00Z', ?1, '', '', '', ?1, ?2, 1, 0, 0, 0, 0, 1, NULL)
            "#,
            rusqlite::params![source.as_str(), format!("/{}.jsonl", source.as_str())],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO ingested_files(path, mtime_ms, size, source, fingerprint, adapter_version)
            VALUES(?1, 1, 1, ?2, '', 1)
            "#,
            rusqlite::params![format!("/{}.jsonl", source.as_str()), source.as_str()],
        )
        .unwrap();
    }

    let removed = store::remove_unknown_sources(&conn).unwrap();
    assert_eq!(removed, 0);

    for source in Source::ALL {
        let (cached_files, record_count, _, _) = store::source_cache_stats(&conn, source).unwrap();
        assert_eq!(
            cached_files,
            1,
            "{} cached files were wiped",
            source.as_str()
        );
        assert_eq!(record_count, 1, "{} records were wiped", source.as_str());
    }
}

#[test]
fn source_diagnostics_explain_detection_cache_and_usage_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let codex_dir = home.join(".codex/sessions");
    std::fs::create_dir_all(&codex_dir).unwrap();
    std::fs::write(codex_dir.join("one.jsonl"), fixture("codex.jsonl")).unwrap();
    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();

    let diagnostics = ingest::source_diagnostics(&conn, home).unwrap();
    let codex = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source == "codex")
        .unwrap();
    let qwen = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source == "qwen")
        .unwrap();

    assert!(codex.detected);
    assert_eq!(codex.cached_files, 1);
    assert_eq!(codex.record_count, 2);
    assert_eq!(codex.total_tokens, 19113);
    assert_eq!(codex.coverage, "轮级 Token");
    assert!(!qwen.detected);
    assert_eq!(qwen.coverage, "本地无 Token");
    let kimi = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source == "kimi")
        .unwrap();
    assert_eq!(kimi.coverage, "轮级 Token（无模型名）");
    let grok = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source == "grok")
        .unwrap();
    assert_eq!(grok.coverage, "轮级 Token");
    let opencode = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source == "opencode")
        .unwrap();
    assert_eq!(opencode.coverage, "轮级 Token");

    let cursor_agent = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source == "cursor_agent")
        .unwrap();
    assert!(
        !cursor_agent.detected,
        "empty home should not report a fake usage directory as detected"
    );
    assert_eq!(
        cursor_agent.root_path,
        format!(
            "{}, {}",
            home.join(".cursor/chats").display(),
            home.join(".cursor/projects").display()
        )
    );
    assert_eq!(
        cursor_agent.coverage,
        "会话与 IDE 共用本机目录；token 仅包装落盘"
    );
}

#[test]
fn cursor_agent_diagnostics_detect_shared_cursor_dirs_not_usage_dir() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::create_dir_all(home.join(".cursor/chats")).unwrap();
    let conn = store::open_memory().unwrap();

    let diagnostics = ingest::source_diagnostics(&conn, home).unwrap();
    let cursor_agent = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source == "cursor_agent")
        .unwrap();
    assert!(cursor_agent.detected);
    assert!(cursor_agent.root_path.contains(".cursor/chats"));
    assert!(cursor_agent.root_path.contains(".cursor/projects"));
    assert!(
        !cursor_agent.root_path.contains(".cursor-agent-usage"),
        "usage capture dir is optional and must not be advertised when absent"
    );
    assert_eq!(
        ingest::source_scan_dirs_with(&ingest::PathOverrides::new(), home, Source::CursorAgent),
        vec![home.join(".cursor-agent-usage")],
    );
}

#[test]
fn rebuild_cursor_agent_cache_refreshes_local_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    let conn = store::open_memory().unwrap();
    ingest::rebuild_cache(&conn, home, Some(Source::CursorAgent)).unwrap();
    let summary = crate::cursor_session::load_summary(&conn).unwrap();
    assert_eq!(summary.session_count, 1);
}

#[test]
fn cursor_agent_conversation_index_uses_projects_dir_not_usage_override() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    let usage_dir = home.join("elsewhere-usage");
    std::fs::create_dir_all(&usage_dir).unwrap();
    let overrides = ingest::PathOverrides::from([("CURSOR_AGENT_USAGE_DIR", vec![usage_dir])]);
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &overrides).unwrap();
    let page =
        crate::conversation::sessions_page(&conn, &crate::domain::ConversationQuery::default())
            .unwrap();
    assert!(
        page.rows
            .iter()
            .any(|row| row.source == Source::CursorAgent.as_str() && row.session_id == "sess-1"),
        "对话目录必须读 ~/.cursor/projects，不能被 token 包装目录覆盖带走：{:?}",
        page.rows
            .iter()
            .map(|row| (row.source.as_str(), row.session_id.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn source_scan_dirs_default_to_home_relative_paths() {
    let home = std::path::Path::new("/home/example");
    let overrides = ingest::PathOverrides::new();

    assert_eq!(
        ingest::source_scan_dirs_with(&overrides, home, Source::Codex),
        vec![home.join(".codex/sessions")],
    );
    assert_eq!(
        ingest::source_scan_dirs_with(&overrides, home, Source::Grok),
        vec![home.join(".grok/sessions")],
    );
    assert_eq!(
        ingest::source_scan_dirs_with(&overrides, home, Source::Opencode),
        vec![home.join(".local/share/opencode/opencode.db")],
    );
    // Claude Code 有的安装方式写到 XDG 目录而不是 ~/.claude，默认两个都扫。
    assert_eq!(
        ingest::source_scan_dirs_with(&overrides, home, Source::Claude),
        vec![
            home.join(".claude/projects"),
            home.join(".config/claude/projects"),
        ],
    );
    assert_eq!(
        ingest::source_scan_dirs_with(&overrides, home, Source::Copilot),
        vec![home.join(".copilot/session-state")],
    );
    assert_eq!(
        ingest::source_scan_dirs_with(&overrides, home, Source::Omp),
        vec![home.join(".omp/agent/sessions")],
    );
}

#[test]
fn source_scan_dirs_env_override_replaces_defaults_with_same_leaf_join_rule() {
    let home = std::path::Path::new("/home/example");
    let overrides = ingest::PathOverrides::from([
        ("CODEX_HOME", vec![PathBuf::from("/custom/codex")]),
        (
            "CLAUDE_CONFIG_DIR",
            vec![
                PathBuf::from("/custom/claude-a"),
                PathBuf::from("/custom/claude-b"),
            ],
        ),
        ("GROK_HOME", vec![PathBuf::from("/custom/grok")]),
        ("OPENCODE_DATA_DIR", vec![PathBuf::from("/custom/opencode")]),
    ]);

    assert_eq!(
        ingest::source_scan_dirs_with(&overrides, home, Source::Codex),
        vec![PathBuf::from("/custom/codex/sessions")],
    );
    // 覆盖后不再回退到默认的 XDG 双路径，只扫用户显式给出的目录。
    assert_eq!(
        ingest::source_scan_dirs_with(&overrides, home, Source::Claude),
        vec![
            PathBuf::from("/custom/claude-a/projects"),
            PathBuf::from("/custom/claude-b/projects"),
        ],
    );
    assert_eq!(
        ingest::source_scan_dirs_with(&overrides, home, Source::Grok),
        vec![PathBuf::from("/custom/grok/sessions")],
    );
    assert_eq!(
        ingest::source_scan_dirs_with(&overrides, home, Source::Opencode),
        vec![PathBuf::from("/custom/opencode/opencode.db")],
    );
    // 未覆盖的 Source 仍然用默认路径。
    assert_eq!(
        ingest::source_scan_dirs_with(&overrides, home, Source::Pi),
        vec![home.join(".pi/agent/sessions")],
    );
}

#[test]
fn ingest_scans_multiple_overridden_directories_for_one_source() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();

    // 默认路径 home/.codex/sessions 放一份数据，用来验证覆盖后它不会再被扫到。
    let default_sessions = home.join(".codex/sessions");
    std::fs::create_dir_all(&default_sessions).unwrap();
    std::fs::write(
        default_sessions.join("ignored.jsonl"),
        fixture("codex.jsonl"),
    )
    .unwrap();

    // CODEX_HOME 覆盖为两个自定义根目录（逗号分隔多个），两个都要按同样的 /sessions
    // 规则拼接、都要被扫到。
    let root_a = home.join("codex-root-a");
    let root_b = home.join("codex-root-b");
    std::fs::create_dir_all(root_a.join("sessions")).unwrap();
    std::fs::create_dir_all(root_b.join("sessions")).unwrap();
    std::fs::write(root_a.join("sessions/a.jsonl"), fixture("codex.jsonl")).unwrap();
    std::fs::write(root_b.join("sessions/b.jsonl"), fixture("codex.jsonl")).unwrap();

    let overrides =
        ingest::PathOverrides::from([("CODEX_HOME", vec![root_a.clone(), root_b.clone()])]);

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &overrides).unwrap();

    assert_eq!(report.files_parsed, 2);
    let records = store::load_all(&conn).unwrap();
    assert_eq!(records.len(), 4);
    assert!(records.iter().all(|r| r.source == Source::Codex));
    assert!(records
        .iter()
        .all(|r| !r.source_file.contains("ignored.jsonl")));

    // 删掉其中一个根目录下的文件，reconcile 应该只处理那一份，另一份不受影响——
    // 说明多目录是合并到同一次对账里的，而不是互相独立、互不感知。
    // 按 ADR 0004，消失的文件只归档、不物理删除，归档记录仍计入统计。
    std::fs::remove_file(root_a.join("sessions/a.jsonl")).unwrap();
    let second = ingest::ingest_all_with_overrides(&conn, home, &overrides).unwrap();
    assert_eq!(second.records_removed, 0);
    assert_eq!(second.records_archived, 2);
    assert_eq!(
        store::load_all(&conn).unwrap().len(),
        4,
        "archived records still count in totals"
    );

    // 被归档的正好是 root_a 那一份：显式清理归档记录后，剩下的应当只有 root_b 的记录。
    assert_eq!(
        store::purge_archived(&conn, Some(Source::Codex)).unwrap(),
        2
    );
    let remaining = store::load_all(&conn).unwrap();
    assert_eq!(remaining.len(), 2);
    assert!(remaining
        .iter()
        .all(|r| r.source_file.contains("codex-root-b")));
}

#[test]
fn overview_matches_source_model_project_and_session_rollups() {
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &seed_records()).unwrap();
    let records = store::load_all(&conn).unwrap();
    assert_rollups_match_overview(&records, &Filter::default());

    let from_aug2 = Filter {
        from: Some("2026-08-02T00:00:00Z".into()),
        ..Filter::default()
    };
    let filtered = aggregate::overview(&records, &from_aug2, &PriceTable::default());
    assert_eq!(filtered.total_tokens, 350);
    assert_rollups_match_overview(&records, &from_aug2);
}

#[test]
fn write_all_source_fixtures_covers_every_registered_source() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    write_all_source_fixtures(home);
    let overrides = ingest::PathOverrides::new();

    assert_eq!(Source::ALL.len(), 13);
    let opencode_fixture = fixture("opencode.json");
    assert!(
        !opencode_fixture.contains("zhangyanhua") && !opencode_fixture.contains("/Users/"),
        "OpenCode 夹具必须脱敏，不能含真实用户路径"
    );
    let cursor_agent_written = std::fs::read_dir(home.join(".cursor-agent-usage"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .find(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
        .expect("Cursor Agent jsonl written");
    assert!(
        !cursor_agent_written.contains("zhangyanhua") && !cursor_agent_written.contains("/Users/"),
        "Cursor Agent 夹具必须脱敏，不能含真实用户路径"
    );
    for source in Source::ALL {
        let dirs = ingest::source_scan_dirs_with(&overrides, home, source);
        assert!(
            dirs.iter().any(|path| path.exists()),
            "{} 扫描路径在铺满夹具后不存在：{dirs:?}",
            source.as_str()
        );
    }

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &overrides).unwrap();
    assert_eq!(report.sources.len(), Source::ALL.len());
    for source in Source::ALL {
        let entry = report
            .sources
            .iter()
            .find(|entry| entry.source == source.as_str())
            .expect("ingest report has every registered source");
        assert!(
            entry.detected,
            "{} 铺满夹具后的摄取报告未标记 detected",
            source.as_str()
        );
        assert!(
            entry.files_seen >= 1,
            "{} 铺满夹具后的摄取报告 files_seen=0",
            source.as_str()
        );
    }
}

#[test]
fn ingest_all_fixtures_is_stable_on_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    write_all_source_fixtures(home);
    let conn = store::open_memory().unwrap();
    let overrides = ingest::PathOverrides::new();
    let first = ingest::ingest_all_with_overrides(&conn, home, &overrides).unwrap();
    let stored = store::load_all(&conn).unwrap();

    // 旧夹具覆盖 10 个来源时的常量；差值必须能归因到新增来源。
    const PREV_FILES: u64 = 10;
    const PREV_RECORDS: usize = 16;
    const PREV_TOKENS: i64 = 828446;
    const OPENCODE_FILES: u64 = 1;
    const OPENCODE_RECORDS: usize = 1;
    const OPENCODE_TOKENS: i64 = 20;
    const CURSOR_AGENT_FILES: u64 = 1;
    const CURSOR_AGENT_RECORDS: usize = 2;
    const CURSOR_AGENT_TOKENS: i64 = 19886;
    const OMP_FILES: u64 = 1;
    const OMP_RECORDS: usize = 2;
    const OMP_TOKENS: i64 = 199;

    let opencode = first
        .sources
        .iter()
        .find(|entry| entry.source == Source::Opencode.as_str())
        .unwrap();
    let cursor_agent = first
        .sources
        .iter()
        .find(|entry| entry.source == Source::CursorAgent.as_str())
        .unwrap();
    let omp = first
        .sources
        .iter()
        .find(|entry| entry.source == Source::Omp.as_str())
        .unwrap();
    assert_eq!(opencode.files_parsed, OPENCODE_FILES);
    assert_eq!(opencode.records_written, OPENCODE_RECORDS as u64);
    assert_eq!(cursor_agent.files_parsed, CURSOR_AGENT_FILES);
    assert_eq!(cursor_agent.records_written, CURSOR_AGENT_RECORDS as u64);
    assert_eq!(omp.files_parsed, OMP_FILES);
    assert_eq!(omp.records_written, OMP_RECORDS as u64);

    let opencode_rows: Vec<_> = stored
        .iter()
        .filter(|record| record.source == Source::Opencode)
        .collect();
    let cursor_agent_rows: Vec<_> = stored
        .iter()
        .filter(|record| record.source == Source::CursorAgent)
        .collect();
    let omp_rows: Vec<_> = stored
        .iter()
        .filter(|record| record.source == Source::Omp)
        .collect();
    assert_eq!(opencode_rows.len(), OPENCODE_RECORDS);
    assert_eq!(cursor_agent_rows.len(), CURSOR_AGENT_RECORDS);
    assert_eq!(omp_rows.len(), OMP_RECORDS);
    assert_eq!(
        opencode_rows
            .iter()
            .map(|record| record.total_tokens)
            .sum::<i64>(),
        OPENCODE_TOKENS
    );
    assert_eq!(
        cursor_agent_rows
            .iter()
            .map(|record| record.total_tokens)
            .sum::<i64>(),
        CURSOR_AGENT_TOKENS
    );
    assert_eq!(
        omp_rows
            .iter()
            .map(|record| record.total_tokens)
            .sum::<i64>(),
        OMP_TOKENS
    );

    let files = PREV_FILES + OPENCODE_FILES + CURSOR_AGENT_FILES + OMP_FILES;
    let records = PREV_RECORDS + OPENCODE_RECORDS + CURSOR_AGENT_RECORDS + OMP_RECORDS;
    let tokens = PREV_TOKENS + OPENCODE_TOKENS + CURSOR_AGENT_TOKENS + OMP_TOKENS;
    assert_eq!(first.files_parsed, files);
    assert_eq!(first.records_written, records as u64);
    assert_eq!(stored.len(), records);
    assert_eq!(stored.iter().map(|r| r.total_tokens).sum::<i64>(), tokens);
    assert_rollups_match_overview(&stored, &Filter::default());

    let second = ingest::ingest_all_with_overrides(&conn, home, &overrides).unwrap();
    assert_eq!(second.files_parsed, 0);
    assert_eq!(second.files_skipped, files);
    assert_eq!(second.records_written, 0);
    let again = store::load_all(&conn).unwrap();
    assert_eq!(again.len(), records);
    assert_eq!(again.iter().map(|r| r.total_tokens).sum::<i64>(), tokens);
}

#[test]
fn heartbeat_enumeration_matches_ingested_file_cache_for_all_sources() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    write_all_source_fixtures(home);
    let conn = store::open_memory().unwrap();
    let overrides = ingest::PathOverrides::new();
    ingest::ingest_all_with_overrides(&conn, home, &overrides).unwrap();

    let cached = store::cached_ingested_files(&conn).unwrap();
    let mut cached_by_source: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();
    for row in &cached {
        cached_by_source
            .entry(row.source.as_str())
            .or_default()
            .push(row.path.as_str());
    }
    for source in Source::ALL {
        let source_paths = cached_by_source
            .get(source.as_str())
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            source_paths.len(),
            1,
            "{} 全量摄取后 ingested_files 应为 1 条，实际 {:?}",
            source.as_str(),
            source_paths
        );
    }

    let cached_paths: std::collections::BTreeSet<String> =
        cached.iter().map(|row| row.path.clone()).collect();
    let watched_paths: std::collections::BTreeSet<String> =
        ingest::watched_usage_paths(home, &overrides)
            .unwrap()
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
    assert_eq!(
        watched_paths, cached_paths,
        "心跳枚举路径集合必须与 ingested_files 逐项相等（同一份 PathOverrides）"
    );

    // 夹具不含 Cursor 会话，会话侧新鲜度不会把过期判定从路径集合里带走。
    let cache = ingest::load_scan_cache(&conn).unwrap();
    assert!(
        !ingest::scan_is_stale_with_overrides(&cache, home, &overrides).unwrap(),
        "全量摄取后心跳必须不过期。缓存路径：{cached_paths:?}"
    );
}

#[test]
fn all_source_ingest_report_matches_behavior_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    write_all_source_fixtures(home);
    let conn = store::open_memory().unwrap();
    let overrides = ingest::PathOverrides::new();
    let report = ingest::ingest_all_with_overrides(&conn, home, &overrides).unwrap();
    let stored = store::load_all(&conn).unwrap();

    assert_eq!(report.files_seen, 13);
    assert_eq!(report.files_skipped, 0);
    assert_eq!(report.files_parsed, 13);
    assert_eq!(report.files_failed, 0);
    assert_eq!(report.records_written, 21);
    assert_eq!(report.records_archived, 0);
    assert_eq!(stored.len(), 21);
    assert_eq!(stored.iter().map(|r| r.total_tokens).sum::<i64>(), 848551);
    assert!(
        report.issues.is_empty(),
        "全来源夹具首次摄取不应产生诊断问题：{:?}",
        report.issues
    );

    assert_eq!(report.sources.len(), Source::ALL.len());
    for source in Source::ALL {
        let (
            files_seen,
            files_skipped,
            files_parsed,
            records_written,
            files_failed,
            records_archived,
        ) = match source {
            Source::Codex
            | Source::Claude
            | Source::Pi
            | Source::Omp
            | Source::Kimi
            | Source::Dsh
            | Source::Grok
            | Source::CursorAgent
            | Source::Copilot => (1, 0, 1, 2, 0, 0),
            Source::Opencode | Source::Gemini | Source::Factory => (1, 0, 1, 1, 0, 0),
            Source::Qwen => (1, 0, 1, 0, 0, 0),
        };
        let entry = report
            .sources
            .iter()
            .find(|entry| entry.source == source.as_str())
            .expect("ingest report has every registered source");
        assert_eq!(
            (
                entry.files_seen,
                entry.files_skipped,
                entry.files_parsed,
                entry.records_written,
                entry.files_failed,
                entry.records_archived
            ),
            (
                files_seen,
                files_skipped,
                files_parsed,
                records_written,
                files_failed,
                records_archived
            ),
            "{} 每来源报告与行为基线不一致",
            source.as_str()
        );
    }
}

#[test]
fn scan_is_stale_detects_new_changed_and_deleted_source_files() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let sessions = home.join(".codex/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let first = sessions.join("one.jsonl");
    std::fs::write(&first, fixture("codex.jsonl")).unwrap();

    let conn = store::open_memory().unwrap();
    assert!(
        ingest::scan_is_stale(&conn, home).unwrap(),
        "empty cache should be stale when source files exist"
    );

    ingest::ingest_all(&conn, home).unwrap();
    assert!(!ingest::scan_is_stale(&conn, home).unwrap());

    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    let file = std::fs::File::options().write(true).open(&first).unwrap();
    file.set_modified(later).unwrap();
    drop(file);
    assert!(
        ingest::scan_is_stale(&conn, home).unwrap(),
        "mtime change should be stale"
    );

    ingest::ingest_all(&conn, home).unwrap();
    assert!(!ingest::scan_is_stale(&conn, home).unwrap());

    std::fs::write(sessions.join("two.jsonl"), fixture("codex.jsonl")).unwrap();
    assert!(
        ingest::scan_is_stale(&conn, home).unwrap(),
        "new file should be stale"
    );

    ingest::ingest_all(&conn, home).unwrap();
    assert!(!ingest::scan_is_stale(&conn, home).unwrap());

    std::fs::remove_file(sessions.join("two.jsonl")).unwrap();
    assert!(
        ingest::scan_is_stale(&conn, home).unwrap(),
        "deleted cached file should be stale"
    );
}

#[test]
fn scan_is_stale_detects_kimi_and_grok_sidecar_changes() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();

    let wire = home.join(".kimi/sessions/hash/sess/wire.jsonl");
    std::fs::create_dir_all(wire.parent().unwrap()).unwrap();
    std::fs::write(&wire, fixture("kimi-wire.jsonl")).unwrap();
    std::fs::write(home.join(".kimi/kimi.json"), "{\"work_dirs\":[]}").unwrap();

    let updates = home.join(".grok/sessions/proj/sid/updates.jsonl");
    std::fs::create_dir_all(updates.parent().unwrap()).unwrap();
    std::fs::write(&updates, fixture("grok-updates.jsonl")).unwrap();
    std::fs::write(
        updates.parent().unwrap().join("summary.json"),
        "{\"current_model_id\":\"grok-4.5\"}",
    )
    .unwrap();

    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert!(!ingest::scan_is_stale(&conn, home).unwrap());

    std::fs::write(home.join(".kimi/kimi.json"), "{\"work_dirs\":[],\"x\":1}").unwrap();
    assert!(
        ingest::scan_is_stale(&conn, home).unwrap(),
        "kimi.json content change should be stale"
    );

    ingest::ingest_all(&conn, home).unwrap();
    assert!(!ingest::scan_is_stale(&conn, home).unwrap());

    std::fs::write(
        updates.parent().unwrap().join("summary.json"),
        "{\"current_model_id\":\"grok-4.5\",\"note\":\"x\"}",
    )
    .unwrap();
    assert!(
        ingest::scan_is_stale(&conn, home).unwrap(),
        "grok summary.json change should be stale"
    );
}

#[test]
fn scan_is_stale_detects_opencode_wal_change() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let db_path = home.join(".local/share/opencode/opencode.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute_batch("CREATE TABLE message (session_id TEXT, data TEXT);")
        .unwrap();
    drop(db);

    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert!(!ingest::scan_is_stale(&conn, home).unwrap());

    let wal = PathBuf::from(format!("{}-wal", db_path.to_string_lossy()));
    std::fs::write(&wal, b"wal").unwrap();
    assert!(
        ingest::scan_is_stale(&conn, home).unwrap(),
        "opencode.db-wal change should be stale"
    );
}

#[test]
fn usage_adapter_table_covers_every_registered_source_once() {
    use crate::adapters::{usage_adapter, usage_adapters};

    let adapters = usage_adapters();
    assert_eq!(
        adapters.len(),
        Source::ALL.len(),
        "适配器表不能多行也不能少行：人为删掉一行必须使本测试失败"
    );
    for source in Source::ALL {
        let matches = adapters
            .iter()
            .filter(|adapter| adapter.source == source)
            .count();
        assert_eq!(matches, 1, "{} 应在适配器表中恰好一行", source.as_str());
        assert_eq!(usage_adapter(source).source, source);
    }

    let grok = usage_adapter(Source::Grok);
    let opencode = usage_adapter(Source::Opencode);
    assert!(
        !grok.append_log,
        "Grok 不是追加型日志，迁表时不能顺手补全标记"
    );
    assert!(
        !opencode.append_log,
        "OpenCode 不是追加型日志，迁表时不能顺手补全标记"
    );
}

#[test]
fn usage_adapter_path_env_covers_every_registered_source_once() {
    use crate::adapters::usage_adapters;
    use std::collections::BTreeSet;

    let home = std::path::Path::new("/home/example");
    let mut seen = BTreeSet::new();
    for adapter in usage_adapters() {
        assert!(
            !adapter.path_env.is_empty(),
            "{} 必须登记路径覆盖环境变量",
            adapter.source.as_str()
        );
        assert!(
            seen.insert(adapter.path_env),
            "{} 与其它来源共用了路径环境变量 {}",
            adapter.source.as_str(),
            adapter.path_env
        );
        let sentinel = PathBuf::from(format!("/override/{}", adapter.source.as_str()));
        let overrides = ingest::PathOverrides::from([(adapter.path_env, vec![sentinel.clone()])]);
        let dirs = (adapter.scan_dirs)(&overrides, home);
        assert!(
            dirs.iter().any(|dir| dir.starts_with(&sentinel)),
            "{} 的 scan_dirs 未消费 path_env {}，实际 {:?}",
            adapter.source.as_str(),
            adapter.path_env,
            dirs
        );
    }
    assert_eq!(seen.len(), Source::ALL.len());
}

#[test]
fn opencode_ingest_opens_source_database_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let db_path = home.join(".local/share/opencode/opencode.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute_batch("CREATE TABLE message (session_id TEXT, data TEXT);")
        .unwrap();
    db.execute(
        "INSERT INTO message (session_id, data) VALUES (?1, ?2)",
        rusqlite::params![
            "sess-ro",
            r#"{"role":"assistant","modelID":"opencode-test","time":{"created":1787000000000,"completed":1787000001000},"tokens":{"input":3,"output":5,"reasoning":0,"cache":{"read":0,"write":0}},"path":{"cwd":"/workspace/opencode"}}"#,
        ],
    )
    .unwrap();
    drop(db);

    let mut permissions = std::fs::metadata(&db_path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&db_path, permissions).unwrap();
    let bytes_before = std::fs::read(&db_path).unwrap();

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    let opencode = report
        .sources
        .iter()
        .find(|entry| entry.source == Source::Opencode.as_str())
        .unwrap();

    assert_eq!(opencode.files_parsed, 1);
    assert_eq!(opencode.files_failed, 0);
    assert_eq!(store::load_all(&conn).unwrap().len(), 1);
    assert_eq!(std::fs::read(&db_path).unwrap(), bytes_before);
}

#[test]
fn opencode_record_count_drop_is_not_treated_as_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let db_path = home.join(".local/share/opencode/opencode.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute_batch("CREATE TABLE message (session_id TEXT, data TEXT);")
        .unwrap();
    db.execute(
        "INSERT INTO message (session_id, data) VALUES (?1, ?2)",
        rusqlite::params![
            "sess-a",
            r#"{"role":"assistant","modelID":"opencode-test","time":{"created":1787000000000,"completed":1787000001000},"tokens":{"input":3,"output":5,"reasoning":0,"cache":{"read":0,"write":0}},"path":{"cwd":"/workspace/opencode"}}"#,
        ],
    )
    .unwrap();
    drop(db);

    let conn = store::open_memory().unwrap();
    ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(store::load_all(&conn).unwrap().len(), 1);

    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute("DELETE FROM message", []).unwrap();
    drop(db);

    let report = ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(report.files_failed, 0);
    assert_eq!(report.files_parsed, 1);
    assert!(store::load_all(&conn).unwrap().is_empty());
}
