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
    assert_overview_cost_split_eq(&sql_ov, &mem_ov, "overview");

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

    // hour_of_day：按本地一天中的第几个小时跨天合并
    let sql_hod = query::hour_of_day(&conn, &Filter::default()).unwrap();
    let mem_hod = aggregate::hour_of_day(&records, &Filter::default());
    assert_eq!(sql_hod.len(), 24, "hour_of_day 必须覆盖 0–23");
    assert_eq!(mem_hod.len(), 24, "hour_of_day 必须覆盖 0–23");
    for hour in 0..24 {
        assert_eq!(
            sql_hod[hour], mem_hod[hour],
            "hour_of_day hour={hour} 不一致"
        );
    }

    // tokens_by_local_day：按本地日历日汇总，不含 Cursor 账号用量
    let sql_days = query::tokens_by_local_day(&conn, &Filter::default()).unwrap();
    let mem_days = aggregate::tokens_by_local_day(&records, &Filter::default());
    assert_eq!(sql_days, mem_days, "tokens_by_local_day 不一致");

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
        assert_overview_cost_split_eq(&sql_ov, &mem_ov, &format!("filter={f:?}"));
        let sql_hod = query::hour_of_day(&conn, f).unwrap();
        let mem_hod = aggregate::hour_of_day(&records, f);
        assert_eq!(sql_hod, mem_hod, "hour_of_day filter={f:?}");
        let sql_days = query::tokens_by_local_day(&conn, f).unwrap();
        let mem_days = aggregate::tokens_by_local_day(&records, f);
        assert_eq!(sql_days, mem_days, "tokens_by_local_day filter={f:?}");
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
    assert_overview_cost_split_eq(&sql, &mem, label);
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

fn trend_window_records() -> Vec<UsageRecord> {
    let mut records = overview_window_records();
    // 同日更早一小时：切分窗 / 单日窗应从 09:00 裁掉，小时桶不能并进 10:00。
    records.push(rec(
        "2026-08-01T08:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        15,
    ));
    records
}

fn assert_trend_parity(
    conn: &rusqlite::Connection,
    records: &[UsageRecord],
    prices: &PriceTable,
    filter: &Filter,
    grain: &str,
    label: &str,
) -> Vec<crate::domain::SeriesPoint> {
    let sql = query::trend(conn, filter, prices, grain).unwrap();
    let mem = aggregate::trend(records, filter, prices, grain);
    assert_eq!(sql.len(), mem.len(), "{label} 桶数");
    for (s, m) in sql.iter().zip(mem.iter()) {
        assert_eq!(s.bucket, m.bucket, "{label} bucket");
        assert_eq!(
            s.total_tokens, m.total_tokens,
            "{label} {} total_tokens",
            s.bucket
        );
        assert_eq!(
            s.input_tokens, m.input_tokens,
            "{label} {} input_tokens",
            s.bucket
        );
        assert_eq!(
            s.output_tokens, m.output_tokens,
            "{label} {} output_tokens",
            s.bucket
        );
        assert_eq!(
            s.cache_read_tokens, m.cache_read_tokens,
            "{label} {} cache_read_tokens",
            s.bucket
        );
        assert_eq!(
            s.cache_creation_tokens, m.cache_creation_tokens,
            "{label} {} cache_creation_tokens",
            s.bucket
        );
        assert_eq!(
            s.reasoning_tokens, m.reasoning_tokens,
            "{label} {} reasoning_tokens",
            s.bucket
        );
        match (s.cost, m.cost) {
            (Some(x), Some(y)) => {
                assert!((x - y).abs() < 1e-9, "{label} {} cost {x} vs {y}", s.bucket)
            }
            (None, None) => {}
            (x, y) => panic!("{label} {} cost Option 不一致：{x:?} vs {y:?}", s.bucket),
        }
    }
    sql
}

fn point_tokens(points: &[crate::domain::SeriesPoint], bucket: &str) -> i64 {
    points
        .iter()
        .find(|point| point.bucket == bucket)
        .map(|point| point.total_tokens)
        .unwrap_or(0)
}

/// 四种时间窗 × 四种粒度 + 单端 partial / 未就绪：趋势与内存聚合逐字段一致；小时桶不得塌成天。
#[test]
fn trend_matches_memory_across_rollup_window_shapes() {
    let conn = store::open_memory().unwrap();
    let records = trend_window_records();
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
    let windows = [
        (&none, "无时间窗"),
        (&aligned, "对齐窗"),
        (&split, "两端 partial 切分窗"),
        (&intra_day, "单日内窗"),
        (&head_empty, "单端 partial 为空"),
    ];

    for grain in ["hour", "day", "week", "month"] {
        for (filter, label) in windows {
            assert_trend_parity(
                &conn,
                &records,
                &prices,
                filter,
                grain,
                &format!("{label} grain={grain}"),
            );
        }
    }

    let hour_none = assert_trend_parity(&conn, &records, &prices, &none, "hour", "无时间窗 hour");
    assert_eq!(point_tokens(&hour_none, "2026-08-01T08"), 15);
    assert_eq!(point_tokens(&hour_none, "2026-08-01T10"), 100);
    assert_eq!(point_tokens(&hour_none, "2026-08-01T11"), 200);
    assert_eq!(point_tokens(&hour_none, "2026-08-01T12"), 80);
    assert!(
        hour_none.iter().all(|point| point.bucket.contains('T')),
        "小时粒度桶必须带时刻，不能塌成 UTC 日"
    );

    let hour_split = assert_trend_parity(&conn, &records, &prices, &split, "hour", "切分窗 hour");
    assert_eq!(
        point_tokens(&hour_split, "2026-08-01T08"),
        0,
        "09:00 之前应裁掉"
    );
    assert_eq!(point_tokens(&hour_split, "2026-08-01T10"), 100);
    assert_eq!(point_tokens(&hour_split, "2026-08-01T11"), 200);
    assert_eq!(point_tokens(&hour_split, "2026-08-01T12"), 80);
    assert_eq!(point_tokens(&hour_split, "2026-08-02T10"), 50);
    assert_eq!(point_tokens(&hour_split, "2026-08-08T10"), 300);

    let hour_intra = assert_trend_parity(
        &conn,
        &records,
        &prices,
        &intra_day,
        "hour",
        "单日内窗 hour",
    );
    assert_eq!(hour_intra.len(), 2);
    assert_eq!(hour_intra[0].bucket, "2026-08-01T10");
    assert_eq!(hour_intra[0].total_tokens, 100);
    assert_eq!(hour_intra[1].bucket, "2026-08-01T11");
    assert_eq!(hour_intra[1].total_tokens, 200);

    let day_aligned = assert_trend_parity(&conn, &records, &prices, &aligned, "day", "对齐窗 day");
    assert_eq!(
        day_aligned
            .iter()
            .map(|point| point.bucket.as_str())
            .collect::<Vec<_>>(),
        ["2026-08-01", "2026-08-02", "2026-08-08"]
    );
    // 08-01：08:00(15)+priced(100)+native(80)+claude(200)
    assert_eq!(day_aligned[0].total_tokens, 15 + 100 + 80 + 200);
    assert_eq!(day_aligned[1].total_tokens, 50);
    assert_eq!(day_aligned[2].total_tokens, 300);

    let split_sql = assert_trend_parity(&conn, &records, &prices, &split, "day", "切分窗 day");
    conn.execute("UPDATE rollup_state SET ready = 0 WHERE id = 1", [])
        .unwrap();
    let not_ready =
        assert_trend_parity(&conn, &records, &prices, &split, "day", "预聚合未就绪 day");
    assert_eq!(
        not_ready
            .iter()
            .map(|point| (point.bucket.as_str(), point.total_tokens))
            .collect::<Vec<_>>(),
        split_sql
            .iter()
            .map(|point| (point.bucket.as_str(), point.total_tokens))
            .collect::<Vec<_>>(),
    );
    let hour_not_ready = assert_trend_parity(
        &conn,
        &records,
        &prices,
        &split,
        "hour",
        "预聚合未就绪 hour",
    );
    assert_eq!(point_tokens(&hour_not_ready, "2026-08-01T08"), 0);
    assert_eq!(point_tokens(&hour_not_ready, "2026-08-01T10"), 100);
    assert_eq!(point_tokens(&hour_not_ready, "2026-08-01T12"), 80);
}

fn breakdown_key(record: &UsageRecord, dim: &str) -> String {
    match dim {
        "source" => record.source.as_str().to_string(),
        "model" => record.model.clone(),
        "provider" => record.provider.clone(),
        "project" => record.project.clone(),
        _ => unreachable!("未知分布维度：{dim}"),
    }
}

fn assert_breakdown_parity(
    conn: &rusqlite::Connection,
    records: &[UsageRecord],
    prices: &PriceTable,
    filter: &Filter,
    dim: &str,
    label: &str,
) -> Vec<crate::domain::NamedAmount> {
    let sql = query::breakdown(conn, filter, prices, dim).unwrap();
    let mem = aggregate::by_name(records, filter, prices, |record| breakdown_key(record, dim));
    assert_eq!(sql.len(), mem.len(), "{label} dim={dim} 行数");
    for (s, m) in sql.iter().zip(mem.iter()) {
        assert_eq!(s.name, m.name, "{label} dim={dim} name");
        assert_eq!(
            s.total_tokens, m.total_tokens,
            "{label} dim={dim} {} total_tokens",
            s.name
        );
        assert!(
            (s.share - m.share).abs() < 1e-9,
            "{label} dim={dim} {} share {} vs {}",
            s.name,
            s.share,
            m.share
        );
        assert_eq!(
            s.unpriced, m.unpriced,
            "{label} dim={dim} {} unpriced",
            s.name
        );
        match (s.cost, m.cost) {
            (Some(x), Some(y)) => {
                assert!(
                    (x - y).abs() < 1e-9,
                    "{label} dim={dim} {} cost {x} vs {y}",
                    s.name
                )
            }
            (None, None) => {}
            (x, y) => panic!(
                "{label} dim={dim} {} cost Option 不一致：{x:?} vs {y:?}",
                s.name
            ),
        }
    }
    sql
}

fn named_tokens(rows: &[crate::domain::NamedAmount], name: &str) -> i64 {
    rows.iter()
        .find(|row| row.name == name)
        .map(|row| row.total_tokens)
        .unwrap_or(0)
}

/// 四种时间窗 × 四个分布维度 + 单端 partial / 未就绪：与内存聚合逐字段一致。
#[test]
fn breakdown_matches_memory_across_rollup_window_shapes() {
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
    let windows = [
        (&none, "无时间窗"),
        (&aligned, "对齐窗"),
        (&split, "两端 partial 切分窗"),
        (&intra_day, "单日内窗"),
        (&head_empty, "单端 partial 为空"),
    ];
    let dims = ["source", "model", "provider", "project"];

    for dim in dims {
        for (filter, label) in windows {
            assert_breakdown_parity(
                &conn,
                &records,
                &prices,
                filter,
                dim,
                &format!("{label} dim={dim}"),
            );
        }
    }

    let split_source =
        assert_breakdown_parity(&conn, &records, &prices, &split, "source", "切分窗 source");
    // 头部 partial 的 s1 priced+native 与完整天 08-02 的 s1 必须并进同一来源。
    assert_eq!(named_tokens(&split_source, "pi"), 300);
    assert_eq!(named_tokens(&split_source, "codex"), 100 + 80 + 50);
    assert_eq!(named_tokens(&split_source, "claude"), 200);
    assert_eq!(split_source.len(), 3);

    let split_model =
        assert_breakdown_parity(&conn, &records, &prices, &split, "model", "切分窗 model");
    assert_eq!(named_tokens(&split_model, "gpt-5.5"), 300);
    assert_eq!(named_tokens(&split_model, "gpt-5.1-codex"), 100 + 80 + 50);
    assert_eq!(named_tokens(&split_model, "claude-sonnet-5"), 200);
    let mixed = split_model
        .iter()
        .find(|row| row.name == "gpt-5.1-codex")
        .expect("切分窗应有 gpt-5.1-codex");
    // priced(0.1085) + 同日 native(0.4) + 次日 native(1.5)
    assert!(
        (mixed.cost.unwrap() - 2.0085).abs() < 1e-9,
        "同会话同日混原生费用：got {:?}",
        mixed.cost
    );
    assert!(!mixed.unpriced);

    let split_provider = assert_breakdown_parity(
        &conn,
        &records,
        &prices,
        &split,
        "provider",
        "切分窗 provider",
    );
    assert_eq!(named_tokens(&split_provider, "subapi"), 300);
    assert_eq!(named_tokens(&split_provider, "official"), 100 + 80 + 50);
    assert_eq!(named_tokens(&split_provider, "anthropic"), 200);

    let split_project = assert_breakdown_parity(
        &conn,
        &records,
        &prices,
        &split,
        "project",
        "切分窗 project",
    );
    assert_eq!(named_tokens(&split_project, "/proj/a"), 100 + 80 + 200 + 50);
    assert_eq!(named_tokens(&split_project, "/proj/b"), 300);

    let intra_source = assert_breakdown_parity(
        &conn,
        &records,
        &prices,
        &intra_day,
        "source",
        "单日内窗 source",
    );
    assert_eq!(intra_source.len(), 2);
    assert_eq!(named_tokens(&intra_source, "claude"), 200);
    assert_eq!(named_tokens(&intra_source, "codex"), 100);

    let none_model =
        assert_breakdown_parity(&conn, &records, &prices, &none, "model", "无时间窗 model");
    assert!(
        none_model
            .iter()
            .find(|row| row.name == "unknown-model")
            .is_some_and(|row| row.unpriced),
        "unknown-model 应标未定价"
    );
    assert!(
        none_model
            .iter()
            .find(|row| row.name == "（未标注）")
            .is_some_and(|row| row.unpriced),
        "空模型应标未定价"
    );

    conn.execute("UPDATE rollup_state SET ready = 0 WHERE id = 1", [])
        .unwrap();
    let not_ready = assert_breakdown_parity(
        &conn,
        &records,
        &prices,
        &split,
        "source",
        "预聚合未就绪 source",
    );
    assert_eq!(
        not_ready
            .iter()
            .map(|row| (row.name.as_str(), row.total_tokens))
            .collect::<Vec<_>>(),
        split_source
            .iter()
            .map(|row| (row.name.as_str(), row.total_tokens))
            .collect::<Vec<_>>(),
    );
}

fn assert_application_analytics_parity(
    conn: &rusqlite::Connection,
    records: &[UsageRecord],
    filter: &Filter,
    grain: &str,
    label: &str,
) -> crate::domain::ApplicationAnalyticsDto {
    let sql = query::application_analytics(conn, filter, grain).unwrap();
    let mem = aggregate::application_analytics(records, filter, grain);
    assert_eq!(sql.summary, mem.summary, "{label} grain={grain} summary");
    assert_eq!(
        sql.by_application, mem.by_application,
        "{label} grain={grain} by_application"
    );
    assert_eq!(sql.trend, mem.trend, "{label} grain={grain} trend");
    assert_eq!(sql.projects, mem.projects, "{label} grain={grain} projects");
    sql
}

fn application_session_count(dto: &crate::domain::ApplicationAnalyticsDto, source: &str) -> i64 {
    dto.by_application
        .iter()
        .find(|row| row.source == source)
        .map(|row| row.metrics.session_count)
        .unwrap_or(0)
}

fn application_tokens(dto: &crate::domain::ApplicationAnalyticsDto, source: &str) -> i64 {
    dto.by_application
        .iter()
        .find(|row| row.source == source)
        .map(|row| row.metrics.total_tokens)
        .unwrap_or(0)
}

fn application_trend_tokens(dto: &crate::domain::ApplicationAnalyticsDto, bucket: &str) -> i64 {
    dto.trend
        .iter()
        .find(|point| point.bucket == bucket)
        .map(|point| point.total_tokens)
        .unwrap_or(0)
}

/// 四种时间窗 + 跨 partial 边界会话 + 未就绪：使用统计与内存聚合逐字段一致。
#[test]
fn application_analytics_matches_memory_across_rollup_window_shapes() {
    let conn = store::open_memory().unwrap();
    let records = trend_window_records();
    store::insert_records(&conn, &records).unwrap();
    store::backfill_rollup(&conn).unwrap();

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
    let windows = [
        (&none, "无时间窗"),
        (&aligned, "对齐窗"),
        (&split, "两端 partial 切分窗"),
        (&intra_day, "单日内窗"),
        (&head_empty, "单端 partial 为空"),
    ];

    for grain in ["hour", "day", "week", "month"] {
        for (filter, label) in windows {
            assert_application_analytics_parity(&conn, &records, filter, grain, label);
        }
    }

    let split_sql =
        assert_application_analytics_parity(&conn, &records, &split, "day", "切分窗 day");
    // 头部 partial 的 s1 priced+native 与完整天 08-02 的 s1 必须并成一个会话。
    assert_eq!(split_sql.summary.total_tokens, 100 + 80 + 50 + 200 + 300);
    assert_eq!(
        split_sql.summary.session_count, 3,
        "跨 partial 边界的 s1 只计一次，加上 s2 / s3"
    );
    assert_eq!(application_tokens(&split_sql, "codex"), 100 + 80 + 50);
    assert_eq!(
        application_session_count(&split_sql, "codex"),
        1,
        "codex 跨 partial 边界的 s1 不得拆成两次"
    );
    assert_eq!(application_tokens(&split_sql, "claude"), 200);
    assert_eq!(application_session_count(&split_sql, "claude"), 1);
    assert_eq!(application_tokens(&split_sql, "pi"), 300);
    assert_eq!(split_sql.by_application.len(), 3);
    assert_eq!(split_sql.trend.len(), 3);
    assert_eq!(split_sql.trend[0].bucket, "2026-08-01");
    assert_eq!(split_sql.trend[0].total_tokens, 100 + 80 + 200);
    assert_eq!(split_sql.trend[0].values["codex"], 100 + 80);
    assert_eq!(split_sql.trend[0].values["claude"], 200);
    assert_eq!(split_sql.trend[1].bucket, "2026-08-02");
    assert_eq!(split_sql.trend[1].total_tokens, 50);
    assert_eq!(split_sql.projects[0].project, "/proj/a");
    assert_eq!(split_sql.projects[0].total_tokens, 100 + 80 + 200 + 50);
    assert_eq!(split_sql.projects[0].values["codex"], 100 + 80 + 50);

    let intra_sql =
        assert_application_analytics_parity(&conn, &records, &intra_day, "day", "单日内窗 day");
    assert_eq!(intra_sql.summary.total_tokens, 100 + 200);
    assert_eq!(intra_sql.summary.session_count, 2);
    assert_eq!(intra_sql.by_application.len(), 2);

    let aligned_sql =
        assert_application_analytics_parity(&conn, &records, &aligned, "day", "对齐窗 day");
    assert_eq!(
        aligned_sql.summary.total_tokens,
        15 + 100 + 80 + 50 + 200 + 300
    );
    assert_eq!(aligned_sql.summary.session_count, 3, "s1 跨天仍只计一次");

    let hour_none =
        assert_application_analytics_parity(&conn, &records, &none, "hour", "无时间窗 hour");
    assert_eq!(application_trend_tokens(&hour_none, "2026-08-01T08"), 15);
    assert_eq!(application_trend_tokens(&hour_none, "2026-08-01T10"), 100);
    assert_eq!(application_trend_tokens(&hour_none, "2026-08-01T12"), 80);
    assert!(
        hour_none
            .trend
            .iter()
            .all(|point| point.bucket.contains('T')),
        "小时粒度桶必须带时刻，不能塌成 UTC 日"
    );

    let hour_split =
        assert_application_analytics_parity(&conn, &records, &split, "hour", "切分窗 hour");
    assert_eq!(
        application_trend_tokens(&hour_split, "2026-08-01T08"),
        0,
        "09:00 之前应裁掉，且不得并进 10:00 桶"
    );
    assert_eq!(application_trend_tokens(&hour_split, "2026-08-01T10"), 100);
    assert_eq!(application_trend_tokens(&hour_split, "2026-08-01T11"), 200);
    assert_eq!(application_trend_tokens(&hour_split, "2026-08-01T12"), 80);

    conn.execute("UPDATE rollup_state SET ready = 0 WHERE id = 1", [])
        .unwrap();
    let not_ready =
        assert_application_analytics_parity(&conn, &records, &split, "day", "预聚合未就绪 day");
    assert_eq!(not_ready.summary, split_sql.summary);
    assert_eq!(not_ready.by_application, split_sql.by_application);
    assert_eq!(not_ready.trend, split_sql.trend);
    assert_eq!(not_ready.projects, split_sql.projects);
}

/// 查询走只读连接（lock_read），切分窗也必须能算出结果。
#[test]
fn application_analytics_runs_on_readonly_connection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.sqlite");
    let path = path.to_str().unwrap();
    let write = store::open_db(path).unwrap();
    store::insert_records(&write, &diverse_records()).unwrap();
    store::rebuild_rollup(&write).unwrap();
    drop(write);

    let read = store::open_readonly(path).unwrap();
    let filter = Filter {
        from: Some("2026-08-01T09:00:00Z".into()),
        to: Some("2026-08-08T12:00:00Z".into()),
        ..Filter::default()
    };
    let dto = query::application_analytics(&read, &filter, "day").unwrap();
    assert_eq!(dto.summary.session_count, 3);
}

fn session_window_records() -> Vec<UsageRecord> {
    let mut records = overview_window_records();
    // 跨 UTC 日且标签变化：中间一条空标签不得盖住更早的值，次日非空应胜出。
    let mut early = rec(
        "2026-08-01T10:30:00Z",
        Source::Codex,
        "old-model",
        "official",
        "/old-proj",
        "s-span",
        10,
    );
    early.source_file = "/old-span.jsonl".into();
    let mut blank = rec(
        "2026-08-01T12:00:00Z",
        Source::Codex,
        "",
        "official",
        "",
        "s-span",
        5,
    );
    blank.source_file = String::new();
    let mut late = rec(
        "2026-08-02T10:00:00Z",
        Source::Codex,
        "new-model",
        "official",
        "/new-proj",
        "s-span",
        20,
    );
    late.source_file = "/new-span.jsonl".into();
    records.push(early);
    records.push(blank);
    records.push(late);
    records
}

fn find_session<'a>(
    rows: &'a [crate::domain::SessionRow],
    source: &str,
    session_id: &str,
) -> &'a crate::domain::SessionRow {
    rows.iter()
        .find(|row| row.source == source && row.session_id == session_id)
        .unwrap_or_else(|| panic!("缺少会话 {source}/{session_id}"))
}

fn assert_session_row_eq(
    sql: &crate::domain::SessionRow,
    mem: &crate::domain::SessionRow,
    label: &str,
) {
    assert_eq!(sql.source, mem.source, "{label} source");
    assert_eq!(sql.session_id, mem.session_id, "{label} session_id");
    assert_eq!(sql.total_tokens, mem.total_tokens, "{label} total_tokens");
    assert_eq!(sql.started_at, mem.started_at, "{label} started_at");
    assert_eq!(sql.ended_at, mem.ended_at, "{label} ended_at");
    assert_eq!(sql.project, mem.project, "{label} project");
    assert_eq!(sql.model, mem.model, "{label} model");
    assert_eq!(sql.source_file, mem.source_file, "{label} source_file");
    assert_eq!(sql.unpriced, mem.unpriced, "{label} unpriced");
    assert_opt_f64_eq(sql.cost, mem.cost);
}

fn assert_top_sessions_parity(
    conn: &rusqlite::Connection,
    records: &[UsageRecord],
    prices: &PriceTable,
    filter: &Filter,
    label: &str,
) -> Vec<crate::domain::SessionRow> {
    let sql = query::top_sessions(conn, filter, prices, 20).unwrap();
    let mem = aggregate::top_sessions(records, filter, prices, 20);
    assert_eq!(sql.len(), mem.len(), "{label} 行数");
    for (s, m) in sql.iter().zip(mem.iter()) {
        assert_session_row_eq(s, m, &format!("{label} {}/{}", s.source, s.session_id));
    }
    sql
}

fn assert_sessions_page_parity(
    conn: &rusqlite::Connection,
    records: &[UsageRecord],
    prices: &PriceTable,
    filter: &Filter,
    label: &str,
) -> crate::domain::SessionPage {
    let page = query::sessions_page(
        conn,
        prices,
        &SessionQuery {
            filter: filter.clone(),
            include_cost: Some(true),
            page: Some(1),
            page_size: Some(20),
            ..Default::default()
        },
    )
    .unwrap();
    let mem = aggregate::top_sessions(records, filter, prices, 20);
    assert_eq!(page.total as usize, mem.len(), "{label} total");
    assert_eq!(page.rows.len(), mem.len(), "{label} 行数");
    assert_eq!(
        page.total_tokens,
        mem.iter().map(|row| row.total_tokens).sum::<i64>(),
        "{label} total_tokens"
    );
    let expected_last = mem.iter().map(|row| row.ended_at.as_str()).max();
    assert_eq!(
        page.last_ended.as_deref(),
        expected_last,
        "{label} last_ended"
    );
    for (s, m) in page.rows.iter().zip(mem.iter()) {
        assert_session_row_eq(s, m, &format!("{label} {}/{}", s.source, s.session_id));
    }
    page
}

/// 四种时间窗 + 跨 partial 边界会话 + 混合原生费用：Top 会话 / 会话列表与内存聚合逐字段一致。
#[test]
fn top_sessions_and_sessions_page_match_memory_across_rollup_window_shapes() {
    let conn = store::open_memory().unwrap();
    let records = session_window_records();
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
    let windows = [
        (&none, "无时间窗"),
        (&aligned, "对齐窗"),
        (&split, "两端 partial 切分窗"),
        (&intra_day, "单日内窗"),
        (&head_empty, "单端 partial 为空"),
    ];

    for (filter, label) in windows {
        assert_top_sessions_parity(&conn, &records, &prices, filter, label);
        assert_sessions_page_parity(&conn, &records, &prices, filter, label);
    }

    let split_top =
        assert_top_sessions_parity(&conn, &records, &prices, &split, "切分窗 top_sessions");
    assert_eq!(
        split_top.len(),
        4,
        "跨 partial 边界的会话必须合并，不得拆成两行"
    );
    let s1 = find_session(&split_top, "codex", "s1");
    assert_eq!(s1.total_tokens, 100 + 80 + 50);
    assert_eq!(s1.started_at, "2026-08-01T10:00:00Z");
    assert_eq!(s1.ended_at, "2026-08-02T10:00:00Z");
    assert_eq!(s1.project, "/proj/a");
    assert_eq!(s1.model, "gpt-5.1-codex");
    assert_eq!(s1.source_file, "/s1.jsonl");
    assert!(!s1.unpriced, "同会话同日混原生费用后整行仍应按价");
    // priced(0.1085) + 同日 native(0.4) + 次日 native(1.5)
    assert_opt_f64_eq(s1.cost, Some(2.0085));

    let span = find_session(&split_top, "codex", "s-span");
    assert_eq!(span.total_tokens, 35);
    assert_eq!(span.started_at, "2026-08-01T10:30:00Z");
    assert_eq!(span.ended_at, "2026-08-02T10:00:00Z");
    assert_eq!(span.project, "/new-proj");
    assert_eq!(span.model, "new-model");
    assert_eq!(span.source_file, "/new-span.jsonl");

    let aligned_top =
        assert_top_sessions_parity(&conn, &records, &prices, &aligned, "对齐窗 top_sessions");
    let aligned_s1 = find_session(&aligned_top, "codex", "s1");
    assert_eq!(aligned_s1.total_tokens, 230);
    assert!(!aligned_s1.unpriced);
    // 08-01 整天走预聚合：同日 priced + native 按 has_native 拆成两行，外层仍须并回。
    assert_opt_f64_eq(aligned_s1.cost, Some(2.0085));

    let split_page =
        assert_sessions_page_parity(&conn, &records, &prices, &split, "切分窗 sessions_page");
    assert_eq!(split_page.total, 4);
    assert_eq!(split_page.total_tokens, 300 + 230 + 200 + 35);
    assert_eq!(
        find_session(&split_page.rows, "codex", "s1").total_tokens,
        230
    );
    assert_eq!(
        find_session(&split_page.rows, "codex", "s-span").project,
        "/new-proj"
    );

    let by_session = query::sessions_page(
        &conn,
        &prices,
        &SessionQuery {
            filter: split.clone(),
            sort_by: Some("session".into()),
            sort_dir: Some("asc".into()),
            include_cost: Some(true),
            page: Some(1),
            page_size: Some(20),
            ..Default::default()
        },
    )
    .unwrap();
    let ids: Vec<&str> = by_session
        .rows
        .iter()
        .map(|row| row.session_id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["s-span", "s1", "s2", "s3"],
        "排序必须作用在跨边界合并后的行上"
    );
    assert_eq!(by_session.total, 4);

    let by_latest = query::sessions_page(
        &conn,
        &prices,
        &SessionQuery {
            filter: split.clone(),
            search: Some("new-proj".into()),
            include_cost: Some(true),
            page: Some(1),
            page_size: Some(20),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_latest.total, 1, "搜索应对合并后的最晚项目");
    assert_eq!(by_latest.rows[0].session_id, "s-span");
    assert_eq!(by_latest.rows[0].project, "/new-proj");

    let by_stale = query::sessions_page(
        &conn,
        &prices,
        &SessionQuery {
            filter: split.clone(),
            search: Some("old-proj".into()),
            include_cost: Some(true),
            page: Some(1),
            page_size: Some(20),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        by_stale.total, 0,
        "已被更晚非空项目盖住的旧标签不得再命中搜索"
    );

    let page1 = query::sessions_page(
        &conn,
        &prices,
        &SessionQuery {
            filter: split.clone(),
            include_cost: Some(true),
            page: Some(1),
            page_size: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page1.total, 4);
    assert_eq!(page1.total_tokens, 300 + 230 + 200 + 35);
    assert_eq!(page1.rows.len(), 2);
    assert_eq!(page1.rows[0].session_id, "s3");
    assert_eq!(page1.rows[1].session_id, "s1");

    let intra_top = assert_top_sessions_parity(
        &conn,
        &records,
        &prices,
        &intra_day,
        "单日内窗 top_sessions",
    );
    assert_eq!(intra_top.len(), 3);
    let intra_span = find_session(&intra_top, "codex", "s-span");
    assert_eq!(intra_span.total_tokens, 10);
    assert_eq!(intra_span.project, "/old-proj");
    assert_eq!(intra_span.model, "old-model");
    assert_eq!(intra_span.source_file, "/old-span.jsonl");
    let intra_s1 = find_session(&intra_top, "codex", "s1");
    assert_eq!(
        intra_s1.total_tokens, 100,
        "12:00 的 native 行应被单日窗裁掉"
    );

    conn.execute("UPDATE rollup_state SET ready = 0 WHERE id = 1", [])
        .unwrap();
    let not_ready_top = assert_top_sessions_parity(
        &conn,
        &records,
        &prices,
        &split,
        "预聚合未就绪 top_sessions",
    );
    assert_eq!(not_ready_top.len(), split_top.len());
    assert_eq!(
        find_session(&not_ready_top, "codex", "s1").total_tokens,
        find_session(&split_top, "codex", "s1").total_tokens
    );
    let not_ready_page = assert_sessions_page_parity(
        &conn,
        &records,
        &prices,
        &split,
        "预聚合未就绪 sessions_page",
    );
    assert_eq!(not_ready_page.total, split_page.total);
    assert_eq!(not_ready_page.total_tokens, split_page.total_tokens);
}
