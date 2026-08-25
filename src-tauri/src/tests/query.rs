use crate::test_support::*;

#[test]
fn filters_restrict_overview_to_matching_subset() {
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &seed_records()).unwrap();
    let records = store::load_all(&conn).unwrap();
    let prices = PriceTable::default();

    let all = aggregate::overview(&records, &Filter::default(), &prices);
    assert_eq!(all.total_tokens, 450);
    assert_eq!(all.session_count, 3);

    let from_aug2 = Filter {
        from: Some("2026-08-02T00:00:00Z".into()),
        ..Filter::default()
    };
    let dto = aggregate::overview(&records, &from_aug2, &prices);
    assert_eq!(dto.total_tokens, 350);
    assert_eq!(dto.session_count, 2);

    let until = Filter {
        to: Some("2026-08-02T00:00:00Z".into()),
        ..Filter::default()
    };
    assert_eq!(
        aggregate::overview(&records, &until, &prices).total_tokens,
        100
    );

    let by_source = Filter {
        sources: vec!["codex".into()],
        ..Filter::default()
    };
    assert_eq!(
        aggregate::overview(&records, &by_source, &prices).total_tokens,
        100
    );

    let by_model = Filter {
        models: vec!["gpt-5.5".into()],
        ..Filter::default()
    };
    assert_eq!(
        aggregate::overview(&records, &by_model, &prices).total_tokens,
        50
    );

    let by_project = Filter {
        projects: vec!["/proj/a".into()],
        ..Filter::default()
    };
    assert_eq!(
        aggregate::overview(&records, &by_project, &prices).total_tokens,
        400
    );

    let intersect = Filter {
        from: Some("2026-08-02T00:00:00Z".into()),
        projects: vec!["/proj/a".into()],
        ..Filter::default()
    };
    assert_eq!(
        aggregate::overview(&records, &intersect, &prices).total_tokens,
        300
    );
}

#[test]
fn filters_apply_across_trend_breakdown_and_sessions() {
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &seed_records()).unwrap();
    let records = store::load_all(&conn).unwrap();
    let prices = PriceTable::default();
    let by_project = Filter {
        projects: vec!["/proj/a".into()],
        ..Filter::default()
    };

    let days = aggregate::trend(&records, &by_project, &prices, "day");
    assert_eq!(days.len(), 2);
    assert_eq!(days[0].bucket, "2026-08-01");
    assert_eq!(days[0].total_tokens, 100);
    assert_eq!(days[1].bucket, "2026-08-02");
    assert_eq!(days[1].total_tokens, 300);

    let by_source = aggregate::by_name(&records, &by_project, &prices, |r| {
        r.source.as_str().to_string()
    });
    assert_eq!(by_source.len(), 2);
    assert_eq!(by_source[0].name, "claude");
    assert_eq!(by_source[0].total_tokens, 300);
    assert_eq!(by_source[1].name, "codex");
    assert_eq!(by_source[1].total_tokens, 100);

    let top = aggregate::top_sessions(&records, &by_project, &prices, 10);
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].session_id, "s2");
    assert_eq!(top[1].session_id, "s1");
}

#[test]
fn filter_options_list_distinct_sources_models_projects() {
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &seed_records()).unwrap();
    let records = store::load_all(&conn).unwrap();
    let options = aggregate::filter_options(&records);
    assert_eq!(options.sources, vec!["claude", "codex", "pi"]);
    assert_eq!(
        options.models,
        vec!["claude-sonnet-5", "gpt-5.1-codex", "gpt-5.5"]
    );
    assert_eq!(options.projects, vec!["/proj/a", "/proj/b"]);
    assert_eq!(options.providers, vec!["anthropic", "official", "subapi"]);
}

#[test]
fn trend_buckets_by_day_and_week() {
    let mut records = seed_records();
    records[0].cache_read_tokens = 10;
    records[0].cache_creation_tokens = 4;
    records[0].reasoning_tokens = 6;
    records.push(rec(
        "2026-08-01T11:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        20,
    ));
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let prices = PriceTable::default();

    let days = aggregate::trend(&stored, &Filter::default(), &prices, "day");
    assert_eq!(days.len(), 3);
    assert_eq!(days[0].bucket, "2026-08-01");
    assert_eq!(days[0].total_tokens, 120);
    assert_eq!(days[0].input_tokens, 120);
    assert_eq!(days[0].output_tokens, 0);
    assert_eq!(days[0].cache_read_tokens, 10);
    assert_eq!(days[0].cache_creation_tokens, 4);
    assert_eq!(days[0].reasoning_tokens, 6);
    assert_eq!(days[1].bucket, "2026-08-02");
    assert_eq!(days[1].total_tokens, 300);
    assert_eq!(days[2].bucket, "2026-08-08");
    assert_eq!(days[2].total_tokens, 50);

    let months = aggregate::trend(&stored, &Filter::default(), &prices, "month");
    assert_eq!(months.len(), 1);
    assert_eq!(months[0].bucket, "2026-08");
    assert_eq!(months[0].total_tokens, 470);

    let hours = aggregate::trend(&stored, &Filter::default(), &prices, "hour");
    assert!(hours.iter().any(|point| point.bucket == "2026-08-01T11"));
    assert_eq!(
        hours
            .iter()
            .filter(|point| point.bucket == "2026-08-01T11")
            .map(|point| point.total_tokens)
            .sum::<i64>(),
        20
    );

    let weeks = aggregate::trend(&stored, &Filter::default(), &prices, "week");
    assert_eq!(weeks.len(), 2);
    assert_eq!(weeks[0].bucket, "2026-W31");
    assert_eq!(weeks[0].total_tokens, 420);
    assert_eq!(weeks[1].bucket, "2026-W32");
    assert_eq!(weeks[1].total_tokens, 50);

    let from_aug2 = Filter {
        from: Some("2026-08-02T00:00:00Z".into()),
        ..Filter::default()
    };
    let filtered_days = aggregate::trend(&stored, &from_aug2, &prices, "day");
    assert_eq!(filtered_days.len(), 2);
    assert_eq!(filtered_days[0].bucket, "2026-08-02");
    assert_eq!(filtered_days[0].total_tokens, 300);
    let filtered_weeks = aggregate::trend(&stored, &from_aug2, &prices, "week");
    assert_eq!(filtered_weeks.len(), 2);
    assert_eq!(filtered_weeks[0].bucket, "2026-W31");
    assert_eq!(filtered_weeks[0].total_tokens, 300);
    assert_eq!(filtered_weeks[1].bucket, "2026-W32");
    assert_eq!(filtered_weeks[1].total_tokens, 50);
}

#[test]
fn breakdowns_rank_source_model_provider_and_project() {
    let records = seed_records();
    let by_source = aggregate::by_name(&records, &Filter::default(), &PriceTable::default(), |r| {
        r.source.as_str().to_string()
    });
    assert_eq!(by_source[0].name, "claude");
    assert_eq!(by_source[0].total_tokens, 300);
    assert!((by_source[0].share - 300.0 / 450.0).abs() < 1e-9);

    let by_model = aggregate::by_name(&records, &Filter::default(), &PriceTable::default(), |r| {
        r.model.clone()
    });
    assert_eq!(by_model[0].name, "claude-sonnet-5");

    let by_provider =
        aggregate::by_name(&records, &Filter::default(), &PriceTable::default(), |r| {
            r.provider.clone()
        });
    assert_eq!(by_provider[0].name, "anthropic");

    let by_project =
        aggregate::by_name(&records, &Filter::default(), &PriceTable::default(), |r| {
            r.project.clone()
        });
    assert_eq!(by_project[0].name, "/proj/a");
    assert_eq!(by_project[0].total_tokens, 400);
}

#[test]
fn breakdown_by_source_ranks_share_and_follows_filter() {
    let mut records = seed_records();
    records.push(rec(
        "2026-08-01T11:00:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/proj/a",
        "s2",
        50,
    ));
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let prices = PriceTable::default();

    let rows = aggregate::by_name(&stored, &Filter::default(), &prices, |r| {
        r.source.as_str().to_string()
    });
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].name, "claude");
    assert_eq!(rows[0].total_tokens, 350);
    assert!((rows[0].share - 350.0 / 500.0).abs() < 1e-9);
    assert_eq!(rows[1].name, "codex");
    assert_eq!(rows[1].total_tokens, 100);
    assert!((rows[1].share - 100.0 / 500.0).abs() < 1e-9);
    assert_eq!(rows[2].name, "pi");
    assert_eq!(rows[2].total_tokens, 50);
    assert!((rows[2].share - 50.0 / 500.0).abs() < 1e-9);

    let from_aug2 = Filter {
        from: Some("2026-08-02T00:00:00Z".into()),
        ..Filter::default()
    };
    let filtered = aggregate::by_name(&stored, &from_aug2, &prices, |r| {
        r.source.as_str().to_string()
    });
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].name, "claude");
    assert_eq!(filtered[0].total_tokens, 300);
    assert!((filtered[0].share - 300.0 / 350.0).abs() < 1e-9);
    assert_eq!(filtered[1].name, "pi");
    assert_eq!(filtered[1].total_tokens, 50);
    assert!((filtered[1].share - 50.0 / 350.0).abs() < 1e-9);
}

#[test]
fn breakdown_by_model_ranks_across_sources_and_follows_filter() {
    let mut records = seed_records();
    records.push(rec(
        "2026-08-01T11:00:00Z",
        Source::Codex,
        "gpt-5.5",
        "official",
        "/proj/a",
        "s1",
        80,
    ));
    records.push(rec(
        "2026-08-08T12:00:00Z",
        Source::Factory,
        "",
        "anthropic",
        "/proj/b",
        "s4",
        20,
    ));
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let prices = PriceTable::default();

    let rows = aggregate::by_name(&stored, &Filter::default(), &prices, |r| r.model.clone());
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].name, "claude-sonnet-5");
    assert_eq!(rows[0].total_tokens, 300);
    assert!((rows[0].share - 300.0 / 550.0).abs() < 1e-9);
    assert_eq!(rows[1].name, "gpt-5.5");
    assert_eq!(rows[1].total_tokens, 130);
    assert!((rows[1].share - 130.0 / 550.0).abs() < 1e-9);
    assert_eq!(rows[2].name, "gpt-5.1-codex");
    assert_eq!(rows[2].total_tokens, 100);
    assert_eq!(rows[3].name, "（未标注）");
    assert_eq!(rows[3].total_tokens, 20);

    let from_aug2 = Filter {
        from: Some("2026-08-02T00:00:00Z".into()),
        ..Filter::default()
    };
    let filtered = aggregate::by_name(&stored, &from_aug2, &prices, |r| r.model.clone());
    assert_eq!(filtered.len(), 3);
    assert_eq!(filtered[0].name, "claude-sonnet-5");
    assert_eq!(filtered[0].total_tokens, 300);
    assert!((filtered[0].share - 300.0 / 370.0).abs() < 1e-9);
    assert_eq!(filtered[1].name, "gpt-5.5");
    assert_eq!(filtered[1].total_tokens, 50);
    assert!((filtered[1].share - 50.0 / 370.0).abs() < 1e-9);
    assert_eq!(filtered[2].name, "（未标注）");
    assert_eq!(filtered[2].total_tokens, 20);
}

#[test]
fn breakdown_by_provider_ranks_and_follows_filter() {
    let mut records = seed_records();
    records.push(rec(
        "2026-08-01T11:00:00Z",
        Source::Factory,
        "claude-sonnet-5",
        "anthropic",
        "/proj/a",
        "s4",
        40,
    ));
    records.push(rec(
        "2026-08-08T12:00:00Z",
        Source::Pi,
        "gpt-5.5",
        "siliconflow",
        "/proj/b",
        "s3",
        70,
    ));
    records.push(rec(
        "2026-08-08T13:00:00Z",
        Source::Kimi,
        "",
        "",
        "/proj/b",
        "s5",
        20,
    ));
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let prices = PriceTable::default();

    let rows = aggregate::by_name(&stored, &Filter::default(), &prices, |r| r.provider.clone());
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].name, "anthropic");
    assert_eq!(rows[0].total_tokens, 340);
    assert!((rows[0].share - 340.0 / 580.0).abs() < 1e-9);
    assert_eq!(rows[1].name, "official");
    assert_eq!(rows[1].total_tokens, 100);
    assert_eq!(rows[2].name, "siliconflow");
    assert_eq!(rows[2].total_tokens, 70);
    assert_eq!(rows[3].name, "subapi");
    assert_eq!(rows[3].total_tokens, 50);
    assert_eq!(rows[4].name, "（未标注）");
    assert_eq!(rows[4].total_tokens, 20);

    let from_aug2 = Filter {
        from: Some("2026-08-02T00:00:00Z".into()),
        ..Filter::default()
    };
    let filtered = aggregate::by_name(&stored, &from_aug2, &prices, |r| r.provider.clone());
    assert_eq!(filtered.len(), 4);
    assert_eq!(filtered[0].name, "anthropic");
    assert_eq!(filtered[0].total_tokens, 300);
    assert!((filtered[0].share - 300.0 / 440.0).abs() < 1e-9);
    assert_eq!(filtered[1].name, "siliconflow");
    assert_eq!(filtered[1].total_tokens, 70);
    assert_eq!(filtered[2].name, "subapi");
    assert_eq!(filtered[2].total_tokens, 50);
    assert_eq!(filtered[3].name, "（未标注）");
    assert_eq!(filtered[3].total_tokens, 20);

    let by_official = Filter {
        providers: vec!["official".into()],
        ..Filter::default()
    };
    let official_only = aggregate::by_name(&stored, &by_official, &prices, |r| r.provider.clone());
    assert_eq!(official_only.len(), 1);
    assert_eq!(official_only[0].name, "official");
    assert_eq!(official_only[0].total_tokens, 100);
}

#[test]
fn breakdown_by_project_ranks_and_follows_filter() {
    let mut records = seed_records();
    records.push(rec(
        "2026-08-01T11:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/c",
        "s6",
        80,
    ));
    records.push(rec(
        "2026-08-08T12:00:00Z",
        Source::Factory,
        "",
        "anthropic",
        "",
        "s7",
        20,
    ));
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let prices = PriceTable::default();

    let rows = aggregate::by_name(&stored, &Filter::default(), &prices, |r| r.project.clone());
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].name, "/proj/a");
    assert_eq!(rows[0].total_tokens, 400);
    assert!((rows[0].share - 400.0 / 550.0).abs() < 1e-9);
    assert_eq!(rows[1].name, "/proj/c");
    assert_eq!(rows[1].total_tokens, 80);
    assert_eq!(rows[2].name, "/proj/b");
    assert_eq!(rows[2].total_tokens, 50);
    assert_eq!(rows[3].name, "（未标注）");
    assert_eq!(rows[3].total_tokens, 20);

    let from_aug2 = Filter {
        from: Some("2026-08-02T00:00:00Z".into()),
        ..Filter::default()
    };
    let filtered = aggregate::by_name(&stored, &from_aug2, &prices, |r| r.project.clone());
    assert_eq!(filtered.len(), 3);
    assert_eq!(filtered[0].name, "/proj/a");
    assert_eq!(filtered[0].total_tokens, 300);
    assert!((filtered[0].share - 300.0 / 370.0).abs() < 1e-9);
    assert_eq!(filtered[1].name, "/proj/b");
    assert_eq!(filtered[1].total_tokens, 50);
    assert_eq!(filtered[2].name, "（未标注）");
    assert_eq!(filtered[2].total_tokens, 20);
}

#[test]
fn top_sessions_returns_highest_token_sessions_first() {
    let conn = store::open_memory().unwrap();
    let mut records = Vec::new();
    for index in 0..20 {
        records.push(rec(
            "2026-01-01T00:00:00+00:00",
            Source::Codex,
            "gpt",
            "openai",
            "/demo",
            &format!("s{index:02}"),
            i64::from(index + 1),
        ));
    }
    store::insert_records(&conn, &records).unwrap();
    let top = query::top_sessions(&conn, &Filter::default(), &PriceTable::default(), 3).unwrap();
    assert_eq!(
        top.iter()
            .map(|row| (row.session_id.as_str(), row.total_tokens))
            .collect::<Vec<_>>(),
        vec![("s19", 20), ("s18", 19), ("s17", 18)]
    );
    assert_eq!(top[0].project, "/demo");
    assert_eq!(top[0].model, "gpt");
}

#[test]
fn top_sessions_and_turns_preserve_source_file() {
    let mut records = seed_records();
    records.push(rec(
        "2026-08-01T11:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        20,
    ));
    records.push(rec(
        "2026-08-01T12:00:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/proj/a",
        "s1",
        99,
    ));
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let prices = PriceTable::default();

    let top = aggregate::top_sessions(&stored, &Filter::default(), &prices, 2);
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].session_id, "s2");
    assert_eq!(top[0].source, "claude");
    assert_eq!(top[0].project, "/proj/a");
    assert_eq!(top[0].total_tokens, 300);
    assert_eq!(top[0].started_at, "2026-08-02T10:00:00Z");
    assert_eq!(top[0].ended_at, "2026-08-02T10:00:00Z");
    assert_eq!(top[0].source_file, "/s2.jsonl");
    assert_eq!(top[1].session_id, "s1");
    assert_eq!(top[1].source, "codex");
    assert_eq!(top[1].total_tokens, 120);
    assert_eq!(top[1].started_at, "2026-08-01T10:00:00Z");
    assert_eq!(top[1].ended_at, "2026-08-01T11:00:00Z");
    assert_eq!(top[1].source_file, "/s1.jsonl");

    let all = aggregate::top_sessions(&stored, &Filter::default(), &prices, 10);
    assert_eq!(all.len(), 4);
    assert_eq!(all[2].session_id, "s1");
    assert_eq!(all[2].source, "claude");
    assert_eq!(all[2].total_tokens, 99);

    let from_aug2 = Filter {
        from: Some("2026-08-02T00:00:00Z".into()),
        ..Filter::default()
    };
    let filtered_top = aggregate::top_sessions(&stored, &from_aug2, &prices, 10);
    assert_eq!(filtered_top.len(), 2);
    assert_eq!(filtered_top[0].session_id, "s2");
    assert_eq!(filtered_top[0].total_tokens, 300);
    assert_eq!(filtered_top[1].session_id, "s3");
    assert_eq!(filtered_top[1].total_tokens, 50);

    let turns = aggregate::session_turns(&stored, "s1", Some("codex"), &Filter::default(), &prices);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].occurred_at, "2026-08-01T10:00:00Z");
    assert_eq!(turns[0].model, "gpt-5.1-codex");
    assert_eq!(turns[0].total_tokens, 100);
    assert_eq!(turns[0].source_file, "/s1.jsonl");
    assert_eq!(turns[1].occurred_at, "2026-08-01T11:00:00Z");
    assert_eq!(turns[1].total_tokens, 20);

    let same_id_all_sources =
        aggregate::session_turns(&stored, "s1", None, &Filter::default(), &prices);
    assert_eq!(same_id_all_sources.len(), 3);
    let same_id_other_source =
        aggregate::session_turns(&stored, "s1", Some("codex"), &Filter::default(), &prices);
    assert_eq!(same_id_other_source.len(), 2);

    let recent = Filter {
        from: Some("2026-08-01T10:30:00Z".into()),
        ..Filter::default()
    };
    let filtered = aggregate::session_turns(&stored, "s1", Some("codex"), &recent, &prices);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].total_tokens, 20);
    assert_eq!(filtered[0].source_file, "/s1.jsonl");
}

#[test]
fn sessions_page_supports_search_sort_and_pagination() {
    let mut records = seed_records();
    records.push(rec(
        "2026-08-01T11:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/c",
        "s6",
        80,
    ));
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let prices = PriceTable::default();

    // 默认排序：按 total_tokens 降序，分页返回第一页。
    let page1 = query::sessions_page(
        &conn,
        &prices,
        &SessionQuery {
            page: Some(1),
            page_size: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page1.total, 4);
    assert_eq!(page1.rows.len(), 2);
    assert_eq!(page1.rows[0].session_id, "s2");
    assert_eq!(page1.rows[0].total_tokens, 300);
    assert_eq!(page1.rows[1].session_id, "s1");
    assert_eq!(page1.total_tokens, 300 + 100 + 80 + 50);
    assert!(page1.rows[0].cost.is_none());
    assert!(!page1.rows[0].unpriced);

    let page2 = query::sessions_page(
        &conn,
        &prices,
        &SessionQuery {
            page: Some(2),
            page_size: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page2.rows.len(), 2);
    assert_eq!(page2.rows[0].session_id, "s6");
    assert_eq!(page2.rows[1].session_id, "s3");

    // 超出页码时仍返回汇总，避免 KPI 被清空。
    let empty_page = query::sessions_page(
        &conn,
        &prices,
        &SessionQuery {
            page: Some(99),
            page_size: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(empty_page.total, 4);
    assert_eq!(empty_page.total_tokens, 300 + 100 + 80 + 50);
    assert!(empty_page.rows.is_empty());
    assert!(empty_page.last_ended.is_some());

    // 升序排序按 session_id。
    let asc_by_session = query::sessions_page(
        &conn,
        &prices,
        &SessionQuery {
            sort_by: Some("session".into()),
            sort_dir: Some("asc".into()),
            page: Some(1),
            page_size: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    let ids: Vec<&str> = asc_by_session
        .rows
        .iter()
        .map(|r| r.session_id.as_str())
        .collect();
    assert_eq!(ids, vec!["s1", "s2", "s3", "s6"]);

    // 搜索：只命中项目名包含 "proj/c" 的会话。
    let searched = query::sessions_page(
        &conn,
        &prices,
        &SessionQuery {
            search: Some("proj/c".into()),
            page: Some(1),
            page_size: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(searched.total, 1);
    assert_eq!(searched.rows[0].session_id, "s6");

    // 搜索无匹配时返回空结果而非报错。
    let no_match = query::sessions_page(
        &conn,
        &prices,
        &SessionQuery {
            search: Some("不存在的关键字".into()),
            page: Some(1),
            page_size: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(no_match.total, 0);
    assert!(no_match.rows.is_empty());
    assert_eq!(no_match.last_ended, None);
}

#[test]
fn sessions_page_computes_cost_only_when_requested() {
    let mut priced = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        0,
    );
    priced.input_tokens = 1000;
    priced.total_tokens = 1000;
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &[priced]).unwrap();
    let table = PriceTable {
        prices: vec![PriceEntry {
            model: "gpt-5.1-codex".into(),
            provider: Some("official".into()),
            input: 0.001,
            output: 0.0,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }],
    };

    let listed = query::sessions_page(&conn, &table, &SessionQuery::default()).unwrap();
    assert_eq!(listed.rows.len(), 1);
    assert_eq!(listed.rows[0].cost, None);
    assert!(!listed.rows[0].unpriced);

    let exported = query::sessions_page(
        &conn,
        &table,
        &SessionQuery {
            include_cost: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(exported.rows[0].cost, Some(1.0));
    assert!(!exported.rows[0].unpriced);
}

#[test]
fn cost_prefers_native_and_marks_unpriced() {
    let priced = UsageRecord {
        native_cost: None,
        ..rec(
            "2026-08-01T10:00:00Z",
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            0,
        )
    };
    let mut priced = priced;
    priced.input_tokens = 1000;
    priced.output_tokens = 500;
    priced.cache_read_tokens = 200;
    priced.cache_creation_tokens = 100;
    let table = PriceTable {
        prices: vec![PriceEntry {
            model: "gpt-5.1-codex".into(),
            provider: Some("official".into()),
            input: 0.001,
            output: 0.002,
            cache_read: 0.0005,
            cache_creation: 0.003,
            origin: PriceOrigin::User,
        }],
    };
    let derived = derive_cost(&priced, &table);
    assert_eq!(derived.amount, Some(1.0 + 1.0 + 0.1 + 0.3));
    assert!(!derived.unpriced);

    let native = UsageRecord {
        native_cost: Some(9.9),
        ..priced.clone()
    };
    let derived = derive_cost(&native, &table);
    assert_eq!(derived.amount, Some(9.9));
    assert!(derived.source_native);

    let missing = rec(
        "2026-08-01T10:00:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/proj/a",
        "s2",
        10,
    );
    let derived = derive_cost(&missing, &table);
    assert_eq!(derived.amount, None);
    assert!(derived.unpriced);

    priced.reasoning_tokens = 999;
    let derived = derive_cost(&priced, &table);
    assert_eq!(derived.amount, Some(2.4));

    let mut by_provider = rec(
        "2026-08-01T10:00:00Z",
        Source::Pi,
        "gpt-5.5",
        "subapi",
        "/proj/b",
        "s3",
        0,
    );
    by_provider.input_tokens = 100;
    let mixed = PriceTable {
        prices: vec![
            PriceEntry {
                model: "gpt-5.5".into(),
                provider: Some("subapi".into()),
                input: 0.02,
                output: 0.0,
                cache_read: 0.0,
                cache_creation: 0.0,
                origin: PriceOrigin::User,
            },
            PriceEntry {
                model: "gpt-5.5".into(),
                provider: None,
                input: 0.01,
                output: 0.0,
                cache_read: 0.0,
                cache_creation: 0.0,
                origin: PriceOrigin::User,
            },
        ],
    };
    assert_eq!(derive_cost(&by_provider, &mixed).amount, Some(2.0));
    by_provider.provider = "siliconflow".into();
    assert_eq!(derive_cost(&by_provider, &mixed).amount, Some(1.0));
    by_provider.model = "unknown-model".into();
    let unknown = derive_cost(&by_provider, &mixed);
    assert_eq!(unknown.amount, None);
    assert!(unknown.unpriced);
}

#[test]
fn cost_matches_model_and_provider_case_insensitively() {
    // 来源上报或用户价目表里的大小写不一致（如 "GPT-4o" vs "gpt-4o"）时仍应命中同一模型单价。
    let mut record = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "GPT-4o",
        "OpenAI",
        "/proj/a",
        "s1",
        0,
    );
    record.input_tokens = 100;
    let table = PriceTable {
        prices: vec![PriceEntry {
            model: "gpt-4o".into(),
            provider: Some("openai".into()),
            input: 0.01,
            output: 0.0,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }],
    };
    let derived = derive_cost(&record, &table);
    assert_eq!(derived.amount, Some(1.0));
    assert!(!derived.unpriced);

    // provider 兜底档（价目表条目 provider 为 None）同样大小写不敏感。
    let table_bare = PriceTable {
        prices: vec![PriceEntry {
            model: "gpt-4o".into(),
            provider: None,
            input: 0.02,
            output: 0.0,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }],
    };
    let derived_bare = derive_cost(&record, &table_bare);
    assert_eq!(derived_bare.amount, Some(2.0));
}

#[test]
fn sql_overview_matches_memory_when_price_table_case_differs() {
    let mut record = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "GPT-4o",
        "OpenAI",
        "/proj/a",
        "s1",
        0,
    );
    record.input_tokens = 100;
    record.total_tokens = 100;
    let prices = PriceTable {
        prices: vec![PriceEntry {
            model: "gpt-4o".into(),
            provider: Some("openai".into()),
            input: 0.01,
            output: 0.0,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }],
    };
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &[record.clone()]).unwrap();
    let sql = query::overview(&conn, &Filter::default(), &prices).unwrap();
    let mem = aggregate::overview(&[record.clone()], &Filter::default(), &prices);
    assert_eq!(sql.cost, mem.cost);
    assert_eq!(sql.cost, Some(1.0));
    assert!(!sql.unpriced);
    assert!(!mem.unpriced);

    let turns = query::session_turns(&conn, "s1", None, &Filter::default(), &prices).unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].cost, Some(1.0));
    assert!(!turns[0].unpriced);
    assert_eq!(turns[0].cost_source, CostSource::User);
}

#[test]
fn overview_and_turns_use_price_table_and_flag_unpriced() {
    let mut priced = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        0,
    );
    priced.input_tokens = 1000;
    priced.total_tokens = 1000;
    let unpriced = rec(
        "2026-08-02T10:00:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/proj/a",
        "s2",
        10,
    );
    let mut native = rec(
        "2026-08-08T10:00:00Z",
        Source::Pi,
        "gpt-5.5",
        "subapi",
        "/proj/b",
        "s3",
        50,
    );
    native.native_cost = Some(0.5);
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &[priced, unpriced, native]).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let table = PriceTable {
        prices: vec![PriceEntry {
            model: "gpt-5.1-codex".into(),
            provider: Some("official".into()),
            input: 0.001,
            output: 0.0,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }],
    };

    let dto = aggregate::overview(&stored, &Filter::default(), &table);
    assert_eq!(dto.cost, Some(1.5));
    assert!(dto.unpriced);

    let priced_turns =
        aggregate::session_turns(&stored, "s1", Some("codex"), &Filter::default(), &table);
    assert_eq!(priced_turns[0].cost, Some(1.0));
    assert_eq!(priced_turns[0].cost_source, CostSource::User);
    assert_eq!(priced_turns[0].cost_note.as_deref(), Some("用户单价"));
    let unpriced_turns =
        aggregate::session_turns(&stored, "s2", Some("claude"), &Filter::default(), &table);
    assert_eq!(unpriced_turns[0].cost, None);
    assert!(unpriced_turns[0].unpriced);
    assert_eq!(unpriced_turns[0].cost_source, CostSource::None);
    assert_eq!(unpriced_turns[0].cost_note.as_deref(), Some("单价未配置"));
    let native_turns =
        aggregate::session_turns(&stored, "s3", Some("pi"), &Filter::default(), &table);
    assert_eq!(native_turns[0].cost, Some(0.5));
    assert_eq!(native_turns[0].cost_source, CostSource::Native);
    assert_eq!(native_turns[0].cost_note.as_deref(), Some("来源自带"));

    let by_source = aggregate::by_name(&stored, &Filter::default(), &table, |r| {
        r.source.as_str().to_string()
    });
    assert_eq!(by_source[0].name, "codex");
    assert_eq!(by_source[0].cost, Some(1.0));
    assert!(!by_source[0].unpriced);
    assert_eq!(by_source[1].name, "pi");
    assert_eq!(by_source[1].cost, Some(0.5));
    assert_eq!(by_source[2].name, "claude");
    assert_eq!(by_source[2].cost, None);
    assert!(by_source[2].unpriced);

    let (lifetime_cost, lifetime_unpriced) = query::lifetime_cost(&conn, &table).unwrap();
    let overview = query::overview(&conn, &Filter::default(), &table).unwrap();
    assert_eq!(lifetime_cost, overview.cost);
    assert_eq!(lifetime_unpriced, overview.unpriced);
}

#[test]
fn recent_projects_order_by_latest_activity() {
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &seed_records()).unwrap();
    assert_eq!(
        query::recent_projects(&conn).unwrap(),
        vec!["/proj/b".to_string(), "/proj/a".to_string()]
    );
}

#[test]
fn source_token_totals_order_by_usage() {
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &seed_records()).unwrap();
    let usage = query::source_token_totals(&conn).unwrap();
    assert_eq!(
        usage
            .sources
            .iter()
            .map(|row| (row.source.as_str(), row.total_tokens))
            .collect::<Vec<_>>(),
        vec![("claude", 300), ("codex", 100), ("pi", 50)]
    );
}

/// 同一个连接上换价表后，下一次查询必须用新价，不能吃到上一次装好的那份。
#[test]
fn price_table_changes_take_effect_on_the_same_connection() {
    let mut record = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        0,
    );
    record.input_tokens = 1000;
    record.total_tokens = 1000;
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &[record]).unwrap();

    let entry = |input: f64| PriceTable {
        prices: vec![PriceEntry {
            model: "gpt-5.1-codex".into(),
            provider: Some("official".into()),
            input,
            output: 0.0,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }],
    };
    let filter = Filter::default();

    let first = query::overview(&conn, &filter, &entry(0.001)).unwrap();
    assert_eq!(first.cost, Some(1.0));

    // 单价翻十倍：必须立刻反映新价。
    let raised = query::overview(&conn, &filter, &entry(0.01)).unwrap();
    assert_eq!(raised.cost, Some(10.0));

    // 同一张表再查一次，结果不能变。
    let repeated = query::overview(&conn, &filter, &entry(0.01)).unwrap();
    assert_eq!(repeated.cost, Some(10.0));

    // 清空价表：回到未定价。
    let cleared = query::overview(&conn, &filter, &PriceTable::default()).unwrap();
    assert_eq!(cleared.cost, None);
    assert!(cleared.unpriced);

    // 从空表换回有价表，反向也要生效。
    let restored = query::overview(&conn, &filter, &entry(0.001)).unwrap();
    assert_eq!(restored.cost, Some(1.0));
    assert!(!restored.unpriced);
}

#[test]
fn usage_calls_page_filters_and_paginates_newest_first() {
    let mut records = seed_records();
    records.push(rec(
        "2026-08-02T12:00:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/proj/a",
        "s2b",
        40,
    ));
    records.push(rec(
        "2026-08-03T09:00:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "",
        "/proj/a",
        "s-unlabeled",
        10,
    ));
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let prices = PriceTable::default();

    let by_provider = Filter {
        providers: vec!["anthropic".into()],
        ..Filter::default()
    };
    let first = query::usage_calls_page(&conn, &by_provider, &prices, 1, 1).unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.rows.len(), 1);
    assert_eq!(first.rows[0].session_id, "s2b");
    assert_eq!(first.rows[0].provider, "anthropic");
    assert_eq!(first.rows[0].total_tokens, 40);

    let second = query::usage_calls_page(&conn, &by_provider, &prices, 2, 1).unwrap();
    assert_eq!(second.total, 2);
    assert_eq!(second.rows[0].session_id, "s2");

    let past_end = query::usage_calls_page(&conn, &by_provider, &prices, 4, 1).unwrap();
    assert_eq!(past_end.total, 2);
    assert!(past_end.rows.is_empty());

    let unlabeled = query::usage_calls_page(
        &conn,
        &Filter {
            providers: vec!["".into()],
            ..Filter::default()
        },
        &prices,
        1,
        20,
    )
    .unwrap();
    assert_eq!(unlabeled.total, 1);
    assert_eq!(unlabeled.rows[0].session_id, "s-unlabeled");
    assert_eq!(unlabeled.rows[0].provider, "");
}
