pub fn message_events(
    detail: &crate::domain::ConversationDetailDto,
) -> Vec<&crate::domain::ConversationEvent> {
    detail
        .events
        .iter()
        .filter(|event| event.kind == crate::domain::ConversationEventKind::Message)
        .collect()
}

pub fn message_texts(detail: &crate::domain::ConversationDetailDto) -> Vec<String> {
    message_events(detail)
        .into_iter()
        .filter_map(|event| event.text.clone())
        .collect()
}

pub fn usage_rows(
    conn: &rusqlite::Connection,
    source: &str,
    session_id: &str,
) -> Vec<crate::domain::UsageRecord> {
    crate::conversation::usage_records_page(conn, source, session_id, 1, 200)
        .unwrap()
        .rows
}

pub fn write_home_fixture(
    home: &std::path::Path,
    relative_path: &str,
    fixture_name: &str,
) -> std::path::PathBuf {
    let path = home.join(relative_path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, fixture(fixture_name)).unwrap();
    path
}

pub fn assert_conversation_index_matches_parse(
    conn: &rusqlite::Connection,
    home: &std::path::Path,
    source: &str,
    session_id: &str,
) {
    let parsed = crate::conversation::load_detail(conn, home, source, session_id).unwrap();
    let indexed = crate::conversation::indexed_events(conn, source, session_id).unwrap();
    assert!(
        indexed.iter().all(|event| event.details.is_null()),
        "{source}/{session_id} 索引不得存 details"
    );
    assert_eq!(
        indexed,
        parsed
            .events
            .into_iter()
            .map(|mut event| {
                event.details = serde_json::Value::Null;
                event
            })
            .collect::<Vec<_>>(),
        "{source}/{session_id} 索引事件序列必须与整份解析逐字段一致（不含 details）"
    );
}

pub fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(path).expect("fixture")
}

pub fn rec(
    occurred_at: &str,
    source: crate::domain::Source,
    model: &str,
    provider: &str,
    project: &str,
    session_id: &str,
    total: i64,
) -> crate::domain::UsageRecord {
    crate::domain::UsageRecord {
        occurred_at: occurred_at.to_string(),
        source,
        model: model.to_string(),
        provider: provider.to_string(),
        project: project.to_string(),
        session_id: session_id.to_string(),
        source_file: format!("/{session_id}.jsonl"),
        input_tokens: total,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: total,
        native_cost: None,
    }
}

pub fn seed_records() -> Vec<crate::domain::UsageRecord> {
    use crate::domain::Source;
    vec![
        rec(
            "2026-08-01T10:00:00Z",
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            100,
        ),
        rec(
            "2026-08-02T10:00:00Z",
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/a",
            "s2",
            300,
        ),
        rec(
            "2026-08-08T10:00:00Z",
            Source::Pi,
            "gpt-5.5",
            "subapi",
            "/proj/b",
            "s3",
            50,
        ),
    ]
}

pub fn rollup_sum(
    records: &[crate::domain::UsageRecord],
    filter: &crate::domain::Filter,
    prices: &crate::domain::PriceTable,
    selector: impl Fn(&crate::domain::UsageRecord) -> String,
) -> i64 {
    crate::aggregate::by_name(records, filter, prices, selector)
        .iter()
        .map(|row| row.total_tokens)
        .sum()
}

pub fn assert_rollups_match_overview(
    records: &[crate::domain::UsageRecord],
    filter: &crate::domain::Filter,
) {
    let prices = crate::domain::PriceTable::default();
    let overview = crate::aggregate::overview(records, filter, &prices);
    assert_eq!(
        overview.total_tokens,
        rollup_sum(records, filter, &prices, |r| r.source.as_str().to_string())
    );
    assert_eq!(
        overview.total_tokens,
        rollup_sum(records, filter, &prices, |r| r.model.clone())
    );
    assert_eq!(
        overview.total_tokens,
        rollup_sum(records, filter, &prices, |r| r.provider.clone())
    );
    assert_eq!(
        overview.total_tokens,
        rollup_sum(records, filter, &prices, |r| r.project.clone())
    );
    let session_total: i64 = crate::aggregate::top_sessions(records, filter, &prices, usize::MAX)
        .iter()
        .map(|row| row.total_tokens)
        .sum();
    assert_eq!(overview.total_tokens, session_total);
}

pub fn write_all_source_fixtures(home: &std::path::Path) {
    let paths: [(&str, &str); 8] = [
        (".codex/sessions/one.jsonl", "codex.jsonl"),
        (
            ".claude/projects/-Users-zhangyanhua-AI-TradingAgents-CN/04868551-34c3-4588-b984-6ae9a5d95f8a.jsonl",
            "claude.jsonl",
        ),
        (
            ".pi/agent/sessions/--Users-zhangyanhua-workCode-ruoyi-ui-vue3--/s.jsonl",
            "pi.jsonl",
        ),
        (
            ".kimi/sessions/hash/bd1ab6fc-768d-4cff-b4c4-221a583c3af8/wire.jsonl",
            "kimi-wire.jsonl",
        ),
        (
            ".gemini/tmp/ruoyi-ui-vue3/chats/session-2026-03-07.json",
            "gemini-session.json",
        ),
        (
            ".grok/sessions/%2FUsers%2Fzhangyanhua%2FAI%2FTradingAgents-CN/019fd235/updates.jsonl",
            "grok-updates.jsonl",
        ),
        (".qwen/tmp/hash/logs.json", "qwen-logs.json"),
        (
            ".copilot/session-state/c0ffee11-2222-4333-8444-555566667777/events.jsonl",
            "copilot-events.jsonl",
        ),
    ];
    for (rel, name) in paths {
        let path = home.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, fixture(name)).unwrap();
    }
    let factory = home.join(
        ".factory/sessions/-Users-zhangyanhua-AI-cli/9ab2ca7b-bd30-495b-9434-07892ee0e5e6.settings.json",
    );
    std::fs::create_dir_all(factory.parent().unwrap()).unwrap();
    std::fs::write(&factory, fixture("factory.settings.json")).unwrap();
    let dsh = home.join(".dsh/sessions/--Users-zhangyanhua-AI-pi--/session.jsonl.zstd");
    std::fs::create_dir_all(dsh.parent().unwrap()).unwrap();
    let compressed = zstd::encode_all(fixture("dsh.jsonl").as_bytes(), 0).unwrap();
    std::fs::write(&dsh, compressed).unwrap();
}

pub fn window_rec(
    occurred_at: &str,
    source: crate::domain::Source,
    session_id: &str,
    total: i64,
) -> crate::domain::UsageRecord {
    let mut record = rec(
        occurred_at,
        source,
        "claude-sonnet-5",
        "anthropic",
        "/proj/a",
        session_id,
        total,
    );
    record.native_cost = Some(total as f64 / 1000.0);
    record
}

pub fn assert_opt_f64_eq(a: Option<f64>, b: Option<f64>) {
    match (a, b) {
        (Some(x), Some(y)) => assert!((x - y).abs() < 1e-9, "金额不一致：{x} vs {y}"),
        (None, None) => {}
        (x, y) => panic!("金额 Option 不一致：{x:?} vs {y:?}"),
    }
}

pub fn diverse_prices() -> crate::domain::PriceTable {
    use crate::domain::{PriceEntry, PriceOrigin, PriceTable};
    PriceTable {
        prices: vec![
            PriceEntry {
                model: "gpt-5.1-codex".into(),
                provider: Some("official".into()),
                input: 0.001,
                output: 0.002,
                cache_read: 0.0005,
                cache_creation: 0.003,
                origin: PriceOrigin::User,
            },
            PriceEntry {
                model: "claude-sonnet-5".into(),
                provider: None,
                input: 0.003,
                output: 0.015,
                cache_read: 0.001,
                cache_creation: 0.0,
                origin: PriceOrigin::User,
            },
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
    }
}

/// 覆盖：多来源、精确/兜底 provider 价格、native_cost、空项目、跨来源同名会话。
pub fn diverse_records() -> Vec<crate::domain::UsageRecord> {
    use crate::domain::Source;
    let mut r1 = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        100,
    );
    r1.input_tokens = 80;
    r1.output_tokens = 10;
    r1.cache_read_tokens = 5;
    r1.cache_creation_tokens = 2;
    r1.reasoning_tokens = 3;

    let mut r2 = rec(
        "2026-08-02T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        50,
    );
    r2.input_tokens = 40;
    r2.output_tokens = 5;
    r2.cache_read_tokens = 3;
    r2.cache_creation_tokens = 1;
    r2.reasoning_tokens = 1;
    r2.native_cost = Some(1.5);

    let mut r3 = rec(
        "2026-08-01T11:00:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/proj/a",
        "s2",
        200,
    );
    r3.input_tokens = 150;
    r3.output_tokens = 50;

    let mut r4 = rec(
        "2026-08-08T10:00:00Z",
        Source::Pi,
        "gpt-5.5",
        "subapi",
        "/proj/b",
        "s3",
        300,
    );
    r4.input_tokens = 100;
    r4.output_tokens = 200;
    r4.native_cost = Some(0.25);

    let mut r5 = rec(
        "2026-08-09T10:00:00Z",
        Source::Pi,
        "gpt-5.5",
        "siliconflow",
        "/proj/b",
        "s4",
        60,
    );
    r5.input_tokens = 60;

    let mut r6 = rec(
        "2026-08-10T10:00:00Z",
        Source::Codex,
        "unknown-model",
        "official",
        "",
        "s5",
        30,
    );
    r6.input_tokens = 30;

    vec![r1, r2, r3, r4, r5, r6]
}

pub fn local_noon_iso(date: chrono::NaiveDate) -> String {
    local_time_iso(date, 12, 0, 0)
}

/// 本地日历日某一时刻 -> UTC RFC3339，用于构造与运行机器时区无关、但仍落在指定
/// 本地日期的测试记录（跨午夜场景需要中午以外的时刻）。
pub fn local_time_iso(date: chrono::NaiveDate, hour: u32, min: u32, sec: u32) -> String {
    use chrono::{Local, Utc};
    let naive = date.and_hms_opt(hour, min, sec).expect("valid time");
    naive
        .and_local_timezone(Local)
        .earliest()
        .or_else(|| naive.and_local_timezone(Local).latest())
        .expect("local time")
        .with_timezone(&Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn seed_cursor_transcript(
    home: &std::path::Path,
    project_slug: &str,
    session_id: &str,
    content: &str,
) -> std::path::PathBuf {
    let path = home
        .join(".cursor")
        .join("projects")
        .join(project_slug)
        .join("agent-transcripts")
        .join(session_id)
        .join(format!("{session_id}.jsonl"));
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
    std::fs::write(&path, content).expect("write transcript");
    path
}

pub fn seed_ai_code_hashes(home: &std::path::Path, rows: &[(&str, &str, i64, &str)]) {
    seed_ai_code_hash_details(
        home,
        &rows
            .iter()
            .map(|(conversation_id, model, timestamp, file_name)| {
                (
                    *conversation_id,
                    *model,
                    *timestamp,
                    *file_name,
                    "composer",
                    "rs",
                )
            })
            .collect::<Vec<_>>(),
    );
}

pub fn seed_ai_code_hash_details(
    home: &std::path::Path,
    rows: &[(&str, &str, i64, &str, &str, &str)],
) {
    let db_path = home.join(".cursor/ai-tracking/ai-code-tracking.db");
    std::fs::create_dir_all(db_path.parent().expect("parent")).expect("create dirs");
    let conn = rusqlite::Connection::open(&db_path).expect("open tracking db");
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS ai_code_hashes (
            hash TEXT,
            source TEXT,
            fileExtension TEXT,
            fileName TEXT,
            requestId TEXT,
            conversationId TEXT,
            timestamp INTEGER,
            createdAt INTEGER,
            model TEXT
        );
        "#,
    )
    .expect("create table");
    for (conversation_id, model, timestamp, file_name, source, extension) in rows {
        conn.execute(
            r#"
            INSERT INTO ai_code_hashes(
                hash, source, fileExtension, fileName, requestId,
                conversationId, timestamp, createdAt, model
            ) VALUES (?1, ?2, ?3, ?4, 'req', ?5, ?6, ?6, ?7)
            "#,
            rusqlite::params![
                format!("hash-{conversation_id}-{file_name}-{source}"),
                source,
                extension,
                file_name,
                conversation_id,
                timestamp,
                model
            ],
        )
        .expect("insert hash");
    }
}

pub fn seed_cursor_subagent(
    home: &std::path::Path,
    project_slug: &str,
    session_id: &str,
    child_id: &str,
    content: &str,
) -> std::path::PathBuf {
    let path = home
        .join(".cursor")
        .join("projects")
        .join(project_slug)
        .join("agent-transcripts")
        .join(session_id)
        .join("subagents")
        .join(format!("{child_id}.jsonl"));
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
    std::fs::write(&path, content).expect("write subagent");
    path
}
