use crate::test_support::*;

fn with_cache(
    occurred_at: &str,
    source: Source,
    session_id: &str,
    project: &str,
    input: i64,
    cache_read: i64,
    cache_creation: i64,
) -> UsageRecord {
    let mut record = rec(
        occurred_at,
        source,
        "model",
        "official",
        project,
        session_id,
        input + cache_read + cache_creation,
    );
    record.input_tokens = input;
    record.cache_read_tokens = cache_read;
    record.cache_creation_tokens = cache_creation;
    record
}

#[test]
fn cache_hit_rate_is_none_without_cache_tokens() {
    assert_eq!(crate::domain::cache_hit_rate(0, 0, 100), None);
    assert_eq!(crate::domain::cache_hit_rate(0, 0, 0), None);
    assert_eq!(crate::domain::cache_hit_rate(0, 10, 100), Some(0.0));
    assert!((crate::domain::cache_hit_rate(20, 0, 80).unwrap() - 0.2).abs() < 1e-9);
}

#[test]
fn lists_lowest_hit_sessions_for_one_source_only() {
    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[
            with_cache(
                "2026-08-01T10:00:00Z",
                Source::Claude,
                "low",
                "/proj/a",
                90,
                10,
                0,
            ),
            with_cache(
                "2026-08-01T11:00:00Z",
                Source::Claude,
                "high",
                "/proj/a",
                20,
                80,
                0,
            ),
            with_cache(
                "2026-08-01T12:00:00Z",
                Source::Claude,
                "mid",
                "/proj/b",
                50,
                50,
                0,
            ),
            with_cache(
                "2026-08-01T13:00:00Z",
                Source::Codex,
                "codex-low",
                "/proj/a",
                95,
                5,
                0,
            ),
            rec(
                "2026-08-01T14:00:00Z",
                Source::Claude,
                "model",
                "official",
                "/proj/a",
                "no-cache",
                40,
            ),
        ],
    )
    .unwrap();

    let dto = query::low_cache_hit_sessions(&conn, &Filter::default(), "claude", 10).unwrap();
    assert!(dto.computable);
    assert_eq!(dto.source, "claude");
    let ids: Vec<&str> = dto.rows.iter().map(|row| row.session_id.as_str()).collect();
    assert_eq!(ids, vec!["low", "mid", "high"]);
    assert!((dto.rows[0].cache_hit_rate.unwrap() - 10.0 / 100.0).abs() < 1e-9);
    assert!(!ids.contains(&"codex-low"));
    assert!(!ids.contains(&"no-cache"));
}

#[test]
fn respects_limit_and_filter() {
    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[
            with_cache(
                "2026-08-01T10:00:00Z",
                Source::Grok,
                "a",
                "/proj/a",
                90,
                10,
                0,
            ),
            with_cache(
                "2026-08-01T11:00:00Z",
                Source::Grok,
                "b",
                "/proj/b",
                80,
                20,
                0,
            ),
            with_cache(
                "2026-08-02T10:00:00Z",
                Source::Grok,
                "c",
                "/proj/a",
                70,
                30,
                0,
            ),
        ],
    )
    .unwrap();

    let limited = query::low_cache_hit_sessions(&conn, &Filter::default(), "grok", 1).unwrap();
    assert_eq!(limited.rows.len(), 1);
    assert_eq!(limited.rows[0].session_id, "a");

    let filtered = query::low_cache_hit_sessions(
        &conn,
        &Filter {
            projects: vec!["/proj/a".into()],
            from: Some("2026-08-02T00:00:00Z".into()),
            ..Filter::default()
        },
        "grok",
        10,
    )
    .unwrap();
    assert_eq!(filtered.rows.len(), 1);
    assert_eq!(filtered.rows[0].session_id, "c");
}

#[test]
fn source_without_cache_tokens_is_not_computable() {
    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-01T10:00:00Z",
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "plain",
            100,
        )],
    )
    .unwrap();

    let dto = query::low_cache_hit_sessions(&conn, &Filter::default(), "codex", 10).unwrap();
    assert!(!dto.computable);
    assert!(dto.rows.is_empty());
}

#[test]
fn cursor_account_source_is_not_computable() {
    let conn = store::open_memory().unwrap();
    let dto = query::low_cache_hit_sessions(&conn, &Filter::default(), "cursor", 10).unwrap();
    assert!(!dto.computable);
    assert!(dto.rows.is_empty());
}

#[test]
fn application_analytics_sql_also_hides_missing_cache_as_zero_percent() {
    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-01T10:00:00Z",
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "plain",
            100,
        )],
    )
    .unwrap();
    let dto = query::application_analytics(&conn, &Filter::default(), "day").unwrap();
    assert_eq!(dto.summary.cache_hit_rate, None);
    assert_eq!(dto.by_application[0].metrics.cache_hit_rate, None);
}

#[test]
fn cache_creation_only_session_counts_as_zero_percent() {
    let conn = store::open_memory().unwrap();
    store::insert_records(
        &conn,
        &[with_cache(
            "2026-08-01T10:00:00Z",
            Source::Claude,
            "wrote",
            "/proj/a",
            100,
            0,
            40,
        )],
    )
    .unwrap();

    let dto = query::low_cache_hit_sessions(&conn, &Filter::default(), "claude", 10).unwrap();
    assert!(dto.computable);
    assert_eq!(dto.rows[0].session_id, "wrote");
    assert_eq!(dto.rows[0].cache_hit_rate, Some(0.0));
}
