use crate::test_support::*;

#[test]
fn sql_queries_match_in_memory_aggregates() {
    let conn = store::open_memory().unwrap();
    let records = diverse_records();
    store::insert_records(&conn, &records).unwrap();
    store::rebuild_rollup(&conn).unwrap();
    let prices = diverse_prices();

    // overview
    let sql_ov = query::overview(&conn, &Filter::default(), &prices).unwrap();
    let mem_ov = aggregate::overview(&records, &Filter::default(), &prices);
    assert_eq!(sql_ov.total_tokens, mem_ov.total_tokens);
    assert_eq!(sql_ov.input_tokens, mem_ov.input_tokens);
    assert_eq!(sql_ov.output_tokens, mem_ov.output_tokens);
    assert_eq!(sql_ov.cache_read_tokens, mem_ov.cache_read_tokens);
    assert_eq!(sql_ov.cache_creation_tokens, mem_ov.cache_creation_tokens);
    assert_eq!(sql_ov.reasoning_tokens, mem_ov.reasoning_tokens);
    assert_eq!(sql_ov.session_count, mem_ov.session_count);
    assert_eq!(sql_ov.unpriced, mem_ov.unpriced);
    assert_opt_f64_eq(sql_ov.cost, mem_ov.cost);

    // 未定价诊断：全库、不接筛选，逐字段对照
    let sql_ud = query::unpriced_diagnosis(&conn, &prices).unwrap();
    let mem_ud = aggregate::unpriced_diagnosis(&records, &prices);
    assert_eq!(sql_ud, mem_ud);
    assert_eq!(sql_ud.len(), mem_ud.len());
    for (sql_row, mem_row) in sql_ud.iter().zip(mem_ud.iter()) {
        assert_eq!(sql_row.model, mem_row.model);
        assert_eq!(sql_row.provider, mem_row.provider);
        assert_eq!(sql_row.sources, mem_row.sources);
        assert_eq!(sql_row.total_tokens, mem_row.total_tokens);
        assert_eq!(sql_row.record_count, mem_row.record_count);
        assert_eq!(sql_row.reason, mem_row.reason);
        assert_eq!(sql_row.candidate, mem_row.candidate);
    }

    // trend 四种粒度
    for grain in ["hour", "day", "week", "month"] {
        let sql_tr = query::trend(&conn, &Filter::default(), &prices, grain).unwrap();
        let mem_tr = aggregate::trend(&records, &Filter::default(), &prices, grain);
        assert_eq!(sql_tr, mem_tr, "trend grain={grain} 不一致");
    }

    // breakdown 五个维度
    for dim in ["application", "source", "model", "provider", "project"] {
        let sql_bd = query::breakdown(&conn, &Filter::default(), &prices, dim).unwrap();
        let mem_bd = aggregate::by_name(&records, &Filter::default(), &prices, |r| match dim {
            "application" => r.source.application_name().to_string(),
            "source" => r.source.as_str().to_string(),
            "model" => r.model.clone(),
            "provider" => r.provider.clone(),
            "project" => r.project.clone(),
            _ => unreachable!(),
        });
        assert_eq!(sql_bd.len(), mem_bd.len(), "breakdown dim={dim} 行数不一致");
        for (s, m) in sql_bd.iter().zip(mem_bd.iter()) {
            assert_eq!(s.name, m.name);
            assert_eq!(s.total_tokens, m.total_tokens);
            assert!((s.share - m.share).abs() < 1e-9);
            assert_eq!(s.unpriced, m.unpriced);
            assert_opt_f64_eq(s.cost, m.cost);
        }
    }

    // application_analytics（DTO 整体相等）
    let sql_aa = query::application_analytics(&conn, &Filter::default(), "day").unwrap();
    let mem_aa = aggregate::application_analytics(&records, &Filter::default(), "day");
    assert_eq!(sql_aa, mem_aa);

    // top_sessions
    let sql_top = query::top_sessions(&conn, &Filter::default(), &prices, 10).unwrap();
    let mem_top = aggregate::top_sessions(&records, &Filter::default(), &prices, 10);
    assert_eq!(sql_top, mem_top);

    // session_turns（含 source 过滤与无 source）
    for source in [Some("codex"), None] {
        let sql_turns =
            query::session_turns(&conn, "s1", source, &Filter::default(), &prices).unwrap();
        let mem_turns =
            aggregate::session_turns(&records, "s1", source, &Filter::default(), &prices);
        assert_eq!(
            sql_turns, mem_turns,
            "session_turns source={source:?} 不一致"
        );
    }

    // filter_options
    let sql_fo = query::filter_options(&conn).unwrap();
    let mem_fo = aggregate::filter_options(&records);
    assert_eq!(sql_fo.sources, mem_fo.sources);
    assert_eq!(sql_fo.models, mem_fo.models);
    assert_eq!(sql_fo.projects, mem_fo.projects);
    assert_eq!(sql_fo.providers, mem_fo.providers);

    // billing_windows（忽略日期筛选，按来源切 5h 窗）
    let window_now = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
    let sql_bw = query::billing_windows(&conn, &Filter::default(), &prices, window_now).unwrap();
    let mem_bw = aggregate::billing_windows(&records, &Filter::default(), &prices, window_now);
    assert_eq!(sql_bw, mem_bw);
    let dated = Filter {
        from: Some("2026-08-09T00:00:00Z".into()),
        to: Some("2026-08-09T23:59:59Z".into()),
        ..Filter::default()
    };
    let sql_bw_dated = query::billing_windows(&conn, &dated, &prices, window_now).unwrap();
    let mem_bw_dated = aggregate::billing_windows(&records, &dated, &prices, window_now);
    assert_eq!(sql_bw_dated, mem_bw_dated);
    assert_eq!(sql_bw_dated, sql_bw);

    // 过滤条件的 overview 对照（覆盖 WHERE 子句）
    let filters = [
        Filter {
            from: Some("2026-08-02T00:00:00Z".into()),
            ..Filter::default()
        },
        Filter {
            to: Some("2026-08-02T00:00:00Z".into()),
            ..Filter::default()
        },
        Filter {
            projects: vec!["/proj/b".into()],
            ..Filter::default()
        },
        Filter {
            models: vec!["gpt-5.5".into()],
            ..Filter::default()
        },
        Filter {
            sources: vec!["codex".into()],
            ..Filter::default()
        },
        Filter {
            providers: vec!["official".into()],
            ..Filter::default()
        },
    ];
    for f in &filters {
        let sql_ov = query::overview(&conn, f, &prices).unwrap();
        let mem_ov = aggregate::overview(&records, f, &prices);
        assert_eq!(sql_ov.total_tokens, mem_ov.total_tokens, "filter={f:?}");
        assert_eq!(sql_ov.session_count, mem_ov.session_count);
        assert_eq!(sql_ov.unpriced, mem_ov.unpriced);
        assert_opt_f64_eq(sql_ov.cost, mem_ov.cost);
    }

    // work_timeline：SQL 宽口径拉取 + Cursor 会话区间，与内存路径对同一批 records/spans 调 build 必须一致。
    for day in [
        "2026-08-01",
        "2026-08-02",
        "2026-08-08",
        "2026-08-09",
        "2026-08-15",
    ] {
        let sql_wt = query::work_timeline(&conn, day).unwrap();
        let extra = match crate::work_timeline::broad_date_bounds(day) {
            Some((from, to)) => query::work_session_spans(&conn, &from, &to).unwrap(),
            None => Vec::new(),
        };
        let mem_wt = aggregate::work_timeline_with_spans(&records, &extra, day);
        assert_eq!(sql_wt, mem_wt, "work_timeline day={day}");
        // 逐字段显式对照强度指标，避免 DTO 整体相等遮盖新字段回归。
        assert_eq!(sql_wt.turn_count, mem_wt.turn_count, "turn_count day={day}");
        assert_eq!(
            sql_wt.ai_exec_minutes, mem_wt.ai_exec_minutes,
            "ai_exec_minutes day={day}"
        );
        assert_eq!(
            sql_wt.peak_parallel, mem_wt.peak_parallel,
            "peak_parallel day={day}"
        );
        assert_eq!(
            sql_wt.parallel_intensity, mem_wt.parallel_intensity,
            "parallel_intensity day={day}"
        );
    }
}

/// 同会话同日混有原生费用与无原生费用：预聚合按 has_native 拆成两行，外层求和仍须对上。
fn mixed_native_same_day() -> UsageRecord {
    let mut extra = rec(
        "2026-08-01T12:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        80,
    );
    extra.input_tokens = 80;
    extra.native_cost = Some(0.4);
    extra
}

fn overview_window_records() -> Vec<UsageRecord> {
    let mut records = diverse_records();
    records.push(mixed_native_same_day());
    records
}

fn assert_overview_parity(
    conn: &rusqlite::Connection,
    records: &[UsageRecord],
    prices: &PriceTable,
    filter: &Filter,
    label: &str,
) -> crate::domain::OverviewDto {
    let sql = query::overview(conn, filter, prices).unwrap();
    let mem = aggregate::overview(records, filter, prices);
    assert_eq!(sql.total_tokens, mem.total_tokens, "{label} total_tokens");
    assert_eq!(sql.input_tokens, mem.input_tokens, "{label} input_tokens");
    assert_eq!(
        sql.output_tokens, mem.output_tokens,
        "{label} output_tokens"
    );
    assert_eq!(
        sql.cache_read_tokens, mem.cache_read_tokens,
        "{label} cache_read_tokens"
    );
    assert_eq!(
        sql.cache_creation_tokens, mem.cache_creation_tokens,
        "{label} cache_creation_tokens"
    );
    assert_eq!(
        sql.reasoning_tokens, mem.reasoning_tokens,
        "{label} reasoning_tokens"
    );
    assert_eq!(
        sql.session_count, mem.session_count,
        "{label} session_count"
    );
    assert_eq!(sql.unpriced, mem.unpriced, "{label} unpriced");
    match (sql.cost, mem.cost) {
        (Some(x), Some(y)) => {
            assert!((x - y).abs() < 1e-9, "{label} cost {x} vs {y}")
        }
        (None, None) => {}
        (x, y) => panic!("{label} cost Option 不一致：{x:?} vs {y:?}"),
    }
    sql
}

/// 四种时间窗 + 未就绪：概览 SQL 与内存聚合逐字段一致。
#[test]
fn overview_matches_memory_across_rollup_window_shapes() {
    let conn = store::open_memory().unwrap();
    let records = overview_window_records();
    store::insert_records(&conn, &records).unwrap();
    store::backfill_rollup(&conn).unwrap();
    let prices = diverse_prices();

    let none = Filter::default();
    let aligned = Filter {
        from: Some("2026-08-01T00:00:00Z".into()),
        to: Some("2026-08-09T00:00:00Z".into()),
        ..Filter::default()
    };
    let split = Filter {
        from: Some("2026-08-01T09:00:00Z".into()),
        to: Some("2026-08-08T12:00:00Z".into()),
        ..Filter::default()
    };
    let intra_day = Filter {
        from: Some("2026-08-01T09:00:00Z".into()),
        to: Some("2026-08-01T11:30:00Z".into()),
        ..Filter::default()
    };
    let head_empty = Filter {
        from: Some("2026-08-01T00:00:00Z".into()),
        to: Some("2026-08-08T12:00:00Z".into()),
        ..Filter::default()
    };

    let none_sql = assert_overview_parity(&conn, &records, &prices, &none, "无时间窗");
    assert!(
        none_sql.unpriced,
        "全库含 unknown-model / 空模型，应标未定价"
    );

    let aligned_sql = assert_overview_parity(&conn, &records, &prices, &aligned, "对齐窗");
    // 08-01 priced(100)+native(80) + 08-02 native(50) + claude(200) + 08-08(300)
    assert_eq!(aligned_sql.total_tokens, 100 + 80 + 50 + 200 + 300);
    assert_eq!(aligned_sql.session_count, 3, "s1 跨天仍只计一次");
    assert!(!aligned_sql.unpriced);

    let split_sql = assert_overview_parity(&conn, &records, &prices, &split, "两端 partial 切分窗");
    // 头部 partial 含 s1 同日 priced(10:00)+native(12:00)；08-02 在完整天，须合并成一个会话。
    assert_eq!(split_sql.total_tokens, 100 + 80 + 200 + 50 + 300);
    assert_eq!(
        split_sql.session_count, 3,
        "跨 partial 边界的 s1 只计一次，加上 s2 / s3"
    );

    let intra_sql = assert_overview_parity(&conn, &records, &prices, &intra_day, "单日内窗");
    assert_eq!(intra_sql.total_tokens, 100 + 200);
    assert_eq!(intra_sql.session_count, 2);

    let head_empty_sql =
        assert_overview_parity(&conn, &records, &prices, &head_empty, "单端 partial 为空");
    assert_eq!(head_empty_sql.total_tokens, 100 + 80 + 50 + 200 + 300);
    assert_eq!(head_empty_sql.session_count, 3);

    conn.execute("UPDATE rollup_state SET ready = 0 WHERE id = 1", [])
        .unwrap();
    let not_ready = assert_overview_parity(&conn, &records, &prices, &split, "预聚合未就绪");
    assert_eq!(not_ready.total_tokens, split_sql.total_tokens);
    assert_eq!(not_ready.session_count, split_sql.session_count);
}
