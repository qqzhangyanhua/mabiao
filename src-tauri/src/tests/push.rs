//! 推送（ADR 0026）：区间按重叠整场选、外置正文读不回就整场跳过、打码规则、上下文清单三层语义、
//! 消耗记录快照与指纹、对本机回环桩服务器的分批推送。

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use push_protocol::{
    usage_fingerprint, ApiErrorCode, ContextLayer, LoginResponse, PricingSource,
    PushSessionRequest, PushSessionResponse, PushUsageRequest, PushUsageResponse, RemoteRole,
    UsageTokens,
};

use crate::domain::{ConversationContextLayer, ConversationQuery, ConversationSessionRow};
use crate::push::history;
use crate::push::payload::{build_session, build_usage};
use crate::push::redact::{redact, PLACEHOLDER};
use crate::push::{self, PushEnv, PushRange, PushRunInput, SessionKey, ADMIN_NOTICE};
use crate::remote_server::store::RemoteServerPaths;
use crate::remote_server::{self, LoginInput};
use crate::test_support::http_stub::{api_error, json_reply, serve, Reply, Stub};
use crate::test_support::*;

fn now() -> DateTime<Utc> {
    "2026-10-10T00:00:00Z".parse().unwrap()
}

fn write_text(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// 一场只有一问一答的 Codex 会话，起止时间由调用方定。
pub(super) fn seed_codex(
    home: &Path,
    id: &str,
    cwd: &str,
    start: &str,
    end: &str,
    user: &str,
    assistant: &str,
) -> PathBuf {
    let lines = [
        serde_json::json!({"type":"session_meta","timestamp":start,"payload":{"id":id,"cwd":cwd,"model_provider":"openai"}}),
        serde_json::json!({"type":"turn_context","timestamp":start,"payload":{"cwd":cwd,"model":"gpt-5.5"}}),
        serde_json::json!({"type":"response_item","timestamp":start,"payload":{"type":"message","role":"user","content":[{"type":"input_text","text":user}]}}),
        serde_json::json!({"type":"response_item","timestamp":end,"payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":assistant}]}}),
    ];
    let path = home.join(format!(".codex/sessions/2026/09/rollout-{id}.jsonl"));
    let body = lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    write_text(&path, &format!("{body}\n"));
    path
}

pub(super) fn refreshed(home: &Path) -> rusqlite::Connection {
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    conn
}

fn range(from: Option<&str>, to: Option<&str>) -> PushRange {
    PushRange {
        from: from.map(str::to_string),
        to: to.map(str::to_string),
        sources: Vec::new(),
    }
}

fn listed_ids(conn: &rusqlite::Connection, range: &PushRange) -> Vec<String> {
    let query = ConversationQuery {
        sources: range.sources.clone(),
        from: range.from.clone(),
        to: range.to.clone(),
        ..Default::default()
    };
    crate::conversation::list_push_sessions(conn, &query)
        .unwrap()
        .into_iter()
        .map(|row| row.session_id)
        .collect()
}

fn only_row(conn: &rusqlite::Connection, session_id: &str) -> ConversationSessionRow {
    crate::conversation::list_push_sessions(conn, &ConversationQuery::default())
        .unwrap()
        .into_iter()
        .find(|row| row.session_id == session_id)
        .unwrap_or_else(|| panic!("没有会话 {session_id}"))
}

fn device() -> push_protocol::DeviceInfo {
    push_protocol::DeviceInfo {
        device_id: "device-1".into(),
        device_name: "测试机".into(),
    }
}

// ---- 区间：按重叠整场选 ----

fn seed_three_sessions(home: &Path) {
    seed_codex(
        home,
        "a",
        "/w/a",
        "2026-09-01T00:00:00Z",
        "2026-09-01T01:00:00Z",
        "q",
        "a",
    );
    seed_codex(
        home,
        "b",
        "/w/b",
        "2026-09-02T10:00:00Z",
        "2026-09-02T11:00:00Z",
        "q",
        "a",
    );
    seed_codex(
        home,
        "c",
        "/w/c",
        "2026-09-03T00:00:00Z",
        "2026-09-05T00:00:00Z",
        "q",
        "a",
    );
}

#[test]
fn overlap_includes_both_boundaries_and_never_splits_a_session() {
    let temp = tempfile::tempdir().unwrap();
    seed_three_sessions(temp.path());
    let conn = refreshed(temp.path());
    let ids = |r: PushRange| listed_ids(&conn, &r);

    assert_eq!(ids(range(None, None)), ["a", "b", "c"]);
    // 结束时间恰好等于 from：算重叠，差一秒就不算。
    assert_eq!(
        ids(range(Some("2026-09-01T01:00:00Z"), None)),
        ["a", "b", "c"]
    );
    assert_eq!(ids(range(Some("2026-09-01T01:00:01Z"), None)), ["b", "c"]);
    // 开始时间恰好等于 to：算重叠，早一秒就不算。
    assert_eq!(ids(range(None, Some("2026-09-02T10:00:00Z"))), ["a", "b"]);
    assert_eq!(ids(range(None, Some("2026-09-02T09:59:59Z"))), ["a"]);
    // 区间整个落在一场会话中间：这场会话整场入选。
    assert_eq!(
        ids(range(
            Some("2026-09-04T00:00:00Z"),
            Some("2026-09-04T12:00:00Z")
        )),
        ["c"]
    );
    // 区间夹在两场之间的空档：一场都不选。
    assert!(ids(range(
        Some("2026-09-01T02:00:00Z"),
        Some("2026-09-02T09:00:00Z")
    ))
    .is_empty());
}

#[test]
fn source_filter_limits_the_sessions() {
    let temp = tempfile::tempdir().unwrap();
    seed_three_sessions(temp.path());
    let conn = refreshed(temp.path());

    let mut only_codex = range(None, None);
    only_codex.sources = vec!["codex".into()];
    assert_eq!(listed_ids(&conn, &only_codex), ["a", "b", "c"]);
    let mut only_claude = range(None, None);
    only_claude.sources = vec!["claude".into()];
    assert!(listed_ids(&conn, &only_claude).is_empty());
}

#[test]
fn archived_session_is_listed_but_skipped_with_a_reason() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let conn = refreshed(home);
    conn.execute(
        "UPDATE conversation_sessions SET file_available = 0 WHERE session_id = 'b'",
        [],
    )
    .unwrap();

    assert_eq!(listed_ids(&conn, &range(None, None)), ["a", "b", "c"]);
    let error = crate::conversation::read_push_session(&conn, home, &only_row(&conn, "b"))
        .err()
        .expect("正文读不回的归档会话应跳过");
    assert!(error.contains("原始文件已不存在"), "{error}");
    assert!(crate::conversation::read_push_session(&conn, home, &only_row(&conn, "a")).is_ok());
}

// ---- 读回正文：ADR 0025 外置正文 ----

const SEMANTIC_PATH: &str = ".codex/sessions/2026/08/rollout-semantic-1.jsonl";

fn seed_semantic(home: &Path) -> (rusqlite::Connection, PathBuf) {
    let path = write_home_fixture(home, SEMANTIC_PATH, "codex-semantic-events.jsonl");
    let conn = refreshed(home);
    (conn, path)
}

#[test]
fn external_texts_are_read_back_in_full_with_contiguous_sequences() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (conn, _) = seed_semantic(home);

    let source =
        crate::conversation::read_push_session(&conn, home, &only_row(&conn, "semantic-1"))
            .expect("源文件没变，外置正文应读得回");
    let texts: Vec<_> = source
        .events
        .iter()
        .filter_map(|event| event.text.as_deref())
        .collect();
    assert!(texts.contains(&"实现语义时间线"), "{texts:?}");
    assert!(texts.contains(&"我先检查现有实现。"), "{texts:?}");
    assert!(texts.contains(&"fn main() {}"), "{texts:?}");
    for (index, event) in source.events.iter().enumerate() {
        assert_eq!(event.sequence as usize, index);
        assert!(event.details.is_null(), "原始载荷不出本机");
    }
}

#[test]
fn rewritten_source_line_skips_the_whole_session_instead_of_pushing_stale_text() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (conn, path) = seed_semantic(home);
    let original = std::fs::read_to_string(&path).unwrap();
    let rewritten = original.replace("实现语义时间线", "实现语义时间轴");
    assert_eq!(
        rewritten.len(),
        original.len(),
        "同长改写，偏移不变只靠指纹发现"
    );
    std::fs::write(&path, rewritten).unwrap();

    let result =
        crate::conversation::read_push_session(&conn, home, &only_row(&conn, "semantic-1"));
    match result {
        Err(reason) => assert!(!reason.is_empty()),
        Ok(source) => {
            // 走了整份解析：必须是现在这份源文件的内容，不能夹着旧正文。
            let texts: Vec<_> = source
                .events
                .iter()
                .filter_map(|e| e.text.as_deref())
                .collect();
            assert!(texts.contains(&"实现语义时间轴"), "{texts:?}");
            assert!(!texts.contains(&"实现语义时间线"), "{texts:?}");
        }
    }
}

#[test]
fn missing_source_file_skips_the_session_with_a_reason() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (conn, path) = seed_semantic(home);
    std::fs::remove_file(&path).unwrap();

    let error = crate::conversation::read_push_session(&conn, home, &only_row(&conn, "semantic-1"))
        .err()
        .expect("源文件没了就读不回外置正文");
    assert!(!error.is_empty());
}

#[test]
fn preview_lists_unreadable_sessions_as_skipped_and_counts_the_rest() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let conn = refreshed(home);
    conn.execute(
        "UPDATE conversation_sessions SET file_available = 0 WHERE session_id = 'b'",
        [],
    )
    .unwrap();
    let h = Harness::new(home, conn);

    let preview = push::preview(&h.env(), &range(None, None)).unwrap();
    assert_eq!(preview.sessions, 2);
    let expected_events: usize = ["a", "c"]
        .iter()
        .map(|id| {
            let row = only_row(&h.conns.get(), id);
            crate::conversation::read_push_session(&h.conns.get(), home, &row)
                .unwrap()
                .events
                .len()
        })
        .sum();
    assert_eq!(preview.events as usize, expected_events);
    assert_eq!(preview.skipped.len(), 1);
    assert_eq!(preview.skipped[0].session_id, "b");
    assert!(preview.skipped[0].reason.contains("原始文件"));
    assert!(preview.estimated_bytes > 0);
    assert_eq!(preview.notice, ADMIN_NOTICE);
    assert!(preview.notice.contains("管理员"));
}

// ---- 打码 ----

fn assert_redacted(input: &str, secret: &str) {
    let (output, count) = redact(input);
    assert!(!output.contains(secret), "{secret} 应被打码：{output}");
    assert!(output.contains(PLACEHOLDER), "{output}");
    assert!(count >= 1, "{output}");
}

#[test]
fn redaction_covers_the_builtin_secret_formats() {
    assert_redacted(
        "key is sk-proj-abcdefghijklmnopqrstuvwx1234 ok",
        "abcdefghijklmnopqrstuvwx1234",
    );
    assert_redacted(
        "export GH=ghp_abcdefghijklmnopqrstuvwxyz0123456789",
        "abcdefghijklmnopqrstuvwxyz0123456789",
    );
    assert_redacted("aws AKIAIOSFODNN7EXAMPLE here", "AKIAIOSFODNN7EXAMPLE");
    assert_redacted(
        "Authorization: Bearer abcdefghijklmnop.qrstuvwxyz-0123456789",
        "abcdefghijklmnop.qrstuvwxyz-0123456789",
    );
    assert_redacted(
        "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r",
        "eyJzdWIiOiIxMjM0NTY3ODkwIn0",
    );
    assert_redacted(
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA\nzzz\n-----END RSA PRIVATE KEY-----",
        "MIIEowIBAAKCAQEA",
    );
    assert_redacted("db password=hunter2hunter2 done", "hunter2hunter2");
    assert_redacted("API_KEY: abcd1234efgh", "abcd1234efgh");
    // 环境变量风格：键名带前缀也要认。
    assert_redacted("GITHUB_TOKEN=abcd1234efgh run", "abcd1234efgh");
    assert_redacted("client_secret: abcd1234efgh", "abcd1234efgh");
}

#[test]
fn assignment_redaction_keeps_the_key_and_the_quotes() {
    let (output, count) = redact(r#"{"password": "p@ss w0rd", "name": "alice"}"#);
    assert_eq!(output, r#"{"password": "[REDACTED]", "name": "alice"}"#);
    assert_eq!(count, 1);

    let (output, _) = redact("secret='top secret value'");
    assert_eq!(output, "secret='[REDACTED]'");

    // 嵌在 JSON 字符串里的 JSON：引号是转义过的，也要整值替换。
    let (output, _) = redact(r#"{"cmd":"{\"api_key\":\"abc123def456\"}"}"#);
    assert!(!output.contains("abc123def456"), "{output}");
    assert!(
        serde_json::from_str::<serde_json::Value>(&output).is_ok(),
        "{output}"
    );
}

#[test]
fn redaction_is_idempotent_and_counts_only_new_replacements() {
    let (once, first) = redact("password=hunter2hunter2 and sk-abcdefghijklmnopqrstuv");
    assert_eq!(first, 2);
    let (twice, second) = redact(&once);
    assert_eq!(twice, once);
    assert_eq!(second, 0);
}

#[test]
fn redaction_leaves_paths_project_names_and_plain_prose_alone() {
    for text in [
        "/Users/zhangyanhua/AI/statistics/src-tauri/src/lib.rs",
        "项目 mabiao 的 token 用量统计",
        "the max_tokens setting and tokens: 120 per call",
        "sk-short",
        "git@github.com:qqzhangyanhua/mabiao.git",
        "PWD=/Users/zhangyanhua/AI/statistics",
    ] {
        assert_eq!(redact(text), (text.to_string(), 0), "{text}");
    }
}

#[test]
fn built_session_is_redacted_everywhere_but_keeps_paths_and_project() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let project = home.join("workspace/secret-proj");
    std::fs::create_dir_all(&project).unwrap();
    seed_codex(
        home,
        "leaky",
        &project.to_string_lossy(),
        "2026-09-01T00:00:00Z",
        "2026-09-01T00:10:00Z",
        "请用 sk-abcdefghijklmnopqrstuv1234 登录",
        "已设置 password=hunter2hunter2，路径 /etc/hosts",
    );
    let conn = refreshed(home);
    let source =
        crate::conversation::read_push_session(&conn, home, &only_row(&conn, "leaky")).unwrap();

    let built = build_session(source, &device());
    // 标题取自首问，所以首问里的那个密钥在标题和事件里各打码一次。
    assert_eq!(built.redactions, 3);
    assert_eq!(built.request.session.redaction_count, 3);
    assert!(!built
        .request
        .session
        .title
        .contains("abcdefghijklmnopqrstuv1234"));
    let body = serde_json::to_string(&built.request).unwrap();
    assert!(!body.contains("abcdefghijklmnopqrstuv1234"), "{body}");
    assert!(!body.contains("hunter2hunter2"), "{body}");
    assert!(body.contains("/etc/hosts"), "路径不打码");
    assert_eq!(built.request.session.project, project.to_string_lossy());
    assert!(built.bytes as usize >= body.len());
    assert_eq!(
        built.event_count as usize,
        built.request.session.events.len()
    );
}

#[test]
fn git_remote_comes_from_dot_git_config_only_and_drops_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let project = home.join("workspace/with-git");
    write_text(
        &project.join(".git/config"),
        "[remote \"origin\"]\n\turl = https://alice:ghs_secrettoken@github.com/acme/app.git\n",
    );
    seed_codex(
        home,
        "g",
        &project.to_string_lossy(),
        "2026-09-01T00:00:00Z",
        "2026-09-01T00:10:00Z",
        "q",
        "a",
    );
    seed_codex(
        home,
        "plain",
        &home.join("workspace/no-git").to_string_lossy(),
        "2026-09-01T00:00:00Z",
        "2026-09-01T00:10:00Z",
        "q",
        "a",
    );
    let conn = refreshed(home);
    let remote_of = |id: &str| {
        let source =
            crate::conversation::read_push_session(&conn, home, &only_row(&conn, id)).unwrap();
        build_session(source, &device())
            .request
            .session
            .git_remote_url
    };

    assert_eq!(
        remote_of("g").as_deref(),
        Some("https://github.com/acme/app.git")
    );
    assert_eq!(remote_of("plain"), None);
}

// ---- 上下文清单三层 ----

fn seed_grok(home: &Path, id: &str) -> PathBuf {
    let path = home
        .join(".grok/sessions/%2Fworkspace%2Fgrok")
        .join(id)
        .join("updates.jsonl");
    write_text(
        &path,
        concat!(
            "{\"timestamp\":1787100000,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-user\"},\"update\":{\"sessionUpdate\":\"user_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"follow AGENTS.md\"}}}}\n",
            "{\"timestamp\":1787100001,\"method\":\"session/update\",\"params\":{\"_meta\":{\"eventId\":\"tool-read-1\"},\"update\":{\"sessionUpdate\":\"tool_call\",\"title\":\"Read\",\"toolCallId\":\"call-read-1\"}}}\n",
            "{\"timestamp\":1787100006,\"method\":\"session/update\",\"params\":{\"_meta\":{\"eventId\":\"turn\",\"promptId\":\"prompt-1\"},\"update\":{\"sessionUpdate\":\"turn_completed\",\"prompt_id\":\"prompt-1\",\"stop_reason\":\"end_turn\"}}}\n"
        ),
    );
    write_text(
        &path.parent().unwrap().join("summary.json"),
        r#"{"current_model_id":"grok-test"}"#,
    );
    path
}

fn write_prompt_context(updates: &Path, path: &str, content: &str) {
    write_text(
        &updates.parent().unwrap().join("prompt_context.json"),
        &serde_json::json!({
            "version": 1,
            "agents_md_files": [{"file_name": "AGENTS.md", "file_path": path, "content": content}],
        })
        .to_string(),
    );
}

fn refresh_grok(conn: &rusqlite::Connection, home: &Path) {
    crate::conversation::refresh(
        conn,
        Source::Grok,
        &ingest::source_scan_dirs(home, Source::Grok),
    )
    .unwrap();
}

const INJECTED_BODY: &str = "UNIQUE_INJECTED_BODY use password=hunter2hunter2 never";

fn grok_request(conn: &rusqlite::Connection, home: &Path, id: &str) -> PushSessionRequest {
    let row = crate::conversation::list_push_sessions(conn, &ConversationQuery::default())
        .unwrap()
        .into_iter()
        .find(|row| row.session_id == id)
        .unwrap();
    let source = crate::conversation::read_push_session(conn, home, &row).unwrap();
    build_session(source, &device()).request
}

#[test]
fn injected_layer_carries_redacted_raw_text_read_live_from_the_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let updates = seed_grok(home, "grok-live");
    write_prompt_context(&updates, "/workspace/proj/AGENTS.md", INJECTED_BODY);
    write_text(&home.join(".grok/AGENTS.md"), "# on disk only\n");
    let conn = store::open_memory().unwrap();
    refresh_grok(&conn, home);

    let request = grok_request(&conn, home, "grok-live");
    request.validate().expect("协议校验应通过");
    let manifest = request.session.context_manifest.as_ref().unwrap();
    assert!(manifest.has_injected_snapshot);
    assert!(!manifest.from_cache);

    let injected = manifest
        .items
        .iter()
        .find(|item| item.layer == ContextLayer::Injected && item.id == "/workspace/proj/AGENTS.md")
        .expect("注入层应有 AGENTS.md");
    let content = injected.content.as_deref().expect("注入层带原文");
    assert!(content.contains("UNIQUE_INJECTED_BODY"));
    assert!(
        !content.contains("hunter2hunter2"),
        "注入原文也要打码：{content}"
    );
    assert_eq!(request.session.redaction_count, 1);

    let on_disk: Vec<_> = manifest
        .items
        .iter()
        .filter(|item| item.layer == ContextLayer::OnDiskPossible)
        .collect();
    assert!(
        !on_disk.is_empty(),
        "磁盘上的 ~/.grok/AGENTS.md 应列为可能生效"
    );
    assert!(
        on_disk.iter().all(|item| item.content.is_none()),
        "磁盘层只推条目、路径、体积"
    );
    assert!(manifest
        .items
        .iter()
        .filter(|item| item.layer == ContextLayer::Observed)
        .all(|item| item.content.is_none()));

    // 注入原文只在这一次读取里，不落本机库。
    let stored: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM conversation_events WHERE coalesce(text, '') LIKE '%UNIQUE_INJECTED_BODY%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, 0);
}

#[test]
fn cleaned_snapshot_pushes_cached_metrics_only_flagged_as_from_cache() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let updates = seed_grok(home, "grok-cached");
    write_prompt_context(&updates, "/workspace/proj/AGENTS.md", INJECTED_BODY);
    let conn = store::open_memory().unwrap();
    refresh_grok(&conn, home);
    std::fs::remove_file(updates.parent().unwrap().join("prompt_context.json")).unwrap();

    let request = grok_request(&conn, home, "grok-cached");
    request
        .validate()
        .expect("来自缓存的清单不带原文，校验应通过");
    let manifest = request.session.context_manifest.as_ref().unwrap();
    assert!(manifest.from_cache, "要标「来自缓存，无原文」");
    let injected: Vec<_> = manifest
        .items
        .iter()
        .filter(|item| item.layer == ContextLayer::Injected)
        .collect();
    assert!(!injected.is_empty(), "度量条目还在");
    assert!(manifest.items.iter().all(|item| item.content.is_none()));
    assert_eq!(
        injected[0].char_count,
        Some(INJECTED_BODY.chars().count() as u64)
    );
    assert!(!serde_json::to_string(&request)
        .unwrap()
        .contains("UNIQUE_INJECTED_BODY"));
}

#[test]
fn cursor_injected_sections_are_pushed_from_the_chat_store_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_cursor_transcript(
        home,
        "Users-workspace-project",
        "sess-cursor-push",
        concat!(
            "{\"role\":\"user\",\"timestamp\":\"2026-09-08T00:00:00Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n",
            "{\"role\":\"assistant\",\"timestamp\":\"2026-09-08T00:00:01Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}\n"
        ),
    );
    write_cursor_chat_store(
        home,
        "sess-cursor-push",
        &[
            serde_json::json!({"role":"system","content":"You are an AI coding assistant."}),
            serde_json::json!({"role":"user","content": fixture("cursor-inject-first-turn.txt")}),
        ],
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh(&conn, Source::CursorAgent, &[home.join(".cursor/projects")])
        .unwrap();

    let request = grok_request(&conn, home, "sess-cursor-push");
    request.validate().unwrap();
    let manifest = request.session.context_manifest.as_ref().unwrap();
    assert!(manifest.has_injected_snapshot);
    let content_of = |id: &str| {
        manifest
            .items
            .iter()
            .find(|item| item.layer == ContextLayer::Injected && item.id == id)
            .and_then(|item| item.content.as_deref())
    };
    assert_eq!(
        content_of("/tmp/home/.cursor/skills/review/SKILL.md"),
        Some("UNIQUE_CURSOR_INJECT_SKILL")
    );
    assert_eq!(
        content_of("/tmp/workspace/demo/AGENTS.md"),
        Some("UNIQUE_CURSOR_INJECT_RULE\n")
    );
    assert!(manifest
        .items
        .iter()
        .filter(|item| item.layer != ContextLayer::Injected)
        .all(|item| item.content.is_none()));
}

#[test]
fn context_layers_map_one_to_one_to_the_wire_layers() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let updates = seed_grok(home, "grok-layers");
    write_prompt_context(&updates, "/workspace/proj/AGENTS.md", "body");
    let conn = store::open_memory().unwrap();
    refresh_grok(&conn, home);
    let row = only_row(&conn, "grok-layers");
    let source = crate::conversation::read_push_session(&conn, home, &row).unwrap();
    let local = source.context.as_ref().unwrap().manifest.items.clone();
    let wire = build_session(source, &device())
        .request
        .session
        .context_manifest
        .unwrap()
        .items;

    assert_eq!(local.len(), wire.len());
    for (local, wire) in local.iter().zip(&wire) {
        let expected = match local.layer {
            ConversationContextLayer::Injected => ContextLayer::Injected,
            ConversationContextLayer::Observed => ContextLayer::Observed,
            ConversationContextLayer::OnDiskPossible => ContextLayer::OnDiskPossible,
        };
        assert_eq!(wire.layer, expected, "{}", local.id);
    }
}

// ---- 消耗记录 ----

pub(super) fn usage_row(at: &str, session: &str, total: i64) -> UsageRecord {
    let mut record = rec(
        at,
        Source::Codex,
        "gpt-5.5",
        "subapi",
        "/w/a",
        session,
        total,
    );
    record.input_tokens = total;
    record
}

#[test]
fn usage_payload_carries_fingerprint_cost_snapshot_and_pricing_source() {
    let prices = diverse_prices();
    let priced = usage_row("2026-09-01T00:00:00Z", "s1", 1000);
    let mut native = usage_row("2026-09-01T00:01:00Z", "s2", 10);
    native.native_cost = Some(0.5);
    let mut unpriced = usage_row("2026-09-01T00:02:00Z", "s3", 10);
    unpriced.model = "no-such-model".into();
    unpriced.provider = "nobody".into();

    let built = build_usage(&[priced.clone(), native, unpriced], &prices);
    assert_eq!(built.skipped_invalid_time, 0);
    assert_eq!(built.records.len(), 3);

    let first = &built.records[0];
    let tokens = UsageTokens {
        input: 1000,
        output: 0,
        cache_read: 0,
        cache_creation: 0,
        reasoning: 0,
        total: 1000,
    };
    assert_eq!(
        first.fingerprint,
        usage_fingerprint(
            "codex",
            &priced.source_file,
            &priced.occurred_at,
            "gpt-5.5",
            &tokens
        )
    );
    let expected = derive_cost(&priced, &prices);
    assert_eq!(first.cost_snapshot, expected.amount, "与共享计价一致");
    assert_eq!(first.pricing_source, PricingSource::Exact);
    assert_eq!(built.records[1].pricing_source, PricingSource::Native);
    assert_eq!(built.records[1].cost_snapshot, Some(0.5));
    assert_eq!(built.records[2].pricing_source, PricingSource::Unpriced);
}

#[test]
fn usage_with_unparseable_time_is_dropped_locally_and_duplicates_collapse() {
    let prices = diverse_prices();
    let good = usage_row("2026-09-01T00:00:00Z", "s1", 5);
    let bad = usage_row("yesterday", "s2", 5);
    let built = build_usage(&[good.clone(), bad, good], &prices);
    assert_eq!(built.skipped_invalid_time, 1);
    assert_eq!(built.records.len(), 1);
}

#[test]
fn preview_counts_usage_by_occurred_at_including_archived_rows() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let conn = refreshed(home);
    store::insert_records(
        &conn,
        &[
            usage_row("2026-09-01T00:30:00Z", "a", 10),
            usage_row("2026-09-02T10:30:00Z", "gone", 20),
            usage_row("2026-09-04T00:00:00Z", "c", 30),
            usage_row("not-a-time", "c", 40),
        ],
    )
    .unwrap();
    let h = Harness::new(home, conn);

    let all = push::preview(&h.env(), &range(None, None)).unwrap();
    assert_eq!(all.usage_records, 3);
    assert_eq!(all.usage_skipped_invalid_time, 1);

    let day_one = range(Some("2026-09-01T00:00:00Z"), Some("2026-09-01T23:59:59Z"));
    assert_eq!(push::preview(&h.env(), &day_one).unwrap().usage_records, 1);
    // 边界两端都含。
    let exact = range(Some("2026-09-02T10:30:00Z"), Some("2026-09-02T10:30:00Z"));
    assert_eq!(push::preview(&h.env(), &exact).unwrap().usage_records, 1);
}

// ---- 推送：对桩服务器 ----

pub(super) struct Harness {
    _dir: tempfile::TempDir,
    pub(super) home: PathBuf,
    pub(super) conns: TestConnection,
    pub(super) remote: RemoteServerPaths,
    pub(super) history: PathBuf,
    pub(super) prices: crate::domain::PriceTable,
}

impl Harness {
    pub(super) fn new(home: &Path, conn: rusqlite::Connection) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let remote = RemoteServerPaths::in_dir(dir.path());
        let history = dir.path().join("push_history.json");
        Self {
            _dir: dir,
            home: home.to_path_buf(),
            conns: TestConnection::new(conn),
            remote,
            history,
            prices: diverse_prices(),
        }
    }

    fn env(&self) -> PushEnv<'_> {
        PushEnv {
            conns: &self.conns,
            home: &self.home,
            prices: &self.prices,
            remote: &self.remote,
            history: &self.history,
            now: now(),
        }
    }

    pub(super) fn dir_path(&self) -> &Path {
        self._dir.path()
    }

    pub(super) fn login(&self, stub: &Stub) {
        remote_server::login(
            &self.remote,
            LoginInput {
                base_url: stub.base_url.clone(),
                account: "alice".into(),
                password: "pw".into(),
                device_name: Some("我的 MacBook".into()),
            },
            now(),
        )
        .unwrap();
    }
}

pub(super) fn login_reply() -> Reply {
    json_reply(
        200,
        LoginResponse {
            token: "tok-secret-123".into(),
            expires_at: (now() + Duration::days(30)).to_rfc3339(),
            account: "alice".into(),
            role: RemoteRole::Member,
        },
    )
}

pub(super) fn session_ok(id: &str) -> Reply {
    json_reply(
        200,
        PushSessionResponse {
            source: "codex".into(),
            session_id: id.into(),
            replaced: false,
        },
    )
}

pub(super) fn usage_ok(inserted: u32, duplicates: u32) -> Reply {
    json_reply(
        200,
        PushUsageResponse {
            inserted,
            duplicates,
        },
    )
}

fn pushed_requests(stub: &Stub) -> Vec<PushSessionRequest> {
    stub.captured
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.request_line.contains("/api/v1/push/session"))
        .map(|c| serde_json::from_str(&c.body).unwrap())
        .collect()
}

fn no_progress(_: push::PushProgress) {}

#[test]
fn run_sends_one_request_per_session_then_usage_and_records_history() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex(
        home,
        "a",
        "/w/a",
        "2026-09-01T00:00:00Z",
        "2026-09-01T01:00:00Z",
        "用 sk-abcdefghijklmnopqrstuv1234 试试",
        "好",
    );
    seed_codex(
        home,
        "b",
        "/w/b",
        "2026-09-02T00:00:00Z",
        "2026-09-02T01:00:00Z",
        "q",
        "a",
    );
    let conn = refreshed(home);
    // 「码表生成」标记跟会话一起推。
    conn.execute(
        "INSERT INTO work_notes_generated_sessions(engine, session_id, work_dir, started_at, ended_at)
         VALUES('codex', 'b', '', '', '')",
        [],
    )
    .unwrap();
    store::insert_records(
        &conn,
        &[
            usage_row("2026-09-01T00:30:00Z", "a", 10),
            usage_row("2026-09-02T00:30:00Z", "b", 20),
        ],
    )
    .unwrap();
    let h = Harness::new(home, conn);
    let stub = serve(vec![
        login_reply(),
        session_ok("a"),
        session_ok("b"),
        usage_ok(2, 0),
    ]);
    h.login(&stub);

    let progress = std::sync::Mutex::new(Vec::new());
    let outcome = push::run(
        &h.env(),
        &PushRunInput {
            range: range(None, None),
            only: Vec::new(),
            include_usage: true,
        },
        &|p| progress.lock().unwrap().push(p),
    )
    .unwrap();

    assert_eq!(outcome.sessions_succeeded, 2);
    assert!(outcome.failed.is_empty() && outcome.skipped.is_empty());
    assert_eq!((outcome.usage_inserted, outcome.usage_duplicates), (2, 0));
    assert!(outcome.usage_error.is_none());
    assert!(!outcome.login_required);

    let captured = stub.captured.lock().unwrap();
    assert_eq!(captured.len(), 4, "登录 + 每场一个请求 + 一批消耗记录");
    assert!(captured[1..]
        .iter()
        .all(|c| c.headers.contains("Bearer tok-secret-123")));
    assert!(captured[3].request_line.contains("/api/v1/push/usage"));
    drop(captured);

    let sessions = pushed_requests(&stub);
    assert_eq!(sessions.len(), 2);
    for request in &sessions {
        request.validate().unwrap();
        assert_eq!(request.device.device_name, "我的 MacBook");
    }
    let (a, b) = (&sessions[0].session, &sessions[1].session);
    assert_eq!((a.session_id.as_str(), b.session_id.as_str()), ("a", "b"));
    assert!(!a.generated_by_work_notes);
    assert!(b.generated_by_work_notes, "码表生成标记随会话推送");
    assert_eq!(a.redaction_count, 2, "标题与首问各一处");
    assert!(!serde_json::to_string(a)
        .unwrap()
        .contains("abcdefghijklmnopqrstuv1234"));

    let usage: PushUsageRequest =
        serde_json::from_str(&stub.captured.lock().unwrap()[3].body).unwrap();
    assert_eq!(usage.records.len(), 2);

    let entries = history::load(&h.history);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].sessions_succeeded, 2);
    assert_eq!(entries[0].usage_inserted, 2);
    assert_eq!(entries[0].at, now().to_rfc3339());

    let progress = progress.lock().unwrap();
    assert_eq!(progress.first().map(|p| (p.done, p.total)), Some((0, 2)));
    assert_eq!(progress.last().map(|p| (p.done, p.total)), Some((2, 2)));
}

#[test]
fn failed_session_is_listed_and_retry_sends_only_that_session() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let conn = refreshed(home);
    let h = Harness::new(home, conn);
    let stub = serve(vec![
        login_reply(),
        session_ok("a"),
        api_error(500, ApiErrorCode::Internal),
        session_ok("c"),
        session_ok("b"),
    ]);
    h.login(&stub);

    let first = push::run(
        &h.env(),
        &PushRunInput {
            range: range(None, None),
            only: Vec::new(),
            include_usage: true,
        },
        &no_progress,
    )
    .unwrap();
    assert_eq!(first.sessions_succeeded, 2);
    assert_eq!(first.failed.len(), 1);
    assert_eq!(first.failed[0].session_id, "b");
    assert!(!first.failed[0].reason.is_empty());
    assert!(!first.login_required);

    let retry = push::run(
        &h.env(),
        &PushRunInput {
            range: range(None, None),
            only: vec![SessionKey {
                source: "codex".into(),
                session_id: "b".into(),
            }],
            include_usage: false,
        },
        &no_progress,
    )
    .unwrap();
    assert_eq!(retry.sessions_succeeded, 1);
    assert!(retry.failed.is_empty());

    let ids: Vec<_> = pushed_requests(&stub)
        .into_iter()
        .map(|r| r.session.session_id)
        .collect();
    assert_eq!(ids, ["a", "b", "c", "b"]);
    assert_eq!(history::load(&h.history).len(), 2);
}

#[test]
fn unreadable_session_is_skipped_not_sent_and_not_retryable_failure() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let conn = refreshed(home);
    conn.execute(
        "UPDATE conversation_sessions SET file_available = 0 WHERE session_id = 'a'",
        [],
    )
    .unwrap();
    let h = Harness::new(home, conn);
    let stub = serve(vec![
        login_reply(),
        session_ok("b"),
        session_ok("c"),
        usage_ok(0, 0),
    ]);
    h.login(&stub);

    let outcome = push::run(
        &h.env(),
        &PushRunInput {
            range: range(None, None),
            only: Vec::new(),
            include_usage: true,
        },
        &no_progress,
    )
    .unwrap();
    assert_eq!(outcome.sessions_succeeded, 2);
    assert!(outcome.failed.is_empty());
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].session_id, "a");
    assert!(outcome.skipped[0].reason.contains("原始文件"));
}

#[test]
fn rejected_token_stops_the_run_and_marks_login_required() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let conn = refreshed(home);
    let h = Harness::new(home, conn);
    let stub = serve(vec![
        login_reply(),
        session_ok("a"),
        api_error(401, ApiErrorCode::TokenExpired),
    ]);
    h.login(&stub);

    let outcome = push::run(
        &h.env(),
        &PushRunInput {
            range: range(None, None),
            only: Vec::new(),
            include_usage: true,
        },
        &no_progress,
    )
    .unwrap();
    assert!(outcome.login_required);
    assert_eq!(outcome.sessions_succeeded, 1);
    let failed: Vec<_> = outcome
        .failed
        .iter()
        .map(|i| i.session_id.as_str())
        .collect();
    assert_eq!(failed, ["b", "c"]);
    assert_eq!(outcome.usage_inserted, 0);
    assert_eq!(
        stub.captured.lock().unwrap().len(),
        3,
        "token 被拒后不再发请求，也不发消耗记录"
    );
    assert!(
        remote_server::push_credentials(&h.remote, now()).is_err(),
        "设置页应提示重新登录"
    );
}

#[test]
fn not_logged_in_is_an_error_that_points_to_login_and_sends_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let h = Harness::new(home, refreshed(home));

    let error = push::run(&h.env(), &PushRunInput::default(), &no_progress).unwrap_err();
    assert!(error.contains("登录"), "{error}");
    assert!(history::load(&h.history).is_empty());
    // 预览只读本机，没登录也能看。
    assert_eq!(
        push::preview(&h.env(), &range(None, None))
            .unwrap()
            .sessions,
        3
    );
}

#[test]
fn usage_is_sent_in_batches_of_at_most_5000() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let conn = refreshed(home);
    let records: Vec<_> = (0..5001)
        .map(|n| usage_row("2026-09-01T00:00:00Z", &format!("s{n}"), n + 1))
        .collect();
    store::insert_records(&conn, &records).unwrap();
    let h = Harness::new(home, conn);
    let stub = serve(vec![login_reply(), usage_ok(5000, 0), usage_ok(1, 0)]);
    h.login(&stub);

    let input = PushRunInput {
        include_usage: true,
        ..Default::default()
    };
    let outcome = push::run(&h.env(), &input, &no_progress).unwrap();
    assert_eq!(outcome.usage_inserted, 5001);
    let sizes: Vec<usize> = stub.captured.lock().unwrap()[1..]
        .iter()
        .map(|c| {
            serde_json::from_str::<PushUsageRequest>(&c.body)
                .unwrap()
                .records
                .len()
        })
        .collect();
    assert_eq!(sizes, [5000, 1]);
}
