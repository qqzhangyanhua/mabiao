use crate::test_support::*;

/// 造一份把各条分支都踩到的数据：带 native_cost 的、按价目算的、算不出价的、
/// 跨天的同一会话、同一天多个模型、以及归档记录（查询不过滤 archived_at，得一并算进去）。
fn seed() -> Vec<UsageRecord> {
    let mut rows = Vec::new();

    // 有 native_cost：费用直接取该值，不查价目表。
    let mut native = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        0,
    );
    native.input_tokens = 500;
    native.total_tokens = 500;
    native.native_cost = Some(0.25);
    rows.push(native);

    // 同一天、同一会话，但没有 native_cost：走价目表。
    let mut priced = rec(
        "2026-08-01T11:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        0,
    );
    priced.input_tokens = 1000;
    priced.total_tokens = 1000;
    rows.push(priced);

    // 同一会话跨到第二天——按天聚合最容易在这里把会话数算重。
    let mut next_day = rec(
        "2026-08-02T09:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        0,
    );
    next_day.input_tokens = 200;
    next_day.output_tokens = 50;
    next_day.total_tokens = 250;
    rows.push(next_day);

    // 同一天另一个模型 + 另一个项目，且价目表里没有它 → unpriced。
    let mut unpriced = rec(
        "2026-08-02T12:00:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/proj/b",
        "s2",
        0,
    );
    unpriced.input_tokens = 300;
    unpriced.cache_read_tokens = 120;
    unpriced.reasoning_tokens = 30;
    unpriced.total_tokens = 450;
    rows.push(unpriced);

    // 第三天，另一个来源，用于验证 provider 大小写在两条路径上表现一致。
    let mut mixed_case = rec(
        "2026-08-03T08:00:00Z",
        Source::Pi,
        "gpt-5.5",
        "cpaApi",
        "/proj/b",
        "s3",
        0,
    );
    mixed_case.input_tokens = 700;
    mixed_case.cache_creation_tokens = 40;
    mixed_case.total_tokens = 740;
    rows.push(mixed_case);

    // 同一天、同一会话里换过项目和模型，且两者的首末次序相反：
    //   /proj/x 首次 10:00、末次 15:00；/proj/y 只有 12:00。
    // 「最晚非空」必须选出 /proj/x + m-a（按末次），选 /proj/y + m-b 就是按首次取，是错的。
    // 没有这组数据，把聚合键从 last_at 换成 first_at 也测不出来。
    for (at, project, model, tokens) in [
        ("2026-08-04T10:00:00Z", "/proj/x", "m-a", 10),
        ("2026-08-04T12:00:00Z", "/proj/y", "m-b", 20),
        ("2026-08-04T15:00:00Z", "/proj/x", "m-a", 30),
    ] {
        let mut row = rec(at, Source::Codex, model, "official", project, "s4", 0);
        row.input_tokens = tokens;
        row.total_tokens = tokens;
        rows.push(row);
    }

    rows
}

fn price_table() -> PriceTable {
    PriceTable {
        prices: vec![
            PriceEntry {
                model: "gpt-5.1-codex".into(),
                provider: Some("official".into()),
                input: 0.001,
                output: 0.002,
                cache_read: 0.0001,
                cache_creation: 0.0003,
                origin: PriceOrigin::User,
            },
            // provider 为 None 的兜底条目，验证 pf 那条 JOIN。
            PriceEntry {
                model: "gpt-5.5".into(),
                provider: None,
                input: 0.005,
                output: 0.006,
                cache_read: 0.0005,
                cache_creation: 0.0007,
                origin: PriceOrigin::Snapshot,
            },
        ],
    }
}

/// 覆盖全部数据的时间范围。带上 from/to 会走「中间日预聚合 + 边界明细」；
/// 范围又足够宽，边界天没有数据，结果应与无时间窗的纯预聚合路径一致。
fn full_range() -> Filter {
    Filter {
        from: Some("2000-01-01T00:00:00Z".into()),
        to: Some("2100-01-01T00:00:00Z".into()),
        ..Filter::default()
    }
}

/// 强制走原始表：清掉就绪位。用于对照 hybrid / 纯预聚合路径。
fn with_rollup_disabled<T>(conn: &rusqlite::Connection, f: impl FnOnce() -> T) -> T {
    conn.execute("UPDATE rollup_state SET ready = 0 WHERE id = 1", [])
        .unwrap();
    let out = f();
    conn.execute("UPDATE rollup_state SET ready = 1 WHERE id = 1", [])
        .unwrap();
    out
}

/// 费用比对要留容差：预聚合是「token 先求和再乘单价」，原始表是「每行乘完再相加」，
/// 数学等价而浮点不等，真实数据上实测差在 1e-14 量级。token 与占比则必须逐位相同。
fn cost_close(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0),
        _ => false,
    }
}

fn assert_overview_eq(rollup: &crate::domain::OverviewDto, raw: &crate::domain::OverviewDto) {
    assert_eq!(rollup.total_tokens, raw.total_tokens);
    assert_eq!(rollup.input_tokens, raw.input_tokens);
    assert_eq!(rollup.output_tokens, raw.output_tokens);
    assert_eq!(rollup.cache_read_tokens, raw.cache_read_tokens);
    assert_eq!(rollup.cache_creation_tokens, raw.cache_creation_tokens);
    assert_eq!(rollup.reasoning_tokens, raw.reasoning_tokens);
    assert_eq!(rollup.session_count, raw.session_count);
    assert_eq!(rollup.unpriced, raw.unpriced);
    assert!(
        cost_close(rollup.cost, raw.cost),
        "cost 超出容差：{:?} vs {:?}",
        rollup.cost,
        raw.cost
    );
}

fn prepared() -> rusqlite::Connection {
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &seed()).unwrap();
    // 必须走 backfill_rollup 而不是 rebuild_rollup：后者只填数据、不置就绪位，
    // 查询会当预聚合表没建好而回退原始表——测试照样通过，却什么都没测到。
    store::backfill_rollup(&conn).unwrap();
    conn
}

/// 防呆：`prepared()` 造出来的库必须真的在走预聚合表。
/// 否则下面那些「两条路径结果一致」的断言会退化成「原始表和自己一致」。
#[test]
fn prepared_fixture_actually_uses_the_rollup() {
    let conn = prepared();
    assert!(store::rollup_is_ready(&conn), "夹具应处于就绪状态");
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM usage_rollup", [], |row| row.get(0))
        .unwrap();
    assert!(rows > 0, "预聚合表不应为空");
}

#[test]
fn rollup_overview_matches_raw_table() {
    let conn = prepared();
    let prices = price_table();

    let via_rollup = query::overview(&conn, &Filter::default(), &prices).unwrap();
    let via_raw = query::overview(&conn, &full_range(), &prices).unwrap();
    assert_overview_eq(&via_rollup, &via_raw);

    // 跨天的 s1 只能算一个会话。
    assert_eq!(via_rollup.session_count, 4);
    assert_eq!(
        via_rollup.total_tokens,
        500 + 1000 + 250 + 450 + 740 + 10 + 20 + 30
    );
    assert!(
        via_rollup.unpriced,
        "claude-sonnet-5 没有价目，应标记未定价"
    );
}

#[test]
fn rollup_trend_matches_raw_table_for_every_supported_grain() {
    let conn = prepared();
    let prices = price_table();
    // hour 比天更细，预聚合表取不出来，应自动回退到原始表——结果同样要对得上。
    for grain in ["day", "week", "month", "hour"] {
        let via_rollup = query::trend(&conn, &Filter::default(), &prices, grain).unwrap();
        let via_raw = query::trend(&conn, &full_range(), &prices, grain).unwrap();
        assert_eq!(via_rollup.len(), via_raw.len(), "grain={grain} 桶数不一致");
        for (a, b) in via_rollup.iter().zip(via_raw.iter()) {
            assert_eq!(a.bucket, b.bucket, "grain={grain}");
            assert_eq!(
                a.total_tokens, b.total_tokens,
                "grain={grain} 桶 {}",
                a.bucket
            );
            assert_eq!(
                a.input_tokens, b.input_tokens,
                "grain={grain} 桶 {}",
                a.bucket
            );
            assert_eq!(
                a.output_tokens, b.output_tokens,
                "grain={grain} 桶 {}",
                a.bucket
            );
            assert_eq!(
                a.cache_read_tokens, b.cache_read_tokens,
                "grain={grain} 桶 {}",
                a.bucket
            );
            assert_eq!(
                a.reasoning_tokens, b.reasoning_tokens,
                "grain={grain} 桶 {}",
                a.bucket
            );
            assert!(
                cost_close(a.cost, b.cost),
                "grain={grain} 桶 {} cost {:?} vs {:?}",
                a.bucket,
                a.cost,
                b.cost
            );
        }
    }
}

#[test]
fn rollup_breakdown_matches_raw_table_for_every_dimension() {
    let conn = prepared();
    let prices = price_table();
    for dimension in ["source", "model", "provider", "project", "application"] {
        let via_rollup = query::breakdown(&conn, &Filter::default(), &prices, dimension).unwrap();
        let via_raw = query::breakdown(&conn, &full_range(), &prices, dimension).unwrap();
        assert_eq!(
            via_rollup.len(),
            via_raw.len(),
            "dimension={dimension} 行数不一致"
        );
        for (a, b) in via_rollup.iter().zip(via_raw.iter()) {
            assert_eq!(a.name, b.name, "dimension={dimension}");
            assert_eq!(
                a.total_tokens, b.total_tokens,
                "dimension={dimension} {}",
                a.name
            );
            assert_eq!(a.share, b.share, "dimension={dimension} {}", a.name);
            assert_eq!(a.unpriced, b.unpriced, "dimension={dimension} {}", a.name);
            assert!(
                cost_close(a.cost, b.cost),
                "dimension={dimension} {} cost {:?} vs {:?}",
                a.name,
                a.cost,
                b.cost
            );
        }
    }
}

#[test]
fn rollup_honours_dimension_filters() {
    let conn = prepared();
    let prices = price_table();
    let cases = [
        Filter {
            sources: vec!["codex".into()],
            ..Filter::default()
        },
        Filter {
            models: vec!["gpt-5.1-codex".into()],
            ..Filter::default()
        },
        Filter {
            projects: vec!["/proj/b".into()],
            ..Filter::default()
        },
        Filter {
            providers: vec!["cpaApi".into()],
            ..Filter::default()
        },
        Filter {
            sources: vec!["codex".into(), "claude".into()],
            projects: vec!["/proj/a".into()],
            ..Filter::default()
        },
    ];
    for filter in cases {
        let raw = Filter {
            from: full_range().from,
            to: full_range().to,
            ..filter.clone()
        };
        assert_overview_eq(
            &query::overview(&conn, &filter, &prices).unwrap(),
            &query::overview(&conn, &raw, &prices).unwrap(),
        );
    }
}

/// 预聚合表落后于原始表就是错的数字。这条盯住摄取之外的那条写入路径。
#[test]
fn rebuilding_rollup_reflects_later_writes() {
    let conn = prepared();
    let prices = price_table();
    let before = query::overview(&conn, &Filter::default(), &prices).unwrap();

    let mut extra = rec(
        "2026-08-04T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/c",
        "s9",
        0,
    );
    extra.input_tokens = 100;
    extra.total_tokens = 100;
    store::insert_records(&conn, &[extra]).unwrap();

    // 还没重建：预聚合表看不到新记录，这正是必须在同一个事务里重建的理由。
    let stale = query::overview(&conn, &Filter::default(), &prices).unwrap();
    assert_eq!(stale, before, "重建前预聚合表不应含新数据");

    store::rebuild_rollup(&conn).unwrap();
    let after = query::overview(&conn, &Filter::default(), &prices).unwrap();
    assert_eq!(after.total_tokens, before.total_tokens + 100);
    assert_overview_eq(
        &after,
        &query::overview(&conn, &full_range(), &prices).unwrap(),
    );
}

/// 老库升级 / 从旧备份恢复时，`usage_rollup` 还没建起来。开库不该同步补建
/// （350 万行要十几秒，启动会像卡死），而应让查询先回退原始表给出正确数字。
#[test]
fn opening_a_stale_database_falls_back_until_backfilled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.sqlite");
    let db = path.to_string_lossy().to_string();
    {
        let conn = store::open_db(&db).unwrap();
        store::insert_records(&conn, &seed()).unwrap();
        // 模拟旧版本留下的库：有原始记录，预聚合表既空又未就绪。
    }
    let conn = store::open_db(&db).unwrap();
    let prices = price_table();

    // 补建是后台做的，开库时还没就绪——此时查询必须回退原始表给出正确数字，
    // 而不是照着空表答 0。
    assert!(!store::rollup_is_ready(&conn), "开库不应同步补建");
    assert!(
        store::rollup_needs_backfill(&conn).unwrap(),
        "应识别出待补建"
    );
    let before = query::overview(&conn, &Filter::default(), &prices).unwrap();
    assert_eq!(
        before.total_tokens,
        500 + 1000 + 250 + 450 + 740 + 10 + 20 + 30,
        "未就绪时应走原始表，不能返回 0"
    );

    // 补建完成后自动切到预聚合表，结果不变。
    store::backfill_rollup(&conn).unwrap();
    assert!(store::rollup_is_ready(&conn));
    assert!(!store::rollup_needs_backfill(&conn).unwrap());
    assert_overview_eq(
        &query::overview(&conn, &Filter::default(), &prices).unwrap(),
        &before,
    );
}

/// 补建期间摄取不能往预聚合表里写：只写进一两天会让它「非空却残缺」，
/// 而就绪位一旦因此被误认，查询就会静默少掉全部历史。
#[test]
fn ingest_leaves_the_rollup_alone_until_backfill_completes() {
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &seed()).unwrap();
    assert!(!store::rollup_is_ready(&conn));

    // 模拟摄取在补建完成前动了一天。
    let one: std::collections::BTreeSet<String> =
        std::iter::once("2026-08-01".to_string()).collect();
    let mut report = crate::domain::IngestReport {
        records_written: 1,
        ..Default::default()
    };
    report.touched_days = one;
    crate::ingest::sync_rollup_for_tests(&conn, &report).unwrap();

    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM usage_rollup", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0, "未就绪时摄取不该碰预聚合表");

    // 查询照旧走原始表，数字完整。
    let prices = price_table();
    assert_eq!(
        query::overview(&conn, &Filter::default(), &prices)
            .unwrap()
            .total_tokens,
        500 + 1000 + 250 + 450 + 740 + 10 + 20 + 30
    );
}

/// 按天增量重建必须和整表重建给出一模一样的表。摄取走的是增量那条路，
/// 一旦两者会有出入，界面上就是「昨天的数字对、今天的不对」这种最难查的错。
#[test]
fn rebuilding_selected_days_matches_a_full_rebuild() {
    let conn = prepared();

    let snapshot = |conn: &rusqlite::Connection| -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT day, source, model, provider, project, session_id, has_native,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        reasoning_tokens, total_tokens, native_cost, record_count,
                        first_at, last_at, COALESCE(file_key, '')
                 FROM usage_rollup
                 ORDER BY day, source, session_id, model, provider, project, has_native",
            )
            .unwrap();
        stmt.query_map([], |row| {
            let mut parts = Vec::new();
            for i in 0..18 {
                parts.push(
                    row.get::<_, rusqlite::types::Value>(i)
                        .map(|v| format!("{v:?}"))?,
                );
            }
            Ok(parts.join("|"))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };

    let full = snapshot(&conn);

    // 逐天重算全部日期，结果必须与整表重建一致。
    let days: std::collections::BTreeSet<String> = conn
        .prepare("SELECT DISTINCT substr(occurred_at, 1, 10) FROM usage_records")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    conn.execute("DELETE FROM usage_rollup", []).unwrap();
    store::rebuild_rollup_days(&conn, &days).unwrap();
    assert_eq!(snapshot(&conn), full, "逐天重建与整表重建结果不一致");

    // 只重算其中一天，其余各天的行不能被动到。
    let one: std::collections::BTreeSet<String> =
        std::iter::once("2026-08-02".to_string()).collect();
    store::rebuild_rollup_days(&conn, &one).unwrap();
    assert_eq!(snapshot(&conn), full, "单日重建不应影响其它天");
}

/// 摄取只该重算被动过的那几天。这条盯住 `days_for_file`：漏了旧日期，
/// 改动前占的那天就会留着过期的聚合行。
#[test]
fn touched_days_cover_both_the_old_and_the_new_dates() {
    let conn = store::open_memory().unwrap();
    let mut first = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        0,
    );
    first.source_file = "/same.jsonl".into();
    first.input_tokens = 100;
    first.total_tokens = 100;
    store::insert_records(&conn, &[first]).unwrap();
    store::rebuild_rollup(&conn).unwrap();

    let before = store::days_for_file(&conn, "/same.jsonl").unwrap();
    assert_eq!(before, vec!["2026-08-01".to_string()]);

    // 同一个文件重新解析出的记录挪到了另一天——旧的那天也必须跟着重算。
    let mut moved = rec(
        "2026-08-05T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        0,
    );
    moved.source_file = "/same.jsonl".into();
    moved.input_tokens = 100;
    moved.total_tokens = 100;
    store::delete_records_for_file(&conn, "/same.jsonl").unwrap();
    store::insert_records(&conn, &[moved]).unwrap();
    let after = store::days_for_file(&conn, "/same.jsonl").unwrap();

    let touched: std::collections::BTreeSet<String> = before.into_iter().chain(after).collect();
    store::rebuild_rollup_days(&conn, &touched).unwrap();

    let stale: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM usage_rollup WHERE day = '2026-08-01'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stale, 0, "记录搬走后，原来那天的聚合行应被清掉");
    let prices = price_table();
    assert_overview_eq(
        &query::overview(&conn, &Filter::default(), &prices).unwrap(),
        &query::overview(&conn, &full_range(), &prices).unwrap(),
    );
}

#[test]
fn rollup_top_sessions_matches_raw_table() {
    let conn = prepared();
    let prices = price_table();
    for limit in [1usize, 3, 8, 100] {
        let via_rollup = query::top_sessions(&conn, &Filter::default(), &prices, limit).unwrap();
        let via_raw = query::top_sessions(&conn, &full_range(), &prices, limit).unwrap();
        assert_eq!(via_rollup.len(), via_raw.len(), "limit={limit} 行数不一致");
        for (a, b) in via_rollup.iter().zip(via_raw.iter()) {
            assert_eq!(a.session_id, b.session_id, "limit={limit}");
            assert_eq!(a.source, b.source, "limit={limit}");
            assert_eq!(
                a.total_tokens, b.total_tokens,
                "limit={limit} {}",
                a.session_id
            );
            assert_eq!(a.started_at, b.started_at, "limit={limit} {}", a.session_id);
            assert_eq!(a.ended_at, b.ended_at, "limit={limit} {}", a.session_id);
            // 展示标签走的是「最晚非空」，预聚合版用 last_at 现拼键，必须选出同一个值。
            assert_eq!(
                a.project, b.project,
                "limit={limit} {} 的项目",
                a.session_id
            );
            assert_eq!(a.model, b.model, "limit={limit} {} 的模型", a.session_id);
            assert_eq!(
                a.source_file, b.source_file,
                "limit={limit} {} 的文件",
                a.session_id
            );
            assert_eq!(a.unpriced, b.unpriced, "limit={limit} {}", a.session_id);
            assert!(
                cost_close(a.cost, b.cost),
                "limit={limit} {} cost",
                a.session_id
            );
        }
    }
}

#[test]
fn rollup_sessions_page_matches_raw_table() {
    let conn = prepared();
    let prices = price_table();
    let variants = [
        SessionQuery::default(),
        SessionQuery {
            include_cost: Some(true),
            ..Default::default()
        },
        SessionQuery {
            search: Some("proj".into()),
            include_cost: Some(true),
            ..Default::default()
        },
        SessionQuery {
            sort_by: Some("time".into()),
            sort_dir: Some("asc".into()),
            ..Default::default()
        },
        SessionQuery {
            sort_by: Some("cost".into()),
            include_cost: Some(true),
            ..Default::default()
        },
        SessionQuery {
            page: Some(2),
            page_size: Some(2),
            ..Default::default()
        },
    ];
    for query in variants {
        let raw_query = SessionQuery {
            filter: full_range(),
            ..query.clone()
        };
        let via_rollup = query::sessions_page(&conn, &prices, &query).unwrap();
        let via_raw = query::sessions_page(&conn, &prices, &raw_query).unwrap();
        assert_eq!(via_rollup.total, via_raw.total, "{query:?} 总数不一致");
        assert_eq!(
            via_rollup.rows.len(),
            via_raw.rows.len(),
            "{query:?} 行数不一致"
        );
        for (a, b) in via_rollup.rows.iter().zip(via_raw.rows.iter()) {
            assert_eq!(a.session_id, b.session_id, "{query:?}");
            assert_eq!(a.total_tokens, b.total_tokens, "{query:?} {}", a.session_id);
            assert_eq!(a.project, b.project, "{query:?} {}", a.session_id);
            assert_eq!(a.model, b.model, "{query:?} {}", a.session_id);
            assert_eq!(a.source_file, b.source_file, "{query:?} {}", a.session_id);
            assert!(
                cost_close(a.cost, b.cost),
                "{query:?} {} cost",
                a.session_id
            );
        }
    }
}

#[test]
fn rollup_application_analytics_matches_raw_table() {
    let conn = prepared();
    // hour 比天更细，应回退原始表；其余粒度走预聚合。两条路径结果都要一致。
    for grain in ["day", "week", "month", "hour"] {
        let via_rollup = query::application_analytics(&conn, &Filter::default(), grain).unwrap();
        let via_raw = query::application_analytics(&conn, &full_range(), grain).unwrap();
        assert_eq!(
            via_rollup.summary, via_raw.summary,
            "grain={grain} 总览不一致"
        );
        assert_eq!(
            via_rollup.by_application, via_raw.by_application,
            "grain={grain} 按应用不一致"
        );
        assert_eq!(via_rollup.trend, via_raw.trend, "grain={grain} 趋势不一致");
        assert_eq!(
            via_rollup.projects, via_raw.projects,
            "grain={grain} 按项目不一致"
        );
    }
}

/// 默认「近 7 天」这类带时分秒的窗口：中间完整 UTC 日走预聚合，两端边界走明细，
/// 结果必须与整段扫原始表一致。这是把 can_use_rollup 从「有 from/to 就禁用」
/// 改成 hybrid 之后最重要的回归。
#[test]
fn hybrid_time_window_matches_raw_table() {
    let conn = prepared();
    let prices = price_table();
    // 窗盖住 08-01 中午 → 08-04 下午：08-02、08-03 是完整中间日；两端是半日。
    let filter = Filter {
        from: Some("2026-08-01T12:00:00Z".into()),
        to: Some("2026-08-04T14:00:00Z".into()),
        ..Filter::default()
    };

    let via_hybrid = query::overview(&conn, &filter, &prices).unwrap();
    let via_raw = with_rollup_disabled(&conn, || query::overview(&conn, &filter, &prices).unwrap());
    assert_overview_eq(&via_hybrid, &via_raw);

    // 跨天的 s1：08-01 11:00 落在窗外，08-02 09:00 在中间日——会话仍应算到。
    // 08-01 10:00 native 也在窗外。窗内应有：priced 被切掉后的 next_day(250) +
    // unpriced(450) + mixed(740) + s4 的 10/20（15:00 的 30 在 14:00 之后被切掉）。
    assert_eq!(via_hybrid.total_tokens, 250 + 450 + 740 + 10 + 20);
    assert_eq!(via_hybrid.session_count, 4); // s1, s2, s3, s4

    for grain in ["day", "week", "month"] {
        let hybrid = query::trend(&conn, &filter, &prices, grain).unwrap();
        let raw = with_rollup_disabled(&conn, || {
            query::trend(&conn, &filter, &prices, grain).unwrap()
        });
        assert_eq!(hybrid.len(), raw.len(), "grain={grain}");
        for (a, b) in hybrid.iter().zip(raw.iter()) {
            assert_eq!(a.total_tokens, b.total_tokens, "grain={grain} {}", a.bucket);
            assert!(
                cost_close(a.cost, b.cost),
                "grain={grain} {} cost",
                a.bucket
            );
        }
    }

    for dimension in ["source", "model", "project"] {
        let hybrid = query::breakdown(&conn, &filter, &prices, dimension).unwrap();
        let raw = with_rollup_disabled(&conn, || {
            query::breakdown(&conn, &filter, &prices, dimension).unwrap()
        });
        assert_eq!(hybrid.len(), raw.len(), "dimension={dimension}");
        for (a, b) in hybrid.iter().zip(raw.iter()) {
            assert_eq!(a.total_tokens, b.total_tokens, "{dimension} {}", a.name);
        }
    }

    let hybrid_aa = query::application_analytics(&conn, &filter, "day").unwrap();
    let raw_aa = with_rollup_disabled(&conn, || {
        query::application_analytics(&conn, &filter, "day").unwrap()
    });
    assert_eq!(hybrid_aa.summary, raw_aa.summary);
    assert_eq!(hybrid_aa.by_application, raw_aa.by_application);
    assert_eq!(hybrid_aa.trend, raw_aa.trend);
    assert_eq!(hybrid_aa.projects, raw_aa.projects);
}

/// 起止落在同一 UTC 日时没有完整中间日，应整段回退原始表；数字仍要对。
#[test]
fn same_utc_day_window_falls_back_to_raw_and_stays_correct() {
    let conn = prepared();
    let prices = price_table();
    let filter = Filter {
        from: Some("2026-08-01T10:30:00Z".into()),
        to: Some("2026-08-01T11:30:00Z".into()),
        ..Filter::default()
    };
    let via = query::overview(&conn, &filter, &prices).unwrap();
    let via_raw = with_rollup_disabled(&conn, || query::overview(&conn, &filter, &prices).unwrap());
    assert_overview_eq(&via, &via_raw);
    // 只剩 11:00 那条 priced（1000）；10:00 native 在 from 之前。
    assert_eq!(via.total_tokens, 1000);
    assert_eq!(via.session_count, 1);
}
