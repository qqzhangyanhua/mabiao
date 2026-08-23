use crate::aggregate::{work_timeline, work_timeline_with_spans};
use crate::domain::{CursorSessionRecord, Source, WorkSessionSpan};
use crate::test_support::{local_time_iso, rec};
use chrono::NaiveDate;

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 8, 15).expect("valid date")
}

fn day_str() -> &'static str {
    "2026-08-15"
}

#[test]
fn single_session_within_day_sums_tokens() {
    let records = vec![
        rec(
            &local_time_iso(day(), 10, 0, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            100,
        ),
        rec(
            &local_time_iso(day(), 10, 30, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            50,
        ),
    ];
    let dto = work_timeline(&records, day_str());
    assert_eq!(dto.day, day_str());
    assert_eq!(dto.segment_count, 1);
    assert_eq!(dto.total_tokens, 150);
    let segment = &dto.segments[0];
    assert_eq!(segment.session_id, "s1");
    assert_eq!(segment.project, "/proj/a");
    assert_eq!(segment.model, "gpt-5.1-codex");
    assert_eq!(segment.total_tokens, 150);
    assert!(segment.start <= segment.end);
}

#[test]
fn session_crossing_midnight_is_clipped_and_tokens_split_by_day() {
    let yesterday = day().pred_opt().expect("valid date");
    let records = vec![
        rec(
            &local_time_iso(yesterday, 23, 50, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/b",
            "s2",
            20,
        ),
        rec(
            &local_time_iso(day(), 0, 10, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/b",
            "s2",
            30,
        ),
    ];

    // 昨天视角：只统计落在昨天的那条记录。
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let dto_yesterday = work_timeline(&records, &yesterday_str);
    assert_eq!(dto_yesterday.segment_count, 1);
    assert_eq!(dto_yesterday.total_tokens, 20);
    assert_eq!(
        dto_yesterday.segments[0].end,
        local_time_iso(day(), 0, 0, 0)
    );

    // 今天视角：会话区间与今天有交集，片段从今天零点开始，只统计落在今天的那条记录。
    let dto_today = work_timeline(&records, day_str());
    assert_eq!(dto_today.segment_count, 1);
    assert_eq!(dto_today.total_tokens, 30);
    let segment = &dto_today.segments[0];
    assert_eq!(segment.start, local_time_iso(day(), 0, 0, 0));
    assert_eq!(segment.end, local_time_iso(day(), 0, 10, 0));
    assert_eq!(segment.total_tokens, 30);
}

#[test]
fn session_entirely_before_or_after_day_is_excluded() {
    let before = day() - chrono::Duration::days(2);
    let after = day() + chrono::Duration::days(2);
    let records = vec![
        rec(
            &local_time_iso(before, 10, 0, 0),
            Source::Pi,
            "gpt-5.5",
            "subapi",
            "/proj/c",
            "s3",
            10,
        ),
        rec(
            &local_time_iso(after, 10, 0, 0),
            Source::Pi,
            "gpt-5.5",
            "subapi",
            "/proj/c",
            "s4",
            10,
        ),
    ];
    let dto = work_timeline(&records, day_str());
    assert_eq!(dto.segment_count, 0);
    assert_eq!(dto.total_tokens, 0);
    assert!(dto.segments.is_empty());
}

#[test]
fn single_turn_session_yields_zero_width_segment() {
    let records = vec![rec(
        &local_time_iso(day(), 15, 0, 0),
        Source::Gemini,
        "gemini-pro",
        "google",
        "/proj/d",
        "s5",
        40,
    )];
    let dto = work_timeline(&records, day_str());
    assert_eq!(dto.segment_count, 1);
    let segment = &dto.segments[0];
    assert_eq!(segment.start, segment.end);
    assert_eq!(segment.total_tokens, 40);
}

#[test]
fn same_session_id_different_source_are_separate_segments() {
    let records = vec![
        rec(
            &local_time_iso(day(), 9, 0, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "dup",
            10,
        ),
        rec(
            &local_time_iso(day(), 9, 30, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/a",
            "dup",
            20,
        ),
    ];
    let dto = work_timeline(&records, day_str());
    assert_eq!(dto.segment_count, 2);
    assert_eq!(dto.total_tokens, 30);
}

#[test]
fn empty_records_yield_zero_summary() {
    let dto = work_timeline(&[], day_str());
    assert_eq!(dto.day, day_str());
    assert_eq!(dto.segment_count, 0);
    assert_eq!(dto.total_tokens, 0);
    assert!(dto.segments.is_empty());
}

#[test]
fn invalid_day_string_returns_empty_without_panicking() {
    let records = vec![rec(
        &local_time_iso(day(), 10, 0, 0),
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "s1",
        100,
    )];
    let dto = work_timeline(&records, "not-a-date");
    assert_eq!(dto.day, "not-a-date");
    assert_eq!(dto.segment_count, 0);
    assert!(dto.segments.is_empty());
}

#[test]
fn turn_count_aggregates_records_landing_in_day() {
    let yesterday = day().pred_opt().expect("valid date");
    let records = vec![
        rec(
            &local_time_iso(yesterday, 23, 50, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            10,
        ),
        rec(
            &local_time_iso(day(), 9, 0, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            20,
        ),
        rec(
            &local_time_iso(day(), 10, 0, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/b",
            "s2",
            30,
        ),
    ];
    let dto = work_timeline(&records, day_str());
    // 两条落在今天，昨天那条不计入。
    assert_eq!(dto.turn_count, 2);
}

#[test]
fn peak_parallel_counts_three_way_overlap() {
    let records = vec![
        rec(
            &local_time_iso(day(), 9, 0, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            10,
        ),
        rec(
            &local_time_iso(day(), 11, 0, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            10,
        ),
        rec(
            &local_time_iso(day(), 9, 30, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/b",
            "s2",
            20,
        ),
        rec(
            &local_time_iso(day(), 10, 30, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/b",
            "s2",
            20,
        ),
        rec(
            &local_time_iso(day(), 10, 0, 0),
            Source::Pi,
            "gpt-5.5",
            "subapi",
            "/proj/c",
            "s3",
            30,
        ),
        rec(
            &local_time_iso(day(), 10, 15, 0),
            Source::Pi,
            "gpt-5.5",
            "subapi",
            "/proj/c",
            "s3",
            30,
        ),
    ];
    let dto = work_timeline(&records, day_str());
    // s1: 09:00-11:00, s2: 09:30-10:30, s3: 10:00-10:15 → 10:00 时三段同时进行。
    assert_eq!(dto.peak_parallel, 3);
}

#[test]
fn parallel_intensity_is_one_when_no_overlap() {
    let records = vec![
        rec(
            &local_time_iso(day(), 9, 0, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            10,
        ),
        rec(
            &local_time_iso(day(), 10, 0, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            10,
        ),
        rec(
            &local_time_iso(day(), 11, 0, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/b",
            "s2",
            20,
        ),
        rec(
            &local_time_iso(day(), 12, 0, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/b",
            "s2",
            20,
        ),
    ];
    let dto = work_timeline(&records, day_str());
    // s1: 09:00-10:00, s2: 11:00-12:00，无重叠 → 1.0x。
    assert_eq!(dto.parallel_intensity, Some(1.0));
}

#[test]
fn parallel_intensity_exceeds_one_when_fully_overlapping() {
    let records = vec![
        rec(
            &local_time_iso(day(), 9, 0, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            10,
        ),
        rec(
            &local_time_iso(day(), 10, 0, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            10,
        ),
        rec(
            &local_time_iso(day(), 9, 0, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/b",
            "s2",
            20,
        ),
        rec(
            &local_time_iso(day(), 10, 0, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/b",
            "s2",
            20,
        ),
    ];
    let dto = work_timeline(&records, day_str());
    // 两段完全重叠 09:00-10:00，累计 2h / 并集 1h = 2.0x。
    assert_eq!(dto.parallel_intensity, Some(2.0));
    assert_eq!(dto.peak_parallel, 2);
}

#[test]
fn ai_exec_minutes_sums_clipped_durations() {
    let records = vec![
        rec(
            &local_time_iso(day(), 9, 0, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            10,
        ),
        rec(
            &local_time_iso(day(), 9, 30, 0),
            Source::Codex,
            "gpt-5.1-codex",
            "official",
            "/proj/a",
            "s1",
            10,
        ),
    ];
    let dto = work_timeline(&records, day_str());
    // 会话区间 09:00-09:30 = 30 分钟，单段无重叠 → 1.0x。
    assert_eq!(dto.ai_exec_minutes, 30.0);
    assert_eq!(dto.parallel_intensity, Some(1.0));
}

#[test]
fn zero_width_session_has_zero_ai_exec_minutes() {
    let records = vec![rec(
        &local_time_iso(day(), 15, 0, 0),
        Source::Gemini,
        "gemini-pro",
        "google",
        "/proj/d",
        "s5",
        40,
    )];
    let dto = work_timeline(&records, day_str());
    // 单条记录会话区间为 0 宽度，裁剪后仍为 0 分钟，并集时长为 0 → null 强度。
    assert_eq!(dto.ai_exec_minutes, 0.0);
    assert_eq!(dto.parallel_intensity, None);
}

#[test]
fn empty_day_yields_null_intensity_and_zero_peak() {
    let dto = work_timeline(&[], day_str());
    assert_eq!(dto.turn_count, 0);
    assert_eq!(dto.ai_exec_minutes, 0.0);
    assert_eq!(dto.peak_parallel, 0);
    assert_eq!(dto.parallel_intensity, None);
}

#[test]
fn cross_midnight_session_clips_interval_for_metrics() {
    let yesterday = day().pred_opt().expect("valid date");
    let records = vec![
        rec(
            &local_time_iso(yesterday, 23, 0, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/b",
            "s2",
            20,
        ),
        rec(
            &local_time_iso(day(), 1, 0, 0),
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj/b",
            "s2",
            30,
        ),
    ];
    let dto = work_timeline(&records, day_str());
    // 会话区间 23:00(昨天) ~ 01:00(今天)，裁剪到今天后为 00:00 ~ 01:00 = 60 分钟。
    assert_eq!(dto.segment_count, 1);
    assert_eq!(dto.ai_exec_minutes, 60.0);
    assert_eq!(dto.peak_parallel, 1);
    assert_eq!(dto.parallel_intensity, Some(1.0));
    // turn_count 只算落在今天的记录（1 条）。
    assert_eq!(dto.turn_count, 1);
    assert_eq!(dto.total_tokens, 30);
}

fn cursor_span(
    session_id: &str,
    project: &str,
    model: &str,
    start: &str,
    end: &str,
) -> WorkSessionSpan {
    WorkSessionSpan {
        source: Source::CursorAgent.as_str().to_string(),
        session_id: session_id.to_string(),
        project: project.to_string(),
        model: model.to_string(),
        started_at: start.to_string(),
        ended_at: end.to_string(),
    }
}

fn cursor_session_row(
    source_file: &str,
    session_id: &str,
    project: &str,
    models_json: &str,
    first_seen_at: &str,
    last_seen_at: &str,
) -> CursorSessionRecord {
    CursorSessionRecord {
        session_id: session_id.to_string(),
        project: project.to_string(),
        turn_count: 4,
        success_count: 3,
        error_count: 1,
        aborted_count: 0,
        user_prompt_count: 2,
        subagent_count: 0,
        tool_calls_json: "{}".into(),
        models_json: models_json.to_string(),
        sources_json: "[]".into(),
        extensions_json: "{}".into(),
        first_seen_at: Some(first_seen_at.to_string()),
        last_seen_at: Some(last_seen_at.to_string()),
        files_touched: 1,
        source_file: source_file.to_string(),
    }
}

#[test]
fn cursor_session_span_becomes_a_segment_without_tokens() {
    let extra = [cursor_span(
        "cur-1",
        "/proj/cursor",
        "grok-4.6",
        &local_time_iso(day(), 9, 0, 0),
        &local_time_iso(day(), 10, 0, 0),
    )];
    let dto = work_timeline_with_spans(&[], &extra, day_str());
    assert_eq!(dto.segment_count, 1);
    assert_eq!(dto.total_tokens, 0);
    assert_eq!(dto.turn_count, 0);
    assert_eq!(dto.ai_exec_minutes, 60.0);
    let segment = &dto.segments[0];
    assert_eq!(segment.session_id, "cur-1");
    assert_eq!(segment.source, "cursor_agent");
    assert_eq!(segment.project, "/proj/cursor");
    assert_eq!(segment.model, "grok-4.6");
    assert_eq!(segment.total_tokens, 0);
}

#[test]
fn cursor_span_merges_with_usage_records_of_the_same_session() {
    let records = vec![rec(
        &local_time_iso(day(), 9, 30, 0),
        Source::CursorAgent,
        "composer",
        "",
        "/proj/a",
        "cur-1",
        80,
    )];
    let extra = [cursor_span(
        "cur-1",
        "/proj/cursor",
        "grok-4.6",
        &local_time_iso(day(), 9, 0, 0),
        &local_time_iso(day(), 10, 0, 0),
    )];
    let dto = work_timeline_with_spans(&records, &extra, day_str());
    assert_eq!(dto.segment_count, 1);
    assert_eq!(dto.total_tokens, 80);
    assert_eq!(dto.turn_count, 1);
    assert_eq!(dto.ai_exec_minutes, 60.0);
    let segment = &dto.segments[0];
    assert_eq!(segment.session_id, "cur-1");
    assert_eq!(segment.total_tokens, 80);
    // 记录 09:30 比会话结束 10:00 早，模型/项目取较新的会话标签。
    assert_eq!(segment.model, "grok-4.6");
    assert_eq!(segment.project, "/proj/cursor");
}

#[test]
fn query_work_timeline_includes_cursor_sessions() {
    let conn = crate::store::open_memory().unwrap();
    crate::store::upsert_cursor_session(
        &conn,
        &cursor_session_row(
            "/tmp/cur-1.jsonl",
            "cur-1",
            "/proj/cursor",
            r#"["composer","grok-4.6"]"#,
            &local_time_iso(day(), 14, 0, 0),
            &local_time_iso(day(), 15, 30, 0),
        ),
    )
    .unwrap();
    let dto = crate::query::work_timeline(&conn, day_str()).unwrap();
    assert_eq!(dto.segment_count, 1);
    assert_eq!(dto.total_tokens, 0);
    assert_eq!(dto.turn_count, 0);
    assert_eq!(dto.ai_exec_minutes, 90.0);
    let segment = &dto.segments[0];
    assert_eq!(segment.source, "cursor_agent");
    assert_eq!(segment.session_id, "cur-1");
    assert_eq!(segment.model, "grok-4.6");
    assert_eq!(segment.project, "/proj/cursor");
}
