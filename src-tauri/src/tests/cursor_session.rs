use crate::test_support::*;

#[test]
fn cursor_session_adapter_counts_turns_tools_and_status() {
    let parsed = crate::adapters::cursor_session::parse_cursor_session_transcript(&fixture(
        "cursor-session-transcript.jsonl",
    ))
    .expect("fixture should parse");
    assert_eq!(parsed.turn_count, 2);
    assert_eq!(parsed.success_count, 1);
    assert_eq!(parsed.error_count, 1);
    assert_eq!(parsed.aborted_count, 0);
    assert_eq!(parsed.tool_calls.get("Read"), Some(&1));
    assert_eq!(parsed.tool_calls.get("Shell"), Some(&1));
}

#[test]
fn cursor_session_ingest_summarize_does_not_touch_usage_records() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);
    assert_eq!(report.files_parsed, 1);
    assert!(store::load_all(&conn).unwrap().is_empty());

    let summary = crate::cursor_session::load_summary(&conn).unwrap();
    assert_eq!(summary.session_count, 1);
    assert_eq!(summary.turn_count, 2);
    assert_eq!(summary.error_rate, Some(0.5));
    assert_eq!(summary.active_project_count, 1);
    assert_eq!(summary.by_project.len(), 1);
    assert_eq!(summary.by_project[0].name, "/Users/test/project");
    assert_eq!(summary.by_project[0].session_count, 1);
    assert_eq!(summary.by_project[0].turn_count, 2);
    assert_eq!(summary.by_project[0].error_count, 1);
    let page = crate::cursor_session::sessions_page(&conn, &CursorSessionQuery::default()).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].session_id, "sess-1");
    assert_eq!(page.rows[0].project, "/Users/test/project");
    assert_eq!(page.rows[0].turn_count, 2);
    assert_eq!(page.rows[0].error_count, 1);
    assert_eq!(page.rows[0].tool_call_count, 2);
    assert_eq!(summary.daily.len(), 1);
    assert_eq!(summary.daily[0].session_count, 1);
    assert_eq!(summary.daily[0].turn_count, 2);
}

#[test]
fn cursor_session_ingest_skips_unchanged_transcripts() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut first = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut first);
    assert_eq!(first.files_parsed, 1);
    assert_eq!(first.files_skipped, 0);

    let mut second = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut second);
    assert_eq!(second.files_parsed, 0);
    assert_eq!(second.files_skipped, 1);
}

#[test]
fn cursor_session_ingest_reparses_when_fingerprint_exists_but_session_row_missing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut first = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut first);
    assert_eq!(first.files_parsed, 1);
    assert_eq!(store::load_cursor_sessions(&conn).unwrap().len(), 1);

    conn.execute("DELETE FROM cursor_sessions", [])
        .expect("drop orphan session row");
    assert!(store::load_cursor_sessions(&conn).unwrap().is_empty());

    let mut again = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut again);
    assert_eq!(again.files_skipped, 0);
    assert_eq!(again.files_parsed, 1);
    let sessions = store::load_cursor_sessions(&conn).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "sess-1");
}

#[test]
fn cursor_session_ingest_reconciles_deleted_transcripts() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path = seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);
    assert_eq!(
        crate::cursor_session::load_summary(&conn)
            .unwrap()
            .session_count,
        1
    );

    std::fs::remove_file(path).expect("remove transcript");
    let mut again = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut again);
    assert_eq!(
        crate::cursor_session::load_summary(&conn)
            .unwrap()
            .session_count,
        0
    );
    assert_eq!(again.records_removed, 1);
}

#[test]
fn cursor_session_ingest_skips_reconcile_when_parse_failed() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path_one = seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    let path_two = seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-2",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut first = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut first);
    assert_eq!(
        crate::cursor_session::load_summary(&conn)
            .unwrap()
            .session_count,
        2
    );

    std::fs::remove_file(path_one).expect("remove first transcript");
    std::fs::write(&path_two, "{not-json").expect("corrupt second transcript");
    let mut failed = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut failed);
    assert_eq!(failed.files_failed, 1);
    assert_eq!(
        crate::cursor_session::load_summary(&conn)
            .unwrap()
            .session_count,
        2,
        "reconcile should be skipped while a transcript parse fails"
    );

    std::fs::write(&path_two, fixture("cursor-session-transcript.jsonl")).expect("fix transcript");
    let mut clean = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut clean);
    assert_eq!(clean.files_failed, 0);
    assert_eq!(
        crate::cursor_session::load_summary(&conn)
            .unwrap()
            .session_count,
        1
    );
    assert_eq!(clean.records_removed, 1);
}

#[test]
fn cursor_session_parse_failure_keeps_last_good_cache() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path = seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);
    assert_eq!(
        crate::cursor_session::load_summary(&conn)
            .unwrap()
            .turn_count,
        2
    );

    std::fs::write(&path, "{not-json").expect("write bad json");
    let mut bad = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut bad);
    assert_eq!(bad.files_failed, 1);
    assert_eq!(
        crate::cursor_session::load_summary(&conn)
            .unwrap()
            .turn_count,
        2
    );

    let mut unchanged_bad = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut unchanged_bad);
    assert_eq!(unchanged_bad.files_failed, 1);
    assert_eq!(
        crate::cursor_session::load_summary(&conn)
            .unwrap()
            .turn_count,
        2
    );
}

#[test]
fn cursor_session_enriches_from_ai_code_hashes() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    seed_ai_code_hashes(home, &[("sess-1", "grok-4.6", 1_784_511_794_686, "lib.rs")]);

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);

    let sessions = store::load_cursor_sessions(&conn).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].files_touched, 1);
    assert!(sessions[0].models_json.contains("grok-4.6"));
    assert!(sessions[0]
        .first_seen_at
        .as_deref()
        .unwrap()
        .contains("2026"));

    let summary = crate::cursor_session::load_summary(&conn).unwrap();
    assert_eq!(summary.by_model.len(), 1);
    assert_eq!(summary.by_model[0].name, "grok-4.6");
    assert_eq!(summary.by_model[0].session_count, 1);
    assert_eq!(summary.top_tools.len(), 2);
    assert_eq!(summary.top_tools[0].name, "Read");
    assert_eq!(summary.top_tools[0].call_count, 1);
}

#[test]
fn cursor_session_transcript_without_hash_stays_counted() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);

    let sessions = store::load_cursor_sessions(&conn).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].models_json, "[]");
    assert_eq!(sessions[0].files_touched, 0);
    assert_eq!(
        crate::cursor_session::load_summary(&conn)
            .unwrap()
            .session_count,
        1
    );
}

#[test]
fn cursor_session_orphan_hash_does_not_create_session() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_ai_code_hashes(
        home,
        &[("orphan-only", "grok-4.6", 1_784_511_794_686, "lib.rs")],
    );

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);

    assert!(store::load_cursor_sessions(&conn).unwrap().is_empty());
    assert_eq!(
        crate::cursor_session::load_summary(&conn)
            .unwrap()
            .session_count,
        0
    );
}

#[test]
fn cursor_session_hash_db_read_failure_keeps_last_enrichment() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    seed_ai_code_hashes(home, &[("sess-1", "grok-4.6", 1_784_511_794_686, "lib.rs")]);

    let conn = store::open_memory().unwrap();
    let mut first = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut first);
    let sessions = store::load_cursor_sessions(&conn).unwrap();
    assert_eq!(sessions[0].files_touched, 1);
    assert!(sessions[0].models_json.contains("grok-4.6"));

    let db_path = home.join(".cursor/ai-tracking/ai-code-tracking.db");
    std::fs::write(&db_path, "not-a-sqlite-database").expect("corrupt tracking db");
    let mut failed = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut failed);
    assert!(failed.files_failed >= 1 || !failed.issues.is_empty());

    let again = store::load_cursor_sessions(&conn).unwrap();
    assert_eq!(
        again[0].files_touched, 1,
        "transient hash db failure must not wipe enrichment"
    );
    assert!(again[0].models_json.contains("grok-4.6"));
}

#[test]
fn cursor_session_ingest_ignores_mcps_and_non_transcript_files() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    let project = home.join(".cursor/projects/Users-test-project");
    std::fs::create_dir_all(project.join("mcps")).expect("mcps dir");
    std::fs::write(project.join("mcps/server.json"), r#"{"tools":[]}"#).expect("mcp json");
    std::fs::write(
        project.join("mcps/noise.jsonl"),
        r#"{"type":"turn_ended","status":"success"}"#,
    )
    .expect("mcp jsonl");
    std::fs::write(
        project.join("random.jsonl"),
        r#"{"type":"turn_ended","status":"success"}"#,
    )
    .expect("root jsonl");

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);
    assert_eq!(report.files_seen, 1);
    assert_eq!(report.files_parsed, 1);
    assert_eq!(store::load_cursor_sessions(&conn).unwrap().len(), 1);
    assert!(!ingest::scan_is_stale(&conn, home).unwrap());
}

#[test]
fn cursor_session_enriches_existing_cache_when_tracking_db_appears() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut first = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut first);
    assert_eq!(
        store::load_cursor_sessions(&conn).unwrap()[0].models_json,
        "[]"
    );

    seed_ai_code_hashes(home, &[("sess-1", "grok-4.6", 1_784_511_794_686, "lib.rs")]);
    let mut second = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut second);
    assert!(store::load_cursor_sessions(&conn).unwrap()[0]
        .models_json
        .contains("grok-4.6"));
}

#[test]
fn cursor_session_skips_hash_reload_when_tracking_fingerprint_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    seed_ai_code_hashes(home, &[("sess-1", "grok-4.6", 1_784_511_794_686, "lib.rs")]);

    let conn = store::open_memory().unwrap();
    let mut first = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut first);
    assert!(store::load_cursor_sessions(&conn).unwrap()[0]
        .models_json
        .contains("grok-4.6"));

    let db_path = home.join(".cursor/ai-tracking/ai-code-tracking.db");
    let meta = std::fs::metadata(&db_path).expect("tracking meta");
    let modified = meta.modified().expect("mtime");
    std::fs::write(&db_path, vec![b'x'; meta.len() as usize]).expect("corrupt same size");
    let file = std::fs::File::options()
        .write(true)
        .open(&db_path)
        .expect("reopen tracking db");
    file.set_modified(modified).expect("restore mtime");
    drop(file);

    let mut second = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut second);
    assert_eq!(second.files_failed, 0);
    assert!(second.issues.is_empty());
    let again = store::load_cursor_sessions(&conn).unwrap();
    assert!(
        again[0].models_json.contains("grok-4.6"),
        "unchanged tracking fingerprint must skip hash reload"
    );
}

#[test]
fn scan_is_stale_detects_cursor_transcript_and_tracking_db() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    assert!(
        ingest::scan_is_stale(&conn, home).unwrap(),
        "unseen cursor transcript should be stale"
    );
    ingest::ingest_all(&conn, home).unwrap();
    assert!(!ingest::scan_is_stale(&conn, home).unwrap());

    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-2",
        &fixture("cursor-session-transcript.jsonl"),
    );
    assert!(
        ingest::scan_is_stale(&conn, home).unwrap(),
        "new cursor transcript should be stale"
    );

    ingest::ingest_all(&conn, home).unwrap();
    assert!(!ingest::scan_is_stale(&conn, home).unwrap());

    seed_ai_code_hashes(home, &[("sess-1", "grok-4.6", 1_784_511_794_686, "lib.rs")]);
    assert!(
        ingest::scan_is_stale(&conn, home).unwrap(),
        "ai-code-tracking.db change should be stale"
    );
}

#[test]
fn session_model_uses_latest_occurred_at_not_lexicographic_max() {
    let records = vec![
        rec(
            "2026-08-01T11:00:00.000Z",
            Source::Claude,
            "aaa-new",
            "anthropic",
            "/proj-new",
            "shared",
            20,
        ),
        rec(
            "2026-08-01T10:00:00.000Z",
            Source::Claude,
            "zzz-old",
            "anthropic",
            "/proj-old",
            "shared",
            10,
        ),
    ];
    let prices = PriceTable::default();
    let mem = aggregate::top_sessions(&records, &Filter::default(), &prices, 10);
    assert_eq!(mem.len(), 1);
    assert_eq!(mem[0].model, "aaa-new");
    assert_eq!(mem[0].project, "/proj-new");

    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let sql = query::top_sessions(&conn, &Filter::default(), &prices, 10).unwrap();
    assert_eq!(sql, mem);

    let page = query::sessions_page(
        &conn,
        &prices,
        &SessionQuery {
            page: Some(1),
            page_size: Some(20),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page.rows[0].model, "aaa-new");
    assert_eq!(page.rows[0].project, "/proj-new");
}

#[test]
fn sessions_page_skips_later_empty_labels_and_breaks_ties() {
    let mut early = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "old-model",
        "official",
        "/old",
        "s1",
        10,
    );
    early.source_file = "/old.jsonl".into();
    let mut blank = rec(
        "2026-08-01T11:00:00Z",
        Source::Codex,
        "",
        "official",
        "",
        "s1",
        10,
    );
    blank.source_file = String::new();
    let mut late_a = rec(
        "2026-08-01T12:00:00Z",
        Source::Codex,
        "aaa-model",
        "official",
        "/aaa",
        "s1",
        10,
    );
    late_a.source_file = "/aaa.jsonl".into();
    let mut late_z = rec(
        "2026-08-01T12:00:00Z",
        Source::Codex,
        "zzz-model",
        "official",
        "/zzz",
        "s1",
        10,
    );
    late_z.source_file = "/zzz.jsonl".into();
    let mut later_empty = rec(
        "2026-08-01T13:00:00Z",
        Source::Codex,
        "",
        "official",
        "",
        "s1",
        10,
    );
    later_empty.source_file = String::new();

    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &[early, blank, late_a, late_z, later_empty]).unwrap();
    let page = query::sessions_page(
        &conn,
        &PriceTable::default(),
        &SessionQuery {
            page: Some(1),
            page_size: Some(20),
            include_cost: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].model, "zzz-model");
    assert_eq!(page.rows[0].project, "/zzz");
    assert_eq!(page.rows[0].source_file, "/zzz.jsonl");
    assert_eq!(page.total_tokens, 50);
}

#[test]
fn sessions_page_stays_fast_on_many_sessions() {
    let conn = store::open_memory().unwrap();
    let mut records = Vec::with_capacity(18_000);
    for session in 0..1_500 {
        for turn in 0..12 {
            records.push(rec(
                &format!("2026-08-01T10:{turn:02}:{session:02}Z"),
                Source::Codex,
                "gpt",
                "official",
                "/proj",
                &format!("s{session}"),
                1,
            ));
        }
    }
    store::insert_records(&conn, &records).unwrap();
    let started = std::time::Instant::now();
    let page = query::sessions_page(
        &conn,
        &PriceTable::default(),
        &SessionQuery {
            page: Some(1),
            page_size: Some(20),
            include_cost: Some(true),
            sort_by: Some("time".into()),
            sort_dir: Some("desc".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(page.total, 1_500);
    assert_eq!(page.rows.len(), 20);
    assert_eq!(page.total_tokens, 18_000);
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "sessions_page took {elapsed:?}"
    );
}

#[test]
fn cursor_sessions_page_supports_search_sort_and_pagination() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    seed_cursor_transcript(
        home,
        "Users-other-project",
        "sess-2",
        &fixture("cursor-session-transcript.jsonl"),
    );
    seed_ai_code_hashes(
        home,
        &[
            ("sess-1", "grok-4.6", 1_784_511_794_686, "lib.rs"),
            ("sess-2", "gpt-5.4", 1_784_511_794_687, "main.rs"),
        ],
    );

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);

    let page1 = crate::cursor_session::sessions_page(
        &conn,
        &CursorSessionQuery {
            page: Some(1),
            page_size: Some(1),
            sort_by: Some("session".into()),
            sort_dir: Some("asc".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page1.total, 2);
    assert_eq!(page1.rows.len(), 1);
    assert_eq!(page1.rows[0].session_id, "sess-1");

    let page2 = crate::cursor_session::sessions_page(
        &conn,
        &CursorSessionQuery {
            page: Some(2),
            page_size: Some(1),
            sort_by: Some("session".into()),
            sort_dir: Some("asc".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page2.rows[0].session_id, "sess-2");

    let searched = crate::cursor_session::sessions_page(
        &conn,
        &CursorSessionQuery {
            search: Some("other".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(searched.total, 1);
    assert_eq!(searched.rows[0].session_id, "sess-2");

    let by_project = crate::cursor_session::sessions_page(
        &conn,
        &CursorSessionQuery {
            project: Some("/Users/test/project".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_project.total, 1);
    assert_eq!(by_project.rows[0].session_id, "sess-1");
    assert!(by_project.rows[0]
        .models
        .iter()
        .any(|name| name == "grok-4.6"));
}

#[test]
fn cursor_session_rolls_subagents_into_parent() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    seed_cursor_subagent(
        home,
        "Users-test-project",
        "sess-1",
        "child-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);

    let sessions = store::load_cursor_sessions(&conn).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "sess-1");
    assert_eq!(sessions[0].subagent_count, 1);
    assert_eq!(sessions[0].turn_count, 4);
    assert_eq!(sessions[0].user_prompt_count, 2);
    assert_eq!(sessions[0].error_count, 2);

    let summary = crate::cursor_session::load_summary(&conn).unwrap();
    assert_eq!(summary.session_count, 1);
    assert_eq!(summary.subagent_count, 1);
    assert_eq!(summary.turn_count, 4);
}

#[test]
fn cursor_session_dropping_subagent_updates_parent() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    let child = seed_cursor_subagent(
        home,
        "Users-test-project",
        "sess-1",
        "child-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut first = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut first);
    assert_eq!(
        store::load_cursor_sessions(&conn).unwrap()[0].subagent_count,
        1
    );

    std::fs::remove_file(child).expect("remove subagent");
    let mut again = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut again);
    let session = &store::load_cursor_sessions(&conn).unwrap()[0];
    assert_eq!(session.subagent_count, 0);
    assert_eq!(session.turn_count, 2);
}

#[test]
fn cursor_session_orphan_subagent_does_not_create_session() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_subagent(
        home,
        "Users-test-project",
        "sess-missing",
        "child-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);
    assert!(store::load_cursor_sessions(&conn).unwrap().is_empty());
    assert_eq!(
        crate::cursor_session::load_summary(&conn)
            .unwrap()
            .session_count,
        0
    );
}

#[test]
fn cursor_session_reconcile_drops_legacy_subagent_rows() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    let child = seed_cursor_subagent(
        home,
        "Users-test-project",
        "sess-1",
        "child-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let legacy = crate::domain::CursorSessionRecord {
        session_id: "child-1".into(),
        project: "/Users/test/project".into(),
        turn_count: 2,
        success_count: 1,
        error_count: 1,
        aborted_count: 0,
        user_prompt_count: 1,
        subagent_count: 0,
        tool_calls_json: "{}".into(),
        models_json: "[]".into(),
        sources_json: "[]".into(),
        extensions_json: "{}".into(),
        first_seen_at: None,
        last_seen_at: None,
        files_touched: 0,
        source_file: child.to_string_lossy().into_owned(),
    };
    store::upsert_cursor_session(&conn, &legacy).unwrap();
    assert_eq!(store::load_cursor_sessions(&conn).unwrap().len(), 1);

    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);
    let sessions = store::load_cursor_sessions(&conn).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "sess-1");
    assert_eq!(report.records_removed, 1);
}

#[test]
fn cursor_session_schema_bump_reparses_unchanged_files() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );

    let conn = store::open_memory().unwrap();
    let mut first = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut first);
    assert_eq!(
        store::load_cursor_sessions(&conn).unwrap()[0].user_prompt_count,
        1
    );

    conn.execute("UPDATE cursor_sessions SET user_prompt_count = 0", [])
        .unwrap();
    store::set_cursor_session_schema_version(&conn, "").unwrap();

    let mut again = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut again);
    assert_eq!(again.files_skipped, 0);
    assert_eq!(
        store::load_cursor_sessions(&conn).unwrap()[0].user_prompt_count,
        1
    );
    assert_eq!(
        store::cursor_session_schema_version(&conn).unwrap(),
        store::CURSOR_SESSION_SCHEMA_VERSION
    );
}

#[test]
fn cursor_session_parse_counts_user_prompts() {
    let parsed = crate::adapters::cursor_session::parse_cursor_session_transcript(
        r#"
{"role":"user","message":{"content":[{"type":"text","text":"one"}]}}
{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Read"}]}}
{"type":"turn_ended","status":"success"}
{"role":"user","message":{"content":[{"type":"text","text":"two"}]}}
{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Grep"},{"type":"tool_use","name":"StrReplace"},{"type":"tool_use","name":"Shell"}]}}
{"type":"turn_ended","status":"error"}
"#,
    )
    .unwrap();
    assert_eq!(parsed.user_prompt_count, 2);
    assert_eq!(parsed.turn_count, 2);
    assert_eq!(parsed.error_count, 1);
    assert_eq!(parsed.tool_calls.get("Read"), Some(&1));
}

#[test]
fn cursor_session_enriches_source_and_extension() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    seed_ai_code_hash_details(
        home,
        &[
            (
                "sess-1",
                "grok-4.6",
                1_784_511_794_686,
                "src/lib.rs",
                "cli",
                "rs",
            ),
            (
                "sess-1",
                "grok-4.6",
                1_784_511_794_687,
                "src/main.ts",
                "cli",
                "ts",
            ),
        ],
    );

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);
    let session = &store::load_cursor_sessions(&conn).unwrap()[0];
    assert!(session.sources_json.contains("cli"));
    assert!(session.extensions_json.contains("rs"));
    assert!(session.extensions_json.contains("ts"));
    assert_eq!(session.files_touched, 2);

    let summary = crate::cursor_session::load_summary(&conn).unwrap();
    assert_eq!(summary.by_source[0].name, "cli");
    assert_eq!(summary.by_extension.len(), 2);
}

#[test]
fn cursor_tool_group_maps_known_names() {
    assert_eq!(crate::adapters::cursor_session::tool_group("Read"), "read");
    assert_eq!(
        crate::adapters::cursor_session::tool_group("StrReplace"),
        "write"
    );
    assert_eq!(
        crate::adapters::cursor_session::tool_group("Shell"),
        "shell"
    );
    assert_eq!(
        crate::adapters::cursor_session::tool_group("WebFetch"),
        "web"
    );
    assert_eq!(crate::adapters::cursor_session::tool_group("Task"), "agent");
    assert_eq!(
        crate::adapters::cursor_session::tool_group("TodoWrite"),
        "other"
    );
}

#[test]
fn code_volume_summarize_keeps_composer_percentage_and_adds_deletes() {
    let summary = summarize_code_volume(&parse_cursor_commits(&[
        CursorCommitRow {
            commit_hash: "a".into(),
            branch: "main".into(),
            scored_at_ms: 1_784_511_794_686,
            commit_message: "feat".into(),
            lines_added: 100,
            lines_deleted: 40,
            composer_lines_added: 80,
            tab_lines_added: 5,
            human_lines_added: 10,
            ..Default::default()
        },
        CursorCommitRow {
            commit_hash: "b".into(),
            branch: "dev".into(),
            scored_at_ms: 1_784_598_194_686,
            lines_added: 20,
            lines_deleted: 5,
            composer_lines_added: 10,
            ..Default::default()
        },
    ]));
    assert_eq!(summary.lines_added, 120);
    assert_eq!(summary.lines_deleted, 45);
    assert_eq!(summary.net_lines, 75);
    assert_eq!(summary.tab_lines_added, 5);
    assert!((summary.ai_percentage.unwrap() - 75.0).abs() < 1e-9);
    assert_eq!(summary.by_branch[0].name, "main");
    assert_eq!(summary.daily.len(), 2);
    assert_eq!(summary.commits.len(), 2);
}

#[test]
fn cursor_session_parse_collects_paths_not_command_or_contents() {
    let parsed = crate::adapters::cursor_session::parse_cursor_session_transcript(
        r#"
{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"path":"src/a.rs"}},{"type":"tool_use","name":"Grep","input":{"paths":["src/b.rs","src/c.rs"],"pattern":"fn"}},{"type":"tool_use","name":"Write","input":{"path":"src/d.rs","contents":"SECRET_BODY"}},{"type":"tool_use","name":"Shell","input":{"command":"rm -rf /"}}]}}
{"type":"turn_ended","status":"success"}
"#,
    )
    .unwrap();
    assert_eq!(
        parsed.read_paths.iter().cloned().collect::<Vec<_>>(),
        vec!["src/a.rs", "src/b.rs", "src/c.rs"]
    );
    assert_eq!(
        parsed.write_paths.iter().cloned().collect::<Vec<_>>(),
        vec!["src/d.rs"]
    );
    assert!(!parsed
        .write_paths
        .iter()
        .any(|path| path.contains("SECRET_BODY")));
    assert!(!parsed.read_paths.iter().any(|path| path.contains("rm -rf")));
}

#[test]
fn cursor_session_detail_reads_tools_files_and_paths() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path = seed_cursor_transcript(
        home,
        "Users-test-project",
        "sess-1",
        &fixture("cursor-session-transcript.jsonl"),
    );
    seed_cursor_subagent(
        home,
        "Users-test-project",
        "sess-1",
        "child-1",
        r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Write","input":{"path":"src/new.rs","contents":"nope"}}]}}
{"type":"turn_ended","status":"success"}
"#,
    );
    seed_ai_code_hash_details(
        home,
        &[("sess-1", "grok-4.6", 1, "src/lib.rs", "cli", "rs")],
    );

    let conn = store::open_memory().unwrap();
    let mut report = crate::domain::IngestReport::default();
    crate::cursor_session::ingest(&conn, home, &mut report);

    let detail =
        crate::cursor_session_detail::load_detail(&conn, home, &path.to_string_lossy()).unwrap();
    assert_eq!(detail.session.session_id, "sess-1");
    assert!(detail.tools.iter().any(|row| row.name == "Read"));
    assert_eq!(detail.read_paths, vec!["src/lib.rs"]);
    assert_eq!(detail.write_paths, vec!["src/new.rs"]);
    assert_eq!(detail.hash_files.len(), 1);
    assert_eq!(detail.hash_files[0].path, "src/lib.rs");
    assert!(!detail.transcript_missing);

    std::fs::remove_file(&path).unwrap();
    let missing =
        crate::cursor_session_detail::load_detail(&conn, home, &path.to_string_lossy()).unwrap();
    assert!(missing.transcript_missing);
    assert!(missing.tools.iter().any(|row| row.name == "Read"));
    assert!(missing.read_paths.is_empty());
}

#[test]
fn cursor_account_events_page_orders_newest_first() {
    let events = cursor_account::parse_cursor_usage_events(&fixture("cursor_account_usage.json"))
        .expect("parse");
    let conn = store::open_memory().unwrap();
    store::upsert_cursor_account_events(&conn, &events).unwrap();

    let page = crate::cursor_account::events_page(
        &conn,
        &crate::domain::CursorAccountEventQuery {
            page: Some(1),
            page_size: Some(1),
            sort_dir: Some("desc".into()),
        },
    )
    .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.rows.len(), 1);
    let later = page.rows[0].occurred_at.clone();

    let asc = crate::cursor_account::events_page(
        &conn,
        &crate::domain::CursorAccountEventQuery {
            page: Some(1),
            page_size: Some(20),
            sort_dir: Some("asc".into()),
        },
    )
    .unwrap();
    assert_eq!(asc.rows.last().unwrap().occurred_at, later);
}

fn session_with_prompts(
    session_id: &str,
    user_prompt_count: i64,
) -> crate::domain::CursorSessionRecord {
    crate::domain::CursorSessionRecord {
        session_id: session_id.into(),
        project: "/tmp/demo".into(),
        turn_count: user_prompt_count.max(1),
        success_count: user_prompt_count.max(1),
        error_count: 0,
        aborted_count: 0,
        user_prompt_count,
        subagent_count: 0,
        tool_calls_json: "{}".into(),
        models_json: "[]".into(),
        sources_json: "[]".into(),
        extensions_json: "{}".into(),
        first_seen_at: None,
        last_seen_at: None,
        files_touched: 0,
        source_file: format!("/tmp/{session_id}.jsonl"),
    }
}

#[test]
fn summarize_all_single_prompt_sessions_ratio_is_one() {
    let summary = crate::cursor_session::summarize_cursor_sessions(&[
        session_with_prompts("s1", 1),
        session_with_prompts("s2", 1),
    ]);
    assert_eq!(summary.single_prompt_ratio, Some(1.0));
}

#[test]
fn summarize_all_multi_prompt_sessions_ratio_is_zero() {
    let summary = crate::cursor_session::summarize_cursor_sessions(&[
        session_with_prompts("s1", 2),
        session_with_prompts("s2", 4),
    ]);
    assert_eq!(summary.single_prompt_ratio, Some(0.0));
}

#[test]
fn summarize_mixed_prompt_sessions_ratio_is_session_share() {
    let summary = crate::cursor_session::summarize_cursor_sessions(&[
        session_with_prompts("single", 1),
        session_with_prompts("multi", 3),
    ]);
    assert_eq!(summary.single_prompt_ratio, Some(0.5));
}

#[test]
fn summarize_empty_sessions_single_prompt_ratio_is_none() {
    assert_eq!(
        crate::cursor_session::summarize_cursor_sessions(&[]).single_prompt_ratio,
        None
    );
    assert_eq!(
        crate::domain::CursorSessionSummaryDto::empty().single_prompt_ratio,
        None
    );
}
