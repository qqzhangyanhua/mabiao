use std::io::Cursor;
use std::path::Path;

use rusqlite::params;

use crate::domain::ConversationQuery;
use crate::test_support::*;

fn write_text(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn conversation_roots(home: &Path, source: Source) -> Vec<std::path::PathBuf> {
    if source == Source::CursorAgent {
        vec![home.join(".cursor/projects")]
    } else {
        ingest::source_scan_dirs(home, source)
    }
}

fn refresh_source(conn: &rusqlite::Connection, home: &Path, source: Source) {
    crate::conversation::refresh_source_in_roots(conn, source, &conversation_roots(home, source))
        .unwrap();
}

fn seed_dsh(home: &Path) {
    let path = home.join(".dsh/sessions/-workspace-project/session-dsh/session.jsonl.zstd");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let content = concat!(
        "{\"type\":\"session\",\"version\":0,\"id\":\"dsh-session-1\",\"createdAt\":1786629319248,\"cwd\":\"/workspace/project\"}\n",
        "{\"type\":\"user/message\",\"seq\":2,\"time\":1786629319400,\"data\":{\"content\":[{\"type\":\"text\",\"text\":\"Inspect the compressed session\"}],\"source\":{\"kind\":\"user\"}}}\n",
        "{\"type\":\"assistant/message\",\"seq\":5,\"time\":1786629319700,\"data\":{\"turn\":1,\"step\":1,\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"The session is valid\"}],\"source\":{\"kind\":\"model\",\"provider\":\"deepseek-official\",\"model\":\"deepseek-test\"}}}}\n",
    );
    std::fs::write(
        &path,
        zstd::stream::encode_all(Cursor::new(content.as_bytes()), 1).unwrap(),
    )
    .unwrap();
}

fn seed_factory(home: &Path) {
    let root = home.join(".factory/sessions/-workspace-project");
    write_text(
        &root.join("droid-session-1.jsonl"),
        concat!(
            "{\"role\":\"user\",\"timestamp\":\"2026-08-23T00:00:00Z\",\"content\":[{\"type\":\"text\",\"text\":\"Inspect the Droid session\"}]}\n",
            "{\"type\":\"assistant\",\"timestamp\":\"2026-08-23T00:00:01Z\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-test\",\"content\":[{\"type\":\"text\",\"text\":\"Running a check\"}]}}\n"
        ),
    );
    write_text(&root.join("droid-session-1.settings.json"), "{}");
}

fn seed_kimi(home: &Path) {
    write_text(
        &home.join(".kimi/sessions/project-hash/kimi-session-1/wire.jsonl"),
        concat!(
            "{\"type\":\"metadata\",\"protocol_version\":1}\n",
            "{\"timestamp\":1787000000.0,\"message\":{\"type\":\"UserMessage\",\"payload\":{\"message_id\":\"user-1\",\"content\":[{\"type\":\"text\",\"text\":\"Inspect the Kimi wire\"}]}}}\n",
            "{\"timestamp\":1787000003.0,\"message\":{\"type\":\"AssistantMessage\",\"payload\":{\"message_id\":\"assistant-1\",\"content\":[{\"type\":\"text\",\"text\":\"The wire is readable\"}]}}}\n"
        ),
    );
    write_text(
        &home.join(".kimi/kimi.json"),
        r#"{"work_dirs":[{"last_session_id":"kimi-session-1","path":"/workspace/kimi"}]}"#,
    );
}

fn seed_grok(home: &Path) {
    write_text(
        &home.join(".grok/sessions/%2Fworkspace%2Fgrok/grok-session-1/updates.jsonl"),
        concat!(
            "{\"timestamp\":1787100000,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-user\"},\"update\":{\"sessionUpdate\":\"user_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"Inspect the Grok updates\"}}}}\n",
            "{\"timestamp\":1787100001,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-agent-1\"},\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"The update is readable\"}}}}\n"
        ),
    );
    write_text(
        &home.join(".grok/sessions/%2Fworkspace%2Fgrok/grok-session-1/summary.json"),
        r#"{"current_model_id":"grok-test"}"#,
    );
}

fn seed_cursor(home: &Path) {
    write_text(
        &home
            .join(".cursor/projects/Users-workspace-project/agent-transcripts/sess-parent/sess-parent.jsonl"),
        concat!(
            "{\"role\":\"user\",\"timestamp\":\"2026-08-22T00:00:00Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Inspect the project\"}]}}\n",
            "{\"role\":\"assistant\",\"timestamp\":\"2026-08-22T00:00:01Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Reading now\"}]}}\n"
        ),
    );
}

fn seed_qwen(home: &Path) {
    write_text(
        &home.join(".qwen/tmp/%2Fworkspace%2Fqwen/logs.json"),
        &serde_json::json!([
            {
                "sessionId": "qwen-session-1",
                "messageId": 0,
                "type": "user",
                "message": "Inspect the Qwen log",
                "timestamp": "2026-08-21T09:00:00.000Z"
            },
            {
                "sessionId": "qwen-session-1",
                "messageId": 1,
                "type": "user",
                "message": "Second body is not metadata",
                "timestamp": "2026-08-21T09:01:00.000Z"
            }
        ])
        .to_string(),
    );
}

fn seed_opencode(home: &Path) {
    let path = home.join(".local/share/opencode/opencode.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        r#"
        CREATE TABLE session (
            id TEXT PRIMARY KEY,
            parent_id TEXT,
            title TEXT,
            directory TEXT,
            time_created INTEGER,
            time_updated INTEGER
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            time_created INTEGER,
            data TEXT NOT NULL
        );
        CREATE TABLE part (
            id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            time_created INTEGER,
            data TEXT NOT NULL
        );
        "#,
    )
    .unwrap();
    db.execute(
        "INSERT INTO session VALUES(?1, NULL, ?2, ?3, ?4, ?5)",
        params![
            "ses-usage",
            "Inspect OpenCode",
            "/workspace/opencode",
            1_780_000_000_000_i64,
            1_780_000_003_000_i64
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO message VALUES(?1, ?2, ?3, ?4)",
        params![
            "msg-user",
            "ses-usage",
            1_780_000_000_000_i64,
            serde_json::json!({"role":"user","time":{"created":1_780_000_000_000_i64}}).to_string()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO part VALUES(?1, ?2, ?3, ?4, ?5)",
        params![
            "part-user",
            "msg-user",
            "ses-usage",
            1_780_000_000_100_i64,
            serde_json::json!({"type":"text","text":"Read the manifest"}).to_string()
        ],
    )
    .unwrap();
}

#[test]
fn claude_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1.jsonl",
        "claude-conversation.jsonl",
    );
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Claude);
    assert_conversation_index_matches_parse(&conn, home, "claude", "claude-parent-1");
}

#[test]
fn pi_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".pi/agent/sessions/pi-session-1.jsonl",
        "pi-conversation.jsonl",
    );
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Pi);
    assert_conversation_index_matches_parse(&conn, home, "pi", "pi-session-1");
}

#[test]
fn omp_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".omp/agent/sessions/omp-session-1.jsonl",
        "omp-conversation.jsonl",
    );
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Omp);
    assert_conversation_index_matches_parse(&conn, home, "omp", "omp-session-1");
}

#[test]
fn omp_subagent_jsonl_is_not_a_catalog_row() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let parent_stem = "2026-08-31T10-00-00-000Z_omp-session-1";
    write_home_fixture(
        home,
        &format!(".omp/agent/sessions/-workspace-app/{parent_stem}.jsonl"),
        "omp-conversation.jsonl",
    );
    let scout = home.join(format!(
        ".omp/agent/sessions/-workspace-app/{parent_stem}/Scout.jsonl"
    ));
    std::fs::create_dir_all(scout.parent().unwrap()).unwrap();
    std::fs::write(
        &scout,
        concat!(
            r#"{"type":"session","version":3,"id":"scout-1","timestamp":"2026-09-02T09:01:00Z","cwd":"/workspace/omp-app"}"#,
            "\n",
            r#"{"type":"message","id":"s-user","timestamp":"2026-09-02T09:01:01Z","message":{"role":"user","content":[{"type":"text","text":"scout"}]}}"#,
            "\n",
        ),
    )
    .unwrap();
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Omp);
    let page = crate::conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let omp_rows: Vec<_> = page.rows.iter().filter(|row| row.source == "omp").collect();
    assert_eq!(omp_rows.len(), 1);
    assert_eq!(omp_rows[0].session_id, "omp-session-1");
}

#[test]
fn gemini_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".gemini/tmp/gemini-project/chats/session-gemini-session-1.json",
        "gemini-conversation.json",
    );
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Gemini);
    assert_conversation_index_matches_parse(&conn, home, "gemini", "gemini-session-1");
}

#[test]
fn copilot_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".copilot/session-state/copilot-session-1/events.jsonl",
        "copilot-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Copilot);
    assert_conversation_index_matches_parse(
        &conn,
        home,
        "copilot",
        "c0ffee11-2222-4333-8444-555566667777",
    );
}

#[test]
fn cursor_agent_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_cursor(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::CursorAgent);
    assert_conversation_index_matches_parse(&conn, home, "cursor_agent", "sess-parent");
}

#[test]
fn dsh_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_dsh(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Dsh);
    assert_conversation_index_matches_parse(&conn, home, "dsh", "dsh-session-1");
}

#[test]
fn factory_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_factory(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Factory);
    assert_conversation_index_matches_parse(&conn, home, "factory", "droid-session-1");
}

#[test]
fn kimi_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_kimi(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Kimi);
    assert_conversation_index_matches_parse(&conn, home, "kimi", "kimi-session-1");
}

#[test]
fn grok_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_grok(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Grok);
    assert_conversation_index_matches_parse(&conn, home, "grok", "grok-session-1");
}

#[test]
fn qwen_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_qwen(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Qwen);
    assert_conversation_index_matches_parse(&conn, home, "qwen", "qwen-session-1");
}

#[test]
fn opencode_event_index_matches_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_opencode(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Opencode);
    assert_conversation_index_matches_parse(&conn, home, "opencode", "ses-usage");
}

#[test]
fn event_index_write_survives_when_another_source_fails() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".codex/sessions/2026/08/rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
    let claude = write_home_fixture(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1.jsonl",
        "claude-conversation.jsonl",
    );
    std::fs::write(&claude, "{not-json\n").unwrap();
    let conn = store::open_memory().unwrap();

    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();

    assert_conversation_index_matches_parse(&conn, home, "codex", "semantic-1");
    assert!(
        crate::conversation::indexed_events(&conn, "claude", "claude-parent-1")
            .unwrap()
            .is_empty(),
        "失败来源不得挡住其它来源的索引写入"
    );
}

#[test]
fn event_index_preserves_parent_child_agent_relations() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1.jsonl",
        "claude-conversation.jsonl",
    );
    write_home_fixture(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1/subagents/agent-claude-child-1.jsonl",
        "claude-subagent-conversation.jsonl",
    );
    write_home_fixture(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1/subagents/agent-claude-child-2.jsonl",
        "claude-subagent-conversation-2.jsonl",
    );
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Claude);

    let parent =
        crate::conversation::load_detail(&conn, home, "claude", "claude-parent-1").unwrap();
    assert_eq!(parent.agent_relations.children.len(), 2);
    let page = crate::conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(page
        .rows
        .iter()
        .all(|row| row.session_id != "claude-child-1" && row.session_id != "claude-child-2"));

    assert_conversation_index_matches_parse(&conn, home, "claude", "claude-parent-1");
    assert_conversation_index_matches_parse(&conn, home, "claude", "claude-child-1");
    assert_conversation_index_matches_parse(&conn, home, "claude", "claude-child-2");
}

#[test]
fn cursor_event_index_preserves_parent_child_agent_relations() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_cursor(home);
    write_text(
        &home.join(
            ".cursor/projects/Users-workspace-project/agent-transcripts/sess-parent/subagents/child-1.jsonl",
        ),
        concat!(
            "{\"role\":\"user\",\"timestamp\":\"2026-08-22T00:00:00.500Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Check the child task\"}]}}\n",
            "{\"role\":\"assistant\",\"timestamp\":\"2026-08-22T00:00:01.500Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Child complete\"}]}}\n"
        ),
    );
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::CursorAgent);

    let parent =
        crate::conversation::load_detail(&conn, home, "cursor_agent", "sess-parent").unwrap();
    assert_eq!(parent.agent_relations.children.len(), 1);
    assert_eq!(
        parent.agent_relations.children[0]
            .session
            .as_ref()
            .map(|session| session.session_id.as_str()),
        Some("child-1")
    );
    let page = crate::conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(page.rows.iter().all(|row| row.session_id != "child-1"));

    assert_conversation_index_matches_parse(&conn, home, "cursor_agent", "sess-parent");
    assert_conversation_index_matches_parse(&conn, home, "cursor_agent", "child-1");
}

#[test]
fn claude_event_index_matches_full_parse_across_split_source_files() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".claude/projects/-workspace-split/claude-split-a.jsonl",
        "claude-split-session-a.jsonl",
    );
    write_home_fixture(
        home,
        ".claude/projects/-workspace-split/claude-split-b.jsonl",
        "claude-split-session-b.jsonl",
    );
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Claude);
    assert_conversation_index_matches_parse(&conn, home, "claude", "claude-split-1");

    let texts = crate::conversation::indexed_events(&conn, "claude", "claude-split-1")
        .unwrap()
        .into_iter()
        .filter_map(|event| event.text)
        .collect::<Vec<_>>();
    assert_eq!(texts, vec!["early", "shared", "late"]);
}
