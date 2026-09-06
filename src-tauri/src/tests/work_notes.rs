use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate};
use rusqlite::{params, Connection};

use crate::conversation;
use crate::domain::{
    ConversationQuery, EngineCommand, PriceEntry, PriceOrigin, PriceTable, Source, WorkNotesDto,
    WorkNotesGate, WorkNotesJobStatus, WorkNotesPreviewDto, WorkNotesRange, WorkNotesRangeKind,
};
use crate::test_support::*;
use crate::work_notes::{self, EngineRunner, ScriptedRunner, WorkNotesJob};

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

fn replies(maps: &[&str], reduce: &[&str]) -> ScriptedRunner {
    ScriptedRunner::succeeding(maps.iter().copied(), reduce.iter().copied())
}

struct Harness {
    conn: Connection,
    app_dir: tempfile::TempDir,
    runner: ScriptedRunner,
    prices: PriceTable,
    job: WorkNotesJob,
}

impl Harness {
    fn new(runner: ScriptedRunner) -> Self {
        Self {
            conn: store::open_memory().unwrap(),
            app_dir: tempfile::tempdir().unwrap(),
            runner,
            prices: PriceTable::default(),
            job: WorkNotesJob::new(),
        }
    }

    fn with_prices(mut self, prices: PriceTable) -> Self {
        self.prices = prices;
        self
    }

    fn work_dir(&self) -> PathBuf {
        self.app_dir.path().join("work-notes-engine")
    }

    fn build(&self) -> WorkNotesDto {
        self.try_build(WorkNotesRange::this_week(), false).unwrap()
    }

    fn try_build(&self, range: WorkNotesRange, confirmed: bool) -> Result<WorkNotesDto, String> {
        self.try_build_with(range, confirmed, "codex", None)
    }

    fn try_build_with(
        &self,
        range: WorkNotesRange,
        confirmed: bool,
        engine_id: &str,
        model: Option<&str>,
    ) -> Result<WorkNotesDto, String> {
        work_notes::build(
            &self.conn,
            &self.prices,
            range,
            now(),
            &self.runner,
            self.app_dir.path(),
            engine_id,
            model,
            confirmed,
            &self.job,
        )
    }

    fn preview(&self, range: WorkNotesRange) -> Result<WorkNotesPreviewDto, String> {
        work_notes::preview(&self.conn, &self.prices, range, now(), None, None)
    }

    fn insert_session(&self, seed: SeedSession) {
        self.insert_named("codex", Source::Codex, seed);
    }

    fn insert_named(&self, source: &str, usage: Source, seed: SeedSession) {
        let started_at = local_time_iso(seed.started, seed.hour, 0, 0);
        let ended_at = local_time_iso(seed.started, seed.hour + 1, 0, 0);
        let source_file = format!("/tmp/{source}-{}.jsonl", seed.session_id);
        self.conn
            .execute(
                r#"
                INSERT INTO conversation_sessions(
                    source, session_id, title, project, model, started_at, ended_at,
                    source_file, capabilities_json, support_status, file_available,
                    is_top_level, event_index_generation
                ) VALUES(?1, ?2, ?3, ?4, '', ?5, ?6, ?7, '[]', 'ok', 1, 1, 1)
                "#,
                params![
                    source,
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
                        ?1, ?2, ?3, ?4, ?5, ?4,
                        ?6, ?7, ?8, ?9, ?9, ?10,
                        '[]', 'complete', ?11,
                        ?3, 0, 1
                    )
                    "#,
                    params![
                        source,
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
                    usage,
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
    seed_eligible_at(h, session_id, "对齐口径", project, day(2026, 8, 18), 10);
}

fn seed_eligible_at(
    h: &Harness,
    session_id: &str,
    title: &str,
    project: &str,
    started: NaiveDate,
    hour: u32,
) {
    let events = eligible_events();
    h.insert_session(SeedSession {
        session_id,
        title,
        project,
        started,
        hour,
        tokens: 2000,
        events: &events,
    });
}

fn seed_eligible_n(h: &Harness, n: usize) {
    let events = eligible_events();
    for index in 0..n {
        let session_id = format!("bulk-{index}");
        h.insert_session(SeedSession {
            session_id: &session_id,
            title: "对齐口径",
            project: "/proj/statistics",
            started: day(2026, 8, 18),
            hour: 10,
            tokens: 2000,
            events: &events,
        });
    }
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
    assert!(
        cmd.args.iter().any(|arg| arg == "--json"),
        "缺 --json：{:?}",
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

fn assert_claude_switches(cmd: &EngineCommand, work_dir: &Path) {
    assert_eq!(cmd.program, "claude");
    assert_eq!(cmd.cwd, work_dir);
    assert!(
        cmd.args.iter().any(|arg| arg == "-p"),
        "缺 -p：{:?}",
        cmd.args
    );
    assert!(
        cmd.args.iter().any(|arg| arg == "--no-session-persistence"),
        "缺 --no-session-persistence：{:?}",
        cmd.args
    );
    let tools_at = cmd
        .args
        .iter()
        .position(|arg| arg == "--tools")
        .expect("缺 --tools");
    assert_eq!(
        cmd.args.get(tools_at + 1).map(String::as_str),
        Some(""),
        "禁工具必须是空字符串：{:?}",
        cmd.args
    );
    let schema_at = cmd
        .args
        .iter()
        .position(|arg| arg == "--json-schema")
        .expect("缺 --json-schema");
    let schema = cmd.args.get(schema_at + 1).expect("缺 json-schema 内容");
    assert!(
        schema.contains("\"type\""),
        "json-schema 应是 JSON：{schema}"
    );
}

fn looks_like_uuid(value: &str) -> bool {
    let parts: Vec<_> = value.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
        && value.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-')
}

fn pinned_session_id(cmd: &EngineCommand) -> Option<&str> {
    let at = cmd
        .args
        .iter()
        .position(|arg| arg == "-s" || arg == "--session-id")?;
    cmd.args.get(at + 1).map(String::as_str)
}

fn prompt_of(cmd: &EngineCommand) -> String {
    if !cmd.stdin.is_empty() {
        return cmd.stdin.clone();
    }
    if let Some(at) = cmd
        .args
        .iter()
        .position(|arg| arg == "-p" || arg == "--single")
    {
        if let Some(next) = cmd.args.get(at + 1) {
            if !next.starts_with('-') {
                return next.clone();
            }
        }
    }
    cmd.args
        .iter()
        .rev()
        .find(|arg| arg.contains("请用一句中文概括") || arg.contains("下面是一段时间内"))
        .cloned()
        .unwrap_or_default()
}

fn is_map_command(cmd: &EngineCommand) -> bool {
    cmd.args.iter().any(|arg| arg.ends_with("map.schema.json"))
        || prompt_of(cmd).contains("请用一句中文概括")
}

fn assert_grok_switches(cmd: &EngineCommand, work_dir: &Path) {
    assert_eq!(cmd.program, "grok");
    assert_eq!(cmd.cwd, work_dir);
    assert!(
        cmd.args.iter().any(|arg| arg == "-p"),
        "缺 -p：{:?}",
        cmd.args
    );
    let schema_at = cmd
        .args
        .iter()
        .position(|arg| arg == "--json-schema")
        .expect("缺 --json-schema");
    let schema = cmd.args.get(schema_at + 1).expect("缺 json-schema 内容");
    assert!(
        schema.contains("\"type\""),
        "json-schema 应是 JSON：{schema}"
    );
    let denied_at = cmd
        .args
        .iter()
        .position(|arg| arg == "--disallowed-tools")
        .expect("缺 --disallowed-tools");
    let denied = cmd
        .args
        .get(denied_at + 1)
        .expect("缺 disallowed-tools 内容");
    assert!(
        !denied.is_empty(),
        "--disallowed-tools 不能为空：{:?}",
        cmd.args
    );
    let session_id = pinned_session_id(cmd).expect("grok 必须钉 session id");
    assert!(
        looks_like_uuid(session_id),
        "session id 必须是 UUID：{session_id}"
    );
}

fn assert_cursor_agent_switches(cmd: &EngineCommand, work_dir: &Path) {
    assert_eq!(cmd.program, "cursor-agent");
    assert_eq!(cmd.cwd, work_dir);
    assert!(
        cmd.args.iter().any(|arg| arg == "-p"),
        "缺 -p：{:?}",
        cmd.args
    );
    assert!(
        cmd.args.windows(2).any(|pair| pair == ["--mode", "ask"]),
        "缺 --mode ask：{:?}",
        cmd.args
    );
    assert!(
        !cmd.args
            .iter()
            .any(|arg| arg == "--json-schema" || arg == "--output-schema"),
        "cursor-agent 没有结构化输出开关：{:?}",
        cmd.args
    );
    assert!(
        pinned_session_id(cmd).is_none(),
        "cursor-agent 不得靠 session id 判定：{:?}",
        cmd.args
    );
    let prompt = prompt_of(cmd);
    assert!(
        prompt.contains("只输出 JSON"),
        "无 schema 时必须靠 prompt 约束：{prompt}"
    );
}

#[test]
fn this_week_is_monday_to_now_not_last_completed_week() {
    let h = Harness::new(replies(&[], &[]));
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
    let h = Harness::new(replies(&[], &[]));
    let dto = h.build();
    assert!(!dto.has_data);
    assert_eq!(dto.skipped_sparse, 0);
    assert!(dto.entries.is_empty());
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn sparse_sessions_are_skipped_and_counted() {
    let h = Harness::new(replies(&[], &[]));
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
    let h = Harness::new(replies(&[&map_json("摘要")], &[&reduce_json()]));
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
    let h = Harness::new(replies(&[&map_json("摘要")], &[&reduce_json()]));
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
fn claude_command_is_print_no_persist_no_tools_json_schema() {
    let h = Harness::new(replies(&[&map_json("摘要")], &[&reduce_json()]));
    seed_eligible(&h, "ok", "/Users/me/src/statistics");
    let dto = h
        .try_build_with(WorkNotesRange::this_week(), false, "claude", None)
        .unwrap();
    assert!(dto.has_data);
    let work_dir = h.work_dir();
    let commands = h.runner.recorded();
    assert!(
        commands.len() >= 2,
        "至少一次 map、一次 reduce，实际 {}",
        commands.len()
    );
    for cmd in &commands {
        assert_claude_switches(cmd, &work_dir);
        assert!(
            !cmd.args.iter().any(|arg| arg == "--model"),
            "未指定模型时不应带 --model：{:?}",
            cmd.args
        );
    }
}

#[test]
fn switching_engine_uses_the_matching_profile() {
    let h = Harness::new(replies(
        &[&map_json("摘要"), &map_json("摘要")],
        &[&reduce_json(), &reduce_json()],
    ));
    seed_eligible(&h, "ok", "/proj/statistics");
    let first = h
        .try_build_with(WorkNotesRange::this_week(), false, "codex", None)
        .unwrap();
    h.job.finish(Ok(first)).unwrap();
    h.try_build_with(WorkNotesRange::this_week(), false, "claude", Some("sonnet"))
        .unwrap();
    let commands = h.runner.recorded();
    assert_eq!(commands.len(), 4, "{commands:?}");
    let work_dir = h.work_dir();
    for cmd in &commands[..2] {
        assert_codex_switches(cmd, &work_dir);
    }
    for cmd in &commands[2..] {
        assert_claude_switches(cmd, &work_dir);
        assert!(
            cmd.args
                .windows(2)
                .any(|pair| pair == ["--model", "sonnet"]),
            "换引擎后应带上指定模型：{:?}",
            cmd.args
        );
    }
}

#[test]
fn unknown_engine_is_rejected_before_spawn() {
    let h = Harness::new(replies(&[], &[]));
    seed_eligible(&h, "ok", "/proj/statistics");
    let error = h
        .try_build_with(WorkNotesRange::this_week(), false, "nope", None)
        .unwrap_err();
    assert!(error.contains("未知"), "{error}");
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn detect_marks_missing_cli_uninstalled() {
    let rows = work_notes::detect_with(
        |name| (name == "claude").then(|| PathBuf::from("/bin/claude")),
        |path| Ok(format!("ver-{}", path.display())),
    );
    assert_eq!(rows.len(), 4);
    let claude = rows.iter().find(|row| row.id == "claude").unwrap();
    assert!(claude.installed);
    assert_eq!(claude.version.as_deref(), Some("ver-/bin/claude"));
    assert!(!claude.writes_session_dir);
    let codex = rows.iter().find(|row| row.id == "codex").unwrap();
    assert!(!codex.installed);
    assert!(codex.version.is_none());
    assert!(!codex.writes_session_dir);
    let grok = rows.iter().find(|row| row.id == "grok").unwrap();
    assert!(!grok.installed);
    assert!(grok.writes_session_dir);
    let cursor = rows.iter().find(|row| row.id == "cursor-agent").unwrap();
    assert!(!cursor.installed);
    assert!(cursor.writes_session_dir);
}

#[test]
fn parses_schema_json() {
    let h = Harness::new(replies(&[&map_json("修口径")], &[&reduce_json()]));
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
    let h = Harness::new(replies(&[&map_json("修口径")], &[&fenced]));
    seed_eligible(&h, "ok", "/proj/statistics");
    let dto = h.build();
    assert_eq!(dto.headline, "本周主线是修统计口径");
    assert_eq!(dto.entries.len(), 3);
}

#[test]
fn retries_once_then_parses() {
    let h = Harness::new(replies(
        &[&map_json("修口径")],
        &["这不是 JSON", &reduce_json()],
    ));
    seed_eligible(&h, "ok", "/proj/statistics");
    let dto = h.build();
    assert_eq!(dto.headline, "本周主线是修统计口径");
    assert_eq!(h.runner.recorded().len(), 3);
}

#[test]
fn degrades_to_plain_text_after_retry_fails() {
    let h = Harness::new(replies(
        &[&map_json("修口径")],
        &["完全无法解析的散文", "还是不行的散文 RAW_FALLBACK"],
    ));
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
    let h = Harness::new(replies(&[&map_json("摘要")], &[&reduce_json()]));
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

fn map_stdin(h: &Harness) -> String {
    h.runner
        .recorded()
        .into_iter()
        .find(|cmd| cmd.args.iter().any(|arg| arg.ends_with("map.schema.json")))
        .map(|cmd| cmd.stdin)
        .unwrap_or_default()
}

#[test]
fn this_month_is_first_to_now_not_last_completed_month() {
    let h = Harness::new(replies(&[&map_json("摘要")], &[&reduce_json()]));
    seed_eligible_at(
        &h,
        "last-month",
        "上月的活",
        "/proj/old",
        day(2026, 7, 31),
        10,
    );
    seed_eligible_at(
        &h,
        "in-month",
        "本月的活",
        "/proj/statistics",
        day(2026, 8, 1),
        10,
    );
    seed_eligible_at(
        &h,
        "tonight",
        "今晚的活",
        "/proj/statistics",
        day(2026, 8, 19),
        16,
    );
    let dto = h.try_build(WorkNotesRange::this_month(), false).unwrap();
    assert_eq!(dto.range_kind, WorkNotesRangeKind::ThisMonth);
    assert_eq!(dto.start_date, "2026-08-01");
    assert_eq!(dto.end_date, "2026-08-19");
    assert!(dto.has_data);
    let stdin = map_stdin(&h);
    assert!(stdin.contains("本月的活"), "{stdin}");
    assert!(!stdin.contains("上月的活"), "{stdin}");
    assert!(!stdin.contains("今晚的活"), "{stdin}");
}

#[test]
fn custom_range_is_closed_local_days() {
    let h = Harness::new(replies(
        &[&map_json("摘要一"), &map_json("摘要二")],
        &[&reduce_json()],
    ));
    seed_eligible_at(&h, "before", "区间前", "/proj/a", day(2026, 8, 9), 10);
    seed_eligible_at(&h, "start", "起始日", "/proj/a", day(2026, 8, 10), 10);
    seed_eligible_at(&h, "end", "结束日", "/proj/a", day(2026, 8, 12), 22);
    seed_eligible_at(&h, "after", "区间后", "/proj/a", day(2026, 8, 13), 10);
    let dto = h
        .try_build(WorkNotesRange::custom("2026-08-10", "2026-08-12"), false)
        .unwrap();
    assert_eq!(dto.range_kind, WorkNotesRangeKind::Custom);
    assert_eq!(dto.start_date, "2026-08-10");
    assert_eq!(dto.end_date, "2026-08-12");
    assert!(dto.has_data);
    let commands = h.runner.recorded();
    let maps = commands
        .iter()
        .filter(|cmd| cmd.args.iter().any(|arg| arg.ends_with("map.schema.json")))
        .count();
    assert_eq!(maps, 2);
    let stdin = commands
        .iter()
        .filter(|cmd| cmd.args.iter().any(|arg| arg.ends_with("map.schema.json")))
        .map(|cmd| cmd.stdin.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(stdin.contains("起始日"), "{stdin}");
    assert!(stdin.contains("结束日"), "{stdin}");
    assert!(!stdin.contains("区间前"), "{stdin}");
    assert!(!stdin.contains("区间后"), "{stdin}");
}

#[test]
fn custom_range_ending_today_stops_at_now() {
    let h = Harness::new(replies(&[&map_json("摘要")], &[&reduce_json()]));
    seed_eligible_at(
        &h,
        "this-afternoon",
        "此刻之前",
        "/proj/a",
        day(2026, 8, 19),
        10,
    );
    seed_eligible_at(&h, "tonight", "此刻之后", "/proj/a", day(2026, 8, 19), 16);
    let dto = h
        .try_build(WorkNotesRange::custom("2026-08-19", "2026-08-19"), false)
        .unwrap();
    assert_eq!(dto.start_date, "2026-08-19");
    assert_eq!(dto.end_date, "2026-08-19");
    assert!(dto.has_data);
    let stdin = map_stdin(&h);
    assert!(stdin.contains("此刻之前"), "{stdin}");
    assert!(!stdin.contains("此刻之后"), "{stdin}");
}

#[test]
fn custom_range_allows_31_days() {
    let h = Harness::new(replies(&[], &[]));
    let dto = h
        .try_build(WorkNotesRange::custom("2026-07-20", "2026-08-19"), false)
        .unwrap();
    assert_eq!(dto.start_date, "2026-07-20");
    assert_eq!(dto.end_date, "2026-08-19");
    assert!(!dto.has_data);
}

#[test]
fn custom_range_rejects_more_than_31_days() {
    let h = Harness::new(replies(&[], &[]));
    let error = h
        .try_build(WorkNotesRange::custom("2026-07-19", "2026-08-19"), false)
        .unwrap_err();
    assert!(error.contains("31"), "{error}");
    assert!(error.contains("收窄"), "{error}");
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn custom_range_rejects_future_dates() {
    let h = Harness::new(replies(&[], &[]));
    let error = h
        .try_build(WorkNotesRange::custom("2026-08-19", "2026-08-20"), false)
        .unwrap_err();
    assert!(error.contains("今天"), "{error}");
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn preview_counts_sessions_without_calling_engine() {
    let h = Harness::new(replies(&[], &[]));
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
    let preview = h.preview(WorkNotesRange::this_week()).unwrap();
    assert_eq!(preview.range_kind, WorkNotesRangeKind::ThisWeek);
    assert_eq!(preview.start_date, "2026-08-17");
    assert_eq!(preview.end_date, "2026-08-19");
    assert_eq!(preview.session_count, 1);
    assert_eq!(preview.skipped_sparse, 1);
    assert_eq!(preview.gate, WorkNotesGate::Ok);
    assert!(preview.message.is_empty());
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn sixty_eligible_sessions_do_not_require_confirmation() {
    let h = Harness::new(replies(&[], &[]));
    seed_eligible_n(&h, 60);
    let preview = h.preview(WorkNotesRange::this_week()).unwrap();
    assert_eq!(preview.session_count, 60);
    assert_eq!(preview.gate, WorkNotesGate::Ok);
}

#[test]
fn more_than_60_eligible_sessions_require_confirmation() {
    let h = Harness::new(replies(&[], &[]));
    seed_eligible_n(&h, 61);
    let preview = h.preview(WorkNotesRange::this_week()).unwrap();
    assert_eq!(preview.session_count, 61);
    assert_eq!(preview.gate, WorkNotesGate::Confirm);
    assert!(preview.message.contains("60"), "{}", preview.message);
    let error = h.try_build(WorkNotesRange::this_week(), false).unwrap_err();
    assert!(error.contains("60"), "{error}");
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn more_than_60_eligible_sessions_run_after_confirmation() {
    let h = Harness::new(replies(&[&map_json("摘要")], &[]));
    seed_eligible_n(&h, 61);
    let error = h.try_build(WorkNotesRange::this_week(), true).unwrap_err();
    assert!(
        !h.runner.recorded().is_empty(),
        "确认后应开跑，实际未调用引擎：{error}"
    );
}

#[test]
fn more_than_150_eligible_sessions_are_rejected() {
    let h = Harness::new(replies(&[], &[]));
    seed_eligible_n(&h, 151);
    let preview = h.preview(WorkNotesRange::this_week()).unwrap();
    assert_eq!(preview.session_count, 151);
    assert_eq!(preview.gate, WorkNotesGate::Rejected);
    assert!(preview.message.contains("150"), "{}", preview.message);
    assert!(preview.message.contains("收窄"), "{}", preview.message);
    let error = h.try_build(WorkNotesRange::this_week(), true).unwrap_err();
    assert!(error.contains("150"), "{error}");
    assert!(error.contains("收窄"), "{error}");
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn sparse_sessions_do_not_count_toward_the_gate() {
    let h = Harness::new(replies(&[], &[]));
    let few = [user_msg("hi"), assistant_msg("ok")];
    for index in 0..61 {
        let session_id = format!("sparse-{index}");
        h.insert_session(SeedSession {
            session_id: &session_id,
            title: "误触",
            project: "/proj/a",
            started: day(2026, 8, 18),
            hour: 10,
            tokens: 500,
            events: &few,
        });
    }
    let preview = h.preview(WorkNotesRange::this_week()).unwrap();
    assert_eq!(preview.session_count, 0);
    assert_eq!(preview.skipped_sparse, 61);
    assert_eq!(preview.gate, WorkNotesGate::Ok);
    let dto = h.build();
    assert!(!dto.has_data);
    assert_eq!(dto.skipped_sparse, 61);
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn grok_command_pins_session_id_and_uses_print_json_schema_disallowed_tools() {
    let h = Harness::new(replies(&[&map_json("摘要")], &[&reduce_json()]));
    seed_eligible(&h, "ok", "/Users/me/src/statistics");
    let dto = h
        .try_build_with(WorkNotesRange::this_week(), false, "grok", None)
        .unwrap();
    assert!(dto.has_data);
    let work_dir = h.work_dir();
    let commands = h.runner.recorded();
    assert!(
        commands.len() >= 2,
        "至少一次 map、一次 reduce，实际 {}",
        commands.len()
    );
    let mut session_ids = Vec::new();
    for cmd in &commands {
        assert_grok_switches(cmd, &work_dir);
        session_ids.push(pinned_session_id(cmd).unwrap().to_string());
    }
    session_ids.sort();
    session_ids.dedup();
    assert_eq!(
        session_ids.len(),
        commands.len(),
        "每次 grok 调用必须钉不同的新 session id：{session_ids:?}"
    );
}

#[test]
fn cursor_agent_command_is_print_ask_mode_in_dedicated_workdir_without_schema() {
    let h = Harness::new(replies(&[&map_json("摘要")], &[&reduce_json()]));
    seed_eligible(&h, "ok", "/Users/me/src/statistics");
    let dto = h
        .try_build_with(WorkNotesRange::this_week(), false, "cursor-agent", None)
        .unwrap();
    assert!(dto.has_data);
    let work_dir = h.work_dir();
    let commands = h.runner.recorded();
    assert!(
        commands.len() >= 2,
        "至少一次 map、一次 reduce，实际 {}",
        commands.len()
    );
    for cmd in &commands {
        assert_cursor_agent_switches(cmd, &work_dir);
    }
}

#[test]
fn grok_generated_session_is_excluded_from_later_input_but_tokens_stay_in_kpi() {
    let h = Harness::new(replies(
        &[&map_json("摘要"), &map_json("摘要")],
        &[&reduce_json(), &reduce_json()],
    ));
    seed_eligible(&h, "ok", "/proj/statistics");
    let first = h
        .try_build_with(WorkNotesRange::this_week(), false, "grok", None)
        .unwrap();
    h.job.finish(Ok(first)).unwrap();
    let generated_id = pinned_session_id(&h.runner.recorded()[0])
        .expect("grok 应钉 session id")
        .to_string();
    let events = eligible_events();
    h.insert_named(
        "grok",
        Source::Grok,
        SeedSession {
            session_id: &generated_id,
            title: "码表自己生成的",
            project: h.work_dir().to_str().unwrap(),
            started: day(2026, 8, 18),
            hour: 10,
            tokens: 3000,
            events: &events,
        },
    );
    let preview = h.preview(WorkNotesRange::this_week()).unwrap();
    assert_eq!(preview.session_count, 1);
    let dto = h
        .try_build_with(WorkNotesRange::this_week(), false, "grok", None)
        .unwrap();
    assert_eq!(dto.total_tokens, 5000);
    let later_maps: Vec<String> = h
        .runner
        .recorded()
        .into_iter()
        .skip(2)
        .filter(is_map_command)
        .map(|cmd| prompt_of(&cmd))
        .collect();
    assert_eq!(later_maps.len(), 1, "{later_maps:?}");
    assert!(later_maps[0].contains("对齐口径"), "{}", later_maps[0]);
    assert!(
        !later_maps[0].contains("码表自己生成的"),
        "{}",
        later_maps[0]
    );
    let page = conversation::sessions_page(&h.conn, &ConversationQuery::default()).unwrap();
    let generated = page
        .rows
        .iter()
        .find(|row| row.session_id == generated_id)
        .expect("自造会话应出现在对话记录里");
    assert!(
        generated.generated_by_work_notes,
        "对话记录应打上码表生成标记"
    );
    let user = page
        .rows
        .iter()
        .find(|row| row.session_id == "ok")
        .expect("用户会话");
    assert!(!user.generated_by_work_notes);
}

#[test]
fn cursor_agent_generated_session_is_identified_by_workdir_not_time_window() {
    let h = Harness::new(replies(
        &[&map_json("摘要"), &map_json("摘要一"), &map_json("摘要二")],
        &[&reduce_json(), &reduce_json()],
    ));
    seed_eligible(&h, "ok", "/proj/statistics");
    let first = h
        .try_build_with(WorkNotesRange::this_week(), false, "cursor-agent", None)
        .unwrap();
    h.job.finish(Ok(first)).unwrap();
    let events = eligible_events();
    h.insert_named(
        "cursor_agent",
        Source::CursorAgent,
        SeedSession {
            session_id: "same-hour-user",
            title: "用户自己的活",
            project: "/Users/me/real-project",
            started: day(2026, 8, 18),
            hour: 10,
            tokens: 2000,
            events: &events,
        },
    );
    h.insert_named(
        "cursor_agent",
        Source::CursorAgent,
        SeedSession {
            session_id: "unknown-generated-id",
            title: "码表生成的会话",
            project: h.work_dir().to_str().unwrap(),
            started: day(2026, 8, 18),
            hour: 10,
            tokens: 4000,
            events: &events,
        },
    );
    let preview = h.preview(WorkNotesRange::this_week()).unwrap();
    assert_eq!(preview.session_count, 2, "同小时的真实会话不得被时间窗误杀");
    let dto = h
        .try_build_with(WorkNotesRange::this_week(), false, "cursor-agent", None)
        .unwrap();
    assert_eq!(dto.total_tokens, 8000);
    let later_maps: Vec<String> = h
        .runner
        .recorded()
        .into_iter()
        .skip(2)
        .filter(is_map_command)
        .map(|cmd| prompt_of(&cmd))
        .collect();
    let joined = later_maps.join("\n");
    assert_eq!(later_maps.len(), 2, "{joined}");
    assert!(joined.contains("对齐口径"), "{joined}");
    assert!(joined.contains("用户自己的活"), "{joined}");
    assert!(!joined.contains("码表生成的会话"), "{joined}");
    let page = conversation::sessions_page(&h.conn, &ConversationQuery::default()).unwrap();
    let generated = page
        .rows
        .iter()
        .find(|row| row.session_id == "unknown-generated-id")
        .unwrap();
    assert!(generated.generated_by_work_notes);
    let sibling = page
        .rows
        .iter()
        .find(|row| row.session_id == "same-hour-user")
        .unwrap();
    assert!(!sibling.generated_by_work_notes);
}

#[test]
fn cursor_agent_without_schema_still_parses_fenced_json_to_three_to_six_entries() {
    let fenced = format!("好的，结果如下：\n```json\n{}\n```\n完", reduce_json());
    let h = Harness::new(replies(&[&map_json("修口径")], &[&fenced]));
    seed_eligible(&h, "ok", "/proj/statistics");
    let dto = h
        .try_build_with(WorkNotesRange::this_week(), false, "cursor-agent", None)
        .unwrap();
    assert_eq!(dto.headline, "本周主线是修统计口径");
    assert_eq!(dto.entries.len(), 3);
    assert_eq!(dto.closing, "下周继续补区间和引擎");
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
        None,
    )
    .unwrap();
    let out = work_notes::ProcessRunner
        .run(&cmd, &std::sync::atomic::AtomicBool::new(false))
        .map_err(|error| error.message())
        .expect("codex spawn 失败");
    assert!(!out.trim().is_empty(), "stdout 空：{out:?}");
}

/// `cargo test --manifest-path src-tauri/Cargo.toml work_notes_claude_spawn_smoke -- --ignored --nocapture`
#[test]
#[ignore = "需要本机已登录的 Claude CLI"]
fn work_notes_claude_spawn_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let work_dir = work_notes::ensure_work_dir(dir.path()).unwrap();
    work_notes::write_schemas(&work_dir).unwrap();
    let cmd = work_notes::claude_command(
        &work_dir,
        work_notes::SchemaKind::Map,
        "只输出 JSON：{\"summary\":\"ok\"}".into(),
        None,
    )
    .unwrap();
    let out = work_notes::ProcessRunner
        .run(&cmd, &std::sync::atomic::AtomicBool::new(false))
        .map_err(|error| error.message())
        .expect("claude spawn 失败");
    assert!(!out.trim().is_empty(), "stdout 空：{out:?}");
}

/// `cargo test --manifest-path src-tauri/Cargo.toml work_notes_grok_spawn_smoke -- --ignored --nocapture`
#[test]
#[ignore = "需要本机已登录的 Grok CLI"]
fn work_notes_grok_spawn_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let work_dir = work_notes::ensure_work_dir(dir.path()).unwrap();
    work_notes::write_schemas(&work_dir).unwrap();
    let cmd = work_notes::grok_command(
        &work_dir,
        work_notes::SchemaKind::Map,
        "只输出 JSON：{\"summary\":\"ok\"}".into(),
        None,
    )
    .unwrap();
    let out = work_notes::ProcessRunner
        .run(&cmd, &std::sync::atomic::AtomicBool::new(false))
        .map_err(|error| error.message())
        .expect("grok spawn 失败");
    assert!(!out.trim().is_empty(), "stdout 空：{out:?}");
}

/// `cargo test --manifest-path src-tauri/Cargo.toml work_notes_cursor_agent_spawn_smoke -- --ignored --nocapture`
#[test]
#[ignore = "需要本机已登录的 Cursor Agent CLI"]
fn work_notes_cursor_agent_spawn_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let work_dir = work_notes::ensure_work_dir(dir.path()).unwrap();
    work_notes::write_schemas(&work_dir).unwrap();
    let cmd = work_notes::cursor_agent_command(
        &work_dir,
        work_notes::SchemaKind::Map,
        "只输出 JSON：{\"summary\":\"ok\"}".into(),
        None,
    )
    .unwrap();
    let out = work_notes::ProcessRunner
        .run(&cmd, &std::sync::atomic::AtomicBool::new(false))
        .map_err(|error| error.message())
        .expect("cursor-agent spawn 失败");
    assert!(!out.trim().is_empty(), "stdout 空：{out:?}");
}

fn priced(model: &str, input: f64) -> PriceTable {
    PriceTable {
        prices: vec![PriceEntry {
            model: model.into(),
            provider: None,
            input,
            output: 0.0,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }],
    }
}

fn jsonl_payload(payload: &str, input: i64, output: i64) -> String {
    let text = serde_json::Value::String(payload.to_string());
    format!(
        "{{\"type\":\"thread.started\",\"thread_id\":\"t\"}}\n\
         {{\"type\":\"item.completed\",\"item\":{{\"id\":\"i\",\"type\":\"agent_message\",\"text\":{text}}}}}\n\
         {{\"type\":\"turn.completed\",\"usage\":{{\"input_tokens\":{input},\"cached_input_tokens\":0,\"output_tokens\":{output}}}}}"
    )
}

fn map_count(h: &Harness) -> usize {
    h.runner
        .recorded()
        .iter()
        .filter(|cmd| cmd.args.iter().any(|arg| arg.ends_with("map.schema.json")))
        .count()
}

fn map_titles(h: &Harness) -> Vec<String> {
    let mut titles = Vec::new();
    for cmd in h.runner.recorded() {
        if !cmd.args.iter().any(|arg| arg.ends_with("map.schema.json")) {
            continue;
        }
        for line in cmd.stdin.lines() {
            if let Some(title) = line.strip_prefix("标题：") {
                titles.push(title.to_string());
                break;
            }
        }
    }
    titles
}

#[test]
fn preview_estimates_calls_secs_and_cost_from_compressed_chars() {
    let profile = work_notes::codex_profile();
    assert_eq!(profile.concurrency, 3, "并发必须来自引擎 profile");
    let h = Harness::new(replies(&[], &[])).with_prices(priced(&profile.model, 1.0));
    seed_eligible_n(&h, 6);
    let preview = h.preview(WorkNotesRange::this_week()).unwrap();
    assert_eq!(preview.session_count, 6);
    assert_eq!(preview.estimated_calls, 7, "6 次 map + 1 次 reduce");
    assert_eq!(
        preview.estimated_secs,
        i64::from(profile.secs_per_call) * 3,
        "ceil(6/3) 轮 map + 1 轮 reduce"
    );
    assert!(
        preview.estimated_input_tokens > 0,
        "压缩后字符应能推出 token：{}",
        preview.estimated_input_tokens
    );
    assert!(!preview.estimated_unpriced);
    assert_eq!(
        preview.estimated_cost,
        Some(preview.estimated_input_tokens as f64)
    );
    assert!(h.runner.recorded().is_empty());
}

#[test]
fn preview_has_zero_estimate_when_empty_and_unpriced_without_price() {
    let empty = Harness::new(replies(&[], &[]));
    let preview = empty.preview(WorkNotesRange::this_week()).unwrap();
    assert_eq!(preview.estimated_calls, 0);
    assert_eq!(preview.estimated_secs, 0);
    assert_eq!(preview.estimated_input_tokens, 0);
    assert_eq!(preview.estimated_cost, None);

    let unpriced = Harness::new(replies(&[], &[]));
    seed_eligible(&unpriced, "ok", "/proj/statistics");
    let preview = unpriced.preview(WorkNotesRange::this_week()).unwrap();
    assert_eq!(preview.estimated_calls, 2);
    assert!(preview.estimated_unpriced);
    assert_eq!(preview.estimated_cost, None);
}

#[test]
fn concurrent_maps_use_profile_concurrency_and_call_once_per_session() {
    let profile = work_notes::codex_profile();
    let maps = vec![map_json("摘要"); 4];
    let runner = ScriptedRunner::succeeding(maps, [reduce_json()])
        .with_delay(std::time::Duration::from_millis(40));
    let h = Harness::new(runner);
    seed_eligible_n(&h, 4);
    let dto = h.build();
    assert!(dto.has_data);
    assert_eq!(map_count(&h), 4);
    assert_eq!(h.runner.recorded().len(), 5, "4 map + 1 reduce");
    assert_eq!(
        h.runner.max_in_flight(),
        profile.concurrency as usize,
        "并发必须读 profile，不是编排里写死的另一个数"
    );
}

#[test]
fn one_session_failure_does_not_stop_the_rest_and_keeps_raw_stderr() {
    let runner = replies(
        &[&map_json("摘要"), &map_json("摘要"), &map_json("摘要")],
        &[&reduce_json()],
    )
    .fail_when("会失败", "RAW_STDERR_FROM_CLI");
    let h = Harness::new(runner);
    seed_eligible_at(&h, "ok-1", "成功甲", "/proj/a", day(2026, 8, 18), 10);
    seed_eligible_at(&h, "bad", "会失败", "/proj/a", day(2026, 8, 18), 11);
    seed_eligible_at(&h, "ok-2", "成功乙", "/proj/a", day(2026, 8, 18), 12);
    let dto = h.build();
    assert!(dto.has_data);
    assert_eq!(dto.failed_count, 1);
    assert_eq!(dto.failures.len(), 1);
    assert_eq!(dto.failures[0].title, "会失败");
    assert_eq!(dto.failures[0].error, "RAW_STDERR_FROM_CLI");
    assert_eq!(map_count(&h), 3);
    assert!(h.runner.recorded().iter().any(|cmd| {
        cmd.args
            .iter()
            .any(|arg| arg.ends_with("reduce.schema.json"))
    }));
}

#[test]
fn cancel_keeps_completed_summaries_and_retry_skips_them() {
    let maps = vec![map_json("摘要"); 5];
    let runner = ScriptedRunner::succeeding(maps, [reduce_json()]).cancel_after_maps(2);
    let h = Harness::new(runner);
    for index in 0..5 {
        seed_eligible_at(
            &h,
            &format!("s{index}"),
            &format!("会话{index}"),
            "/proj/a",
            day(2026, 8, 18),
            10 + index as u32,
        );
    }
    let range = WorkNotesRange::this_week();
    h.job.begin(&range, "codex", None).unwrap();
    let error = h.try_build(range.clone(), false).unwrap_err();
    assert!(error.contains("已取消"), "{error}");
    h.job.finish(Err(error)).unwrap();
    assert_eq!(
        h.job.snapshot().unwrap().status,
        WorkNotesJobStatus::Cancelled
    );
    let completed = h.job.completed_session_ids();
    assert!(
        !completed.is_empty(),
        "中断后应留下已完成集合：{completed:?}"
    );
    let first_maps = map_count(&h);
    assert!(first_maps < 5, "不应跑完全部会话，实际 {first_maps}");

    h.job.begin(&range, "codex", None).unwrap();
    let dto = h.try_build(range, false).unwrap();
    assert!(dto.has_data);
    let titles = map_titles(&h);
    let covered: std::collections::HashSet<_> = titles.iter().cloned().collect();
    assert_eq!(covered.len(), 5, "五次调用应覆盖五个会话：{titles:?}");
    for index in 0..5 {
        let title = format!("会话{index}");
        let id = format!("s{index}");
        assert!(covered.contains(&title), "应覆盖 {title}：{titles:?}");
        if completed.contains(&id) {
            assert_eq!(
                titles.iter().filter(|item| *item == &title).count(),
                1,
                "已完成的 {title} 被重算了：{titles:?}"
            );
        }
    }
}

#[test]
fn actual_usage_comes_from_engine_json_and_is_priced() {
    let profile = work_notes::codex_profile();
    let map_out = jsonl_payload(&map_json("摘要"), 100, 20);
    let reduce_out = jsonl_payload(&reduce_json(), 50, 10);
    let h =
        Harness::new(replies(&[&map_out], &[&reduce_out])).with_prices(priced(&profile.model, 2.0));
    seed_eligible(&h, "ok", "/proj/statistics");
    let dto = h.build();
    assert!(dto.has_data);
    assert_eq!(dto.actual_input_tokens, 150);
    assert_eq!(dto.actual_output_tokens, 30);
    assert!(!dto.actual_unpriced);
    assert_eq!(dto.actual_cost, Some(300.0));
}

#[test]
fn progress_tracks_completed_and_total() {
    let h = Harness::new(replies(&[&map_json("摘要")], &[&reduce_json()]));
    seed_eligible(&h, "ok", "/proj/statistics");
    let _ = h.build();
    let progress = h.job.snapshot().unwrap();
    assert_eq!(progress.total, 2, "map + reduce");
    assert_eq!(progress.done, 2);
    assert_eq!(progress.current_title, "正在汇总");
}
