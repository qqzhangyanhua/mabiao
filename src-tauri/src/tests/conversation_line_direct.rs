use std::io::Cursor;
use std::path::Path;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rusqlite::params;

use crate::domain::{ConversationEvent, Source};
use crate::test_support::*;

fn test_png_bytes() -> Vec<u8> {
    let pixels = image::RgbaImage::from_pixel(2, 2, image::Rgba([24, 160, 200, 255]));
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

fn seed_codex_with_image(home: &Path) -> String {
    let attachment = home.join("attachments/screenshot.png");
    std::fs::create_dir_all(attachment.parent().unwrap()).unwrap();
    std::fs::write(&attachment, test_png_bytes()).unwrap();
    let path = home.join(".codex/sessions/2026/08/rollout-attach-1.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let records = [
        serde_json::json!({
            "type": "session_meta",
            "timestamp": "2026-08-24T00:00:00Z",
            "payload": {"id": "attach-1", "cwd": home, "title": "附件会话"}
        }),
        serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-24T00:00:01Z",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "查看附件"},
                    {
                        "type": "input_image",
                        "file_path": attachment,
                        "name": "screenshot.png",
                        "mime_type": "image/png"
                    }
                ]
            }
        }),
    ];
    let transcript = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, format!("{transcript}\n")).unwrap();
    "attach-1".to_string()
}

fn mark_session_unready(conn: &rusqlite::Connection, source: &str, session_id: &str) {
    conn.execute(
        "DELETE FROM conversation_events WHERE source = ?1 AND session_id = ?2",
        rusqlite::params![source, session_id],
    )
    .unwrap();
    conn.execute(
        r#"
        UPDATE conversation_sessions
        SET adapter_version = 0, event_index_generation = NULL
        WHERE source = ?1 AND session_id = ?2
        "#,
        rusqlite::params![source, session_id],
    )
    .unwrap();
    conn.execute(
        r#"
        UPDATE conversation_session_files
        SET adapter_version = 0
        WHERE source = ?1 AND session_id = ?2
        "#,
        rusqlite::params![source, session_id],
    )
    .unwrap();
}

fn conversation_roots(home: &std::path::Path, source: Source) -> Vec<std::path::PathBuf> {
    if source == Source::CursorAgent {
        vec![home.join(".cursor/projects")]
    } else {
        ingest::source_scan_dirs(home, source)
    }
}

fn refresh_source(conn: &rusqlite::Connection, home: &std::path::Path, source: Source) {
    crate::conversation::refresh_source_in_roots(conn, source, &conversation_roots(home, source))
        .unwrap();
}

fn event_without_global_sequence(mut event: ConversationEvent) -> ConversationEvent {
    event.sequence = event.source_sequence;
    event
}

fn assert_line_rebuild_matches_or_falls_back(
    conn: &rusqlite::Connection,
    home: &std::path::Path,
    source: &str,
    session_id: &str,
) -> (usize, usize) {
    let parsed =
        crate::conversation::parse_session_events(conn, home, source, session_id, true).unwrap();
    assert!(
        !parsed.is_empty(),
        "{source}/{session_id} 整份解析应至少有一条事件"
    );

    let mut matched = 0usize;
    let mut fell_back = 0usize;
    for event in &parsed {
        let rebuilt = crate::conversation::rebuild_events_from_line(
            Source::parse(source).expect("supported source"),
            std::path::Path::new(&event.source_file),
            session_id,
            event.source_sequence,
            true,
        );
        let line_hit = rebuilt.ok().and_then(|events| {
            events
                .into_iter()
                .find(|candidate| candidate.event_id == event.event_id)
        });
        if let Some(rebuilt) = line_hit {
            if event_without_global_sequence(rebuilt)
                == event_without_global_sequence(event.clone())
            {
                matched += 1;
                continue;
            }
        }

        let content = crate::conversation::load_event_content(
            conn,
            home,
            source,
            session_id,
            &event.event_id,
        )
        .unwrap();
        assert_eq!(content.event_id, event.event_id);
        assert_eq!(content.text, event.text);
        assert_eq!(content.details, event.details);
        fell_back += 1;
    }

    assert!(
        matched + fell_back == parsed.len(),
        "{source}/{session_id} 每条事件必须按行命中或走回退"
    );
    (matched, fell_back)
}

fn seed_codex(home: &std::path::Path) {
    write_home_fixture(
        home,
        ".codex/sessions/2026/08/rollout-semantic-1.jsonl",
        "codex-semantic-events.jsonl",
    );
}

fn seed_claude(home: &std::path::Path) {
    write_home_fixture(
        home,
        ".claude/projects/-workspace-claude-app/claude-parent-1.jsonl",
        "claude-conversation.jsonl",
    );
}

fn seed_pi(home: &std::path::Path) {
    write_home_fixture(
        home,
        ".pi/agent/sessions/pi-session-1.jsonl",
        "pi-conversation.jsonl",
    );
}

fn seed_gemini(home: &std::path::Path) {
    write_home_fixture(
        home,
        ".gemini/tmp/gemini-project/chats/session-gemini-session-1.json",
        "gemini-conversation.json",
    );
}

#[test]
fn codex_line_rebuild_matches_full_parse_event_fields() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_codex(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Codex);
    let (matched, _) =
        assert_line_rebuild_matches_or_falls_back(&conn, home, "codex", "semantic-1");
    assert!(matched > 0, "codex 已确认的无上下文响应事件必须能按行直取");
}

#[test]
fn claude_line_rebuild_matches_full_parse_event_fields() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_claude(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Claude);
    assert_line_rebuild_matches_or_falls_back(&conn, home, "claude", "claude-parent-1");
}

#[test]
fn pi_line_rebuild_matches_full_parse_event_fields() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_pi(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Pi);
    assert_line_rebuild_matches_or_falls_back(&conn, home, "pi", "pi-session-1");
}

#[test]
fn gemini_line_rebuild_falls_back_to_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_gemini(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Gemini);
    assert_line_rebuild_matches_or_falls_back(&conn, home, "gemini", "gemini-session-1");

    let parsed =
        crate::conversation::parse_session_events(&conn, home, "gemini", "gemini-session-1", true)
            .unwrap();
    let event = parsed.first().expect("gemini fixture has events");
    let error = crate::conversation::rebuild_events_from_line(
        Source::Gemini,
        std::path::Path::new(&event.source_file),
        "gemini-session-1",
        event.source_sequence,
        true,
    )
    .unwrap_err();
    assert!(
        error.contains("不是按行无上下文"),
        "整份 JSON 来源必须拒绝按行直取：{error}"
    );
}

#[test]
fn unindexed_session_still_loads_event_content_and_attachments() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let session_id = seed_codex_with_image(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Codex);
    mark_session_unready(&conn, "codex", &session_id);

    let parsed =
        crate::conversation::parse_session_events(&conn, home, "codex", &session_id, true).unwrap();
    let event = parsed
        .iter()
        .find(|event| !event.attachments.is_empty())
        .expect("附件会话应有带图的事件");
    let content =
        crate::conversation::load_event_content(&conn, home, "codex", &session_id, &event.event_id)
            .unwrap();
    assert_eq!(content.event_id, event.event_id);
    assert_eq!(content.text, event.text);
    assert_eq!(content.details, event.details);

    let image = crate::conversation::load_attachment(
        &conn,
        home,
        "codex",
        &session_id,
        &event.attachments[0].id,
    )
    .unwrap();
    assert_eq!(image.attachment, event.attachments[0]);
    assert_eq!(
        image.data_url,
        format!("data:image/png;base64,{}", BASE64.encode(test_png_bytes()))
    );
    let thumbnail = crate::conversation::load_attachment_thumbnail(
        &conn,
        home,
        "codex",
        &session_id,
        &event.attachments[0].id,
    )
    .unwrap();
    assert_eq!(thumbnail.attachment, event.attachments[0]);
}

#[test]
fn same_line_multiple_events_are_located_by_occurrence() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_claude(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Claude);

    let parsed =
        crate::conversation::parse_session_events(&conn, home, "claude", "claude-parent-1", true)
            .unwrap();
    let mut by_line = std::collections::BTreeMap::<u32, Vec<&ConversationEvent>>::new();
    for event in &parsed {
        by_line
            .entry(event.source_sequence)
            .or_default()
            .push(event);
    }
    let (line, events) = by_line
        .into_iter()
        .find(|(_, events)| events.len() > 1)
        .expect("claude assistant line produces multiple events");
    assert!(events.len() >= 2, "第 {line} 行应产出多条事件");

    let rebuilt = crate::conversation::rebuild_events_from_line(
        Source::Claude,
        std::path::Path::new(&events[0].source_file),
        "claude-parent-1",
        line,
        true,
    )
    .unwrap();
    assert_eq!(
        rebuilt.len(),
        events.len(),
        "按行重建必须保留同一行的全部事件"
    );
    for event in events {
        let content = crate::conversation::load_event_content(
            &conn,
            home,
            "claude",
            "claude-parent-1",
            &event.event_id,
        )
        .unwrap();
        assert_eq!(content.event_id, event.event_id);
        assert_eq!(content.text, event.text);
        assert_eq!(content.details, event.details);
    }
}

fn write_text(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
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
fn copilot_line_rebuild_falls_back_to_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_home_fixture(
        home,
        ".copilot/session-state/copilot-session-1/events.jsonl",
        "copilot-events.jsonl",
    );
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Copilot);
    let (_, fell_back) = assert_line_rebuild_matches_or_falls_back(
        &conn,
        home,
        "copilot",
        "c0ffee11-2222-4333-8444-555566667777",
    );
    assert!(fell_back > 0, "事件计数器来源应退回整份解析");
}

#[test]
fn cursor_line_rebuild_falls_back_to_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_cursor(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::CursorAgent);
    let (_, fell_back) =
        assert_line_rebuild_matches_or_falls_back(&conn, home, "cursor_agent", "sess-parent");
    assert!(fell_back > 0, "事件计数器来源应退回整份解析");
}

#[test]
fn dsh_line_rebuild_falls_back_to_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_dsh(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Dsh);
    let (_, fell_back) =
        assert_line_rebuild_matches_or_falls_back(&conn, home, "dsh", "dsh-session-1");
    assert!(fell_back > 0, "压缩会话来源应退回整份解析");
}

#[test]
fn factory_line_rebuild_falls_back_to_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_factory(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Factory);
    let (_, fell_back) =
        assert_line_rebuild_matches_or_falls_back(&conn, home, "factory", "droid-session-1");
    assert!(fell_back > 0, "事件计数器来源应退回整份解析");
}

#[test]
fn kimi_line_rebuild_falls_back_to_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_kimi(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Kimi);
    let (_, fell_back) =
        assert_line_rebuild_matches_or_falls_back(&conn, home, "kimi", "kimi-session-1");
    assert!(fell_back > 0, "事件计数器来源应退回整份解析");
}

#[test]
fn grok_line_rebuild_falls_back_to_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_grok(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Grok);
    let (_, fell_back) =
        assert_line_rebuild_matches_or_falls_back(&conn, home, "grok", "grok-session-1");
    assert!(fell_back > 0, "流式来源应退回整份解析");
}

#[test]
fn qwen_line_rebuild_falls_back_to_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_qwen(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Qwen);
    let (_, fell_back) =
        assert_line_rebuild_matches_or_falls_back(&conn, home, "qwen", "qwen-session-1");
    assert!(fell_back > 0, "整份 JSON 来源应退回整份解析");
}

#[test]
fn opencode_line_rebuild_falls_back_to_full_parse() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_opencode(home);
    let conn = store::open_memory().unwrap();
    refresh_source(&conn, home, Source::Opencode);
    let (_, fell_back) =
        assert_line_rebuild_matches_or_falls_back(&conn, home, "opencode", "ses-usage");
    assert!(fell_back > 0, "sqlite 来源应退回整份解析");
}
