use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate};
use rusqlite::{params, Connection};

use crate::domain::{
    EngineCommand, PriceTable, Source, WorkNotesDto, WorkNotesRange, WorkNotesRangeKind,
};
use crate::test_support::*;
use crate::work_notes::{self, EngineRunner, ScriptedRunner};

fn day(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
}

fn now_on(date: NaiveDate) -> DateTime<Local> {
    let naive = date.and_hms_opt(15, 0, 0).expect("valid time");
    naive
        .and_local_timezone(Local)
        .earliest()
        .or_else(|| naive.and_local_timezone(Local).latest())
        .expect("local time")
}

/// 固定「现在」为 2026-08-19（周三）：本周至今是 8/17 一 00:00 到此刻，不是上一个已结束周。
fn now() -> DateTime<Local> {
    now_on(day(2026, 8, 19))
}

fn map_json(summary: &str) -> String {
    serde_json::json!({ "summary": summary }).to_string()
}

fn reduce_json() -> String {
    serde_json::json!({
        "headline": "本周主线是修统计口径",
        "entries": [
            {"title": "压缩会话输入", "detail": "把长对话压到短摘要", "project": "statistics"},
            {"title": "只读调用 Codex", "detail": "用 ephemeral 跑总结", "project": "statistics"},
            {"title": "硬数字同屏", "detail": "会话数和 token 一起展示", "project": "statistics"}
        ],
        "closing": "下周继续补区间和引擎"
    })
    .to_string()
}

fn replies(items: &[&str]) -> ScriptedRunner {
    ScriptedRunner::succeeding(items.iter().copied())
}

struct Harness {
    conn: Connection,
    app_dir: tempfile::TempDir,
    runner: ScriptedRunner,
}

impl Harness {
    fn new(runner: ScriptedRunner) -> Self {
        Self {
            conn: store::open_memory().unwrap(),
            app_dir: tempfile::tempdir().unwrap(),
            runner,
        }
    }

    fn work_dir(&self) -> PathBuf {
        self.app_dir.path().join("work-notes-engine")
    }

    fn build(&self) -> WorkNotesDto {
        work_notes::build(
            &self.conn,
            &PriceTable::default(),
            WorkNotesRange::this_week(),
            now(),
            &self.runner,
            self.app_dir.path(),
        )
        .unwrap()
    }

    fn insert_session(&self, seed: SeedSession) {
        let started_at = local_time_iso(seed.started, seed.hour, 0, 0);
        let ended_at = local_time_iso(seed.started, seed.hour + 1, 0, 0);
        let source_file = format!("/tmp/{}.jsonl", seed.session_id);
        self.conn
            .execute(
                r#"
                INSERT INTO conversation_sessions(
                    source, session_id, title, project, model, started_at, ended_at,
                    source_file, capabilities_json, support_status, file_available,
                    is_top_level, event_index_generation
                ) VALUES('codex', ?1, ?2, ?3, '', ?4, ?5, ?6, '[]', 'ok', 1, 1, 1)
                "#,
                params![
                    seed.session_id,
                    seed.title,
                    seed.project,
                    started_at,
                    ended_at,
                    source_file
                ],
            )
            .unwrap();
        self.conn
            .execute(
                "INSERT OR IGNORE INTO conversation_files(path) VALUES(?1)",
                params![source_file],
            )
            .unwrap();
        let file_id: i64 = self
            .conn
            .query_row(
                "SELECT file_id FROM conversation_files WHERE path = ?1",
                params![source_file],
                |row| row.get(0),
            )
            .unwrap();
        for (sequence, event) in seed.events.iter().enumerate() {
            let event_id = format!("{}-{sequence}", seed.session_id);
            self.conn
                .execute(
                    r#"
                    INSERT INTO conversation_events(
                        source, session_id, event_id, sequence, file_id, source_sequence,
                        kind, actor, name, occurred_at, occurred_at_sort, text,
                        attachments_json, capability_status, content_status,
                        identity_hash, identity_occurrence, index_generation
                    ) VALUES(
                        'codex', ?1, ?2, ?3, ?4, ?3,
                        ?5, ?6, ?7, ?8, ?8, ?9,
                        '[]', 'complete', ?10,
                        ?2, 0, 1
                    )
                    "#,
                    params![
                        seed.session_id,
                        event_id,
                        sequence as i64,
                        file_id,
                        event.kind,
                        event.actor,
                        event.name,
                        started_at,
                        event.text,
                        event.content_status,
                    ],
                )
                .unwrap();
        }
        if seed.tokens > 0 {
            store::insert_records(
                &self.conn,
                &[rec(
                    &started_at,
                    Source::Codex,
                    "gpt-5.1-codex",
                    "official",
                    seed.project,
                    seed.session_id,
                    seed.tokens,
                )],
            )
            .unwrap();
        }
    }
}

struct SeedSession<'a> {
    session_id: &'a str,
    title: &'a str,
    project: &'a str,
    started: NaiveDate,
    hour: u32,
    tokens: i64,
    events: &'a [SeedEvent],
}

struct SeedEvent {
    kind: &'static str,
    actor: Option<&'static str>,
    name: Option<&'static str>,
    text: Option<String>,
    content_status: &'static str,
}

fn user_msg(text: impl Into<String>) -> SeedEvent {
    SeedEvent {
        kind: "message",
        actor: Some("user"),
        name: None,
        text: Some(text.into()),
        content_status: "complete",
    }
}

fn assistant_msg(text: impl Into<String>) -> SeedEvent {
    SeedEvent {
        kind: "message",
        actor: Some("assistant"),
        name: None,
        text: Some(text.into()),
        content_status: "complete",
    }
}

fn tool_call(name: &'static str) -> SeedEvent {
    SeedEvent {
        kind: "tool_call",
        actor: Some("tool"),
        name: Some(name),
        text: None,
        content_status: "complete",
    }
}

fn eligible_events() -> Vec<SeedEvent> {
    vec![
        user_msg("先把口径对齐"),
        user_msg("再把压缩规则写死"),
        assistant_msg("已经按 4 到 6k 压好了"),
        tool_call("Edit"),
        tool_call("Edit"),
        tool_call("Bash"),
    ]
}

fn seed_eligible(h: &Harness, session_id: &str, project: &str) {
    let events = eligible_events();
    h.insert_session(SeedSession {
        session_id,
        title: "对齐口径",
        project,
        started: day(2026, 8, 18),
        hour: 10,
        tokens: 2000,
        events: &events,
    });
}

fn assert_codex_switches(cmd: &EngineCommand, work_dir: &Path) {
    assert_eq!(cmd.program, "codex");
    assert_eq!(cmd.cwd, work_dir);
    assert!(
        cmd.args.windows(2).any(|pair| pair == ["-a", "never"]),
        "缺禁审批：{:?}",
        cmd.args
    );
    assert!(
        cmd.args.windows(2).any(|pair| pair == ["-s", "read-only"]),
        "缺只读沙箱：{:?}",
        cmd.args
    );
    assert!(
        cmd.args.iter().any(|arg| arg == "--ephemeral"),
        "缺 --ephemeral：{:?}",
        cmd.args
    );
    let schema_at = cmd
        .args
        .iter()
        .position(|arg| arg == "--output-schema")
        .expect("缺 --output-schema");
    let schema = Path::new(&cmd.args[schema_at + 1]);
    assert!(
        schema.starts_with(work_dir),
        "schema 不在专用工作目录：{schema:?} cwd={work_dir:?}"
    );
    assert_eq!(cmd.args.last().map(String::as_str), Some("-"));
}

#[test]
fn this_week_is_monday_to_now_not_last_completed_week() {
    let h = Harness::new(replies(&[]));
    let events = eligible_events();
    h.insert_session(SeedSession {
        session_id: "last-week",
        title: "上周的活",
        project: "/Users/me/old",
        started: day(2026, 8, 16),
        hour: 10,
        tokens: 2000,
        events: &events,
    });
    let dto = h.build();
    assert_eq!(dto.range_kind, WorkNotesRangeKind::ThisWeek);
    assert_eq!(dto.start_date, "2026-08-17");
    assert_eq!(dto.end_date, "2026-08-19");
    assert!(!dto.has_data);
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn empty_range_does_not_call_engine() {
    let h = Harness::new(replies(&[]));
    let dto = h.build();
    assert!(!dto.has_data);
    assert_eq!(dto.skipped_sparse, 0);
    assert!(dto.entries.is_empty());
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn sparse_sessions_are_skipped_and_counted() {
    let h = Harness::new(replies(&[]));
    let few_events = [user_msg("hi"), assistant_msg("ok")];
    h.insert_session(SeedSession {
        session_id: "few-events",
        title: "误触",
        project: "/Users/me/a",
        started: day(2026, 8, 18),
        hour: 10,
        tokens: 5000,
        events: &few_events,
    });
    let few_tokens = eligible_events();
    h.insert_session(SeedSession {
        session_id: "few-tokens",
        title: "一句话",
        project: "/Users/me/b",
        started: day(2026, 8, 18),
        hour: 11,
        tokens: 500,
        events: &few_tokens,
    });
    let dto = h.build();
    assert!(!dto.has_data);
    assert_eq!(dto.skipped_sparse, 2);
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn session_prompt_is_compressed_and_project_is_directory_name() {
    let h = Harness::new(replies(&[&map_json("摘要"), &reduce_json()]));
    let first = format!("HEAD{}TAIL", "X".repeat(1500));
    let last = format!("ASSIST-HEAD{}ASSIST-TAIL", "Y".repeat(1500));
    let mut events = vec![user_msg(first)];
    for i in 0..12 {
        events.push(user_msg(format!("MID{i:02}{}", "m".repeat(300))));
    }
    events.push(assistant_msg(last));
    events.push(SeedEvent {
        kind: "message",
        actor: Some("user"),
        name: None,
        text: Some("DEFERRED_SECRET".into()),
        content_status: "deferred",
    });
    events.extend([
        tool_call("Edit"),
        tool_call("Edit"),
        tool_call("Bash"),
        tool_call("Edit"),
    ]);
    h.insert_session(SeedSession {
        session_id: "long",
        title: "长会话",
        project: "/Users/someone/src/statistics",
        started: day(2026, 8, 18),
        hour: 10,
        tokens: 4000,
        events: &events,
    });
    let dto = h.build();
    assert!(dto.has_data);
    let commands = h.runner.recorded();
    let map_cmd = commands
        .iter()
        .find(|cmd| cmd.args.iter().any(|arg| arg.ends_with("map.schema.json")))
        .expect("map command");
    assert!(
        map_cmd.stdin.contains("项目：statistics"),
        "项目必须是目录名：{}",
        map_cmd.stdin
    );
    assert!(
        !map_cmd.stdin.contains("/Users/someone"),
        "不得发送绝对路径：{}",
        map_cmd.stdin
    );
    assert!(map_cmd.stdin.contains("HEAD"));
    assert!(!map_cmd.stdin.contains("TAIL"));
    assert!(map_cmd.stdin.contains("MID00"));
    assert!(map_cmd.stdin.contains("MID09"));
    assert!(!map_cmd.stdin.contains("MID10"));
    assert!(!map_cmd.stdin.contains("MID11"));
    assert!(map_cmd.stdin.contains("ASSIST-HEAD"));
    assert!(!map_cmd.stdin.contains("ASSIST-TAIL"));
    assert!(!map_cmd.stdin.contains("DEFERRED_SECRET"));
    assert!(
        map_cmd.stdin.contains("Edit×3") && map_cmd.stdin.contains("Bash×1"),
        "工具统计：{}",
        map_cmd.stdin
    );
    let chars = map_cmd.stdin.chars().count();
    assert!(chars <= 6000, "单会话必须压在 6k 字符内，实际 {chars}");
}

#[test]
fn codex_command_is_readonly_no_approval_ephemeral_in_dedicated_workdir() {
    let h = Harness::new(replies(&[&map_json("摘要"), &reduce_json()]));
    seed_eligible(&h, "ok", "/Users/me/src/statistics");
    let dto = h.build();
    assert!(dto.has_data);
    let work_dir = h.work_dir();
    let commands = h.runner.recorded();
    assert!(
        commands.len() >= 2,
        "至少一次 map、一次 reduce，实际 {}",
        commands.len()
    );
    for cmd in &commands {
        assert_codex_switches(cmd, &work_dir);
    }
    assert!(work_dir.join("map.schema.json").is_file());
    assert!(work_dir.join("reduce.schema.json").is_file());
}

#[test]
fn parses_schema_json() {
    let h = Harness::new(replies(&[&map_json("修口径"), &reduce_json()]));
    seed_eligible(&h, "ok", "/proj/statistics");
    let dto = h.build();
    assert_eq!(dto.headline, "本周主线是修统计口径");
    assert_eq!(dto.entries.len(), 3);
    assert_eq!(dto.entries[0].title, "压缩会话输入");
    assert_eq!(dto.closing, "下周继续补区间和引擎");
    assert_eq!(dto.session_count, 1);
    assert_eq!(dto.project_count, 1);
    assert_eq!(dto.active_days, 1);
    assert_eq!(dto.total_tokens, 2000);
    assert_eq!(dto.skipped_sparse, 0);
}

#[test]
fn parses_fenced_json_block() {
    let fenced = format!("好的，结果如下：\n```json\n{}\n```\n完", reduce_json());
    let h = Harness::new(replies(&[&map_json("修口径"), &fenced]));
    seed_eligible(&h, "ok", "/proj/statistics");
    let dto = h.build();
    assert_eq!(dto.headline, "本周主线是修统计口径");
    assert_eq!(dto.entries.len(), 3);
}

#[test]
fn retries_once_then_parses() {
    let h = Harness::new(replies(&[
        &map_json("修口径"),
        "这不是 JSON",
        &reduce_json(),
    ]));
    seed_eligible(&h, "ok", "/proj/statistics");
    let dto = h.build();
    assert_eq!(dto.headline, "本周主线是修统计口径");
    assert_eq!(h.runner.recorded().len(), 3);
}

#[test]
fn degrades_to_plain_text_after_retry_fails() {
    let h = Harness::new(replies(&[
        &map_json("修口径"),
        "完全无法解析的散文",
        "还是不行的散文 RAW_FALLBACK",
    ]));
    seed_eligible(&h, "ok", "/proj/statistics");
    let dto = h.build();
    assert!(dto.has_data);
    assert_eq!(dto.entries.len(), 1);
    assert!(
        dto.entries[0].detail.contains("RAW_FALLBACK"),
        "{:?}",
        dto.entries
    );
    assert_eq!(h.runner.recorded().len(), 3);
}

#[test]
fn skipped_sparse_still_counted_when_other_sessions_summarize() {
    let h = Harness::new(replies(&[&map_json("摘要"), &reduce_json()]));
    seed_eligible(&h, "ok", "/proj/statistics");
    let tiny = [user_msg("?"), assistant_msg(".")];
    h.insert_session(SeedSession {
        session_id: "tiny",
        title: "误触",
        project: "/proj/other",
        started: day(2026, 8, 18),
        hour: 12,
        tokens: 80,
        events: &tiny,
    });
    let dto = h.build();
    assert!(dto.has_data);
    assert_eq!(dto.skipped_sparse, 1);
}

/// `cargo test --manifest-path src-tauri/Cargo.toml work_notes_codex_spawn_smoke -- --ignored --nocapture`
#[test]
#[ignore = "需要本机已登录的 Codex CLI"]
fn work_notes_codex_spawn_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let work_dir = work_notes::ensure_work_dir(dir.path()).unwrap();
    work_notes::write_schemas(&work_dir).unwrap();
    let cmd = work_notes::codex_command(
        &work_dir,
        work_notes::SchemaKind::Map,
        "只输出 JSON：{\"summary\":\"ok\"}".into(),
    )
    .unwrap();
    let out = work_notes::ProcessRunner
        .run(&cmd)
        .expect("codex spawn 失败");
    assert!(!out.trim().is_empty(), "stdout 空：{out:?}");
}
