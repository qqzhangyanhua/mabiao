use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::conversation;
use crate::domain::{ConversationEventCapabilityStatus, ConversationEventKind, ConversationQuery};
use crate::test_support::*;

fn write_text(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn seed_droid_conversations(home: &Path) -> PathBuf {
    let root = home.join(".factory/sessions/-workspace-project");
    let raw = root.join("droid-session-1.jsonl");
    write_text(
        &raw,
        concat!(
            "{\"role\":\"user\",\"timestamp\":\"2026-08-23T00:00:00Z\",\"content\":[{\"type\":\"text\",\"text\":\"Inspect the Droid session\"}]}\n",
            "{\"type\":\"assistant\",\"timestamp\":\"2026-08-23T00:00:01Z\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-test\",\"content\":[{\"type\":\"text\",\"text\":\"Running a check\"},{\"type\":\"tool_use\",\"id\":\"call-shell\",\"name\":\"Shell\",\"input\":{\"command\":\"cargo test\"}}]}}\n",
            "{\"type\":\"tool_result\",\"role\":\"user\",\"timestamp\":\"2026-08-23T00:00:02Z\",\"tool_use_id\":\"call-shell\",\"content\":\"tests passed\"}\n"
        ),
    );
    write_text(
        &root.join("droid-session-1.settings.json"),
        &serde_json::json!({
            "providerLock":"anthropic",
            "providerLockTimestamp":"2026-08-23T00:00:03Z",
            "tokenUsage":{
                "inputTokens":20,
                "outputTokens":7,
                "cacheCreationTokens":3,
                "cacheReadTokens":5,
                "thinkingTokens":2
            }
        })
        .to_string(),
    );
    write_text(
        &root.join("orphan.jsonl"),
        "{\"role\":\"user\",\"content\":\"must not be discovered without settings\"}\n",
    );
    write_text(&root.join("droid-sparse.settings.json"), "{}");
    write_text(
        &root.join("droid-sparse.jsonl"),
        "{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"call-missing\",\"name\":\"Read\",\"input\":{\"path\":\"README.md\"}}]}\n",
    );
    raw
}

fn seed_dsh_conversation(home: &Path) -> PathBuf {
    let path = home.join(".dsh/sessions/-workspace-project/session-dsh/session.jsonl.zstd");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let content = concat!(
        "{\"type\":\"session\",\"version\":0,\"id\":\"dsh-session-1\",\"createdAt\":1786629319248,\"cwd\":\"/workspace/project\"}\n",
        "{\"type\":\"request/header\",\"seq\":1,\"time\":1786629319300,\"data\":{\"header\":{\"config\":{\"provider\":\"deepseek-official\",\"model\":\"deepseek-test\"},\"reason\":\"initial\"}}}\n",
        "{\"type\":\"user/message\",\"seq\":2,\"time\":1786629319400,\"data\":{\"content\":[{\"type\":\"text\",\"text\":\"Inspect the compressed session\"}],\"source\":{\"kind\":\"user\"}}}\n",
        "{\"type\":\"tool/call\",\"seq\":3,\"time\":1786629319500,\"data\":{\"turn\":1,\"step\":1,\"callId\":\"call-read\",\"name\":\"read\",\"arguments\":\"{\\\"file_path\\\":\\\"src/lib.rs\\\"}\"}}\n",
        "{\"type\":\"tool/result\",\"seq\":4,\"time\":1786629319600,\"data\":{\"turn\":1,\"step\":1,\"message\":{\"role\":\"user\",\"source\":{\"kind\":\"tool\",\"callId\":\"call-read\"},\"content\":[{\"type\":\"tool-result\",\"toolCallId\":\"call-read\",\"content\":[{\"type\":\"text\",\"text\":\"file contents\"}]}]}}}\n",
        "{\"type\":\"assistant/message\",\"seq\":5,\"time\":1786629319700,\"data\":{\"turn\":1,\"step\":1,\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"The session is valid\"}],\"source\":{\"kind\":\"model\",\"provider\":\"deepseek-official\",\"model\":\"deepseek-test\"}},\"usage\":{\"inputTokens\":12,\"outputTokens\":5,\"cacheReadTokens\":3,\"reasoningTokens\":2}}}\n",
        "{\"type\":\"turn/end\",\"seq\":6,\"time\":1786629319800,\"data\":{\"turn\":1,\"reason\":{\"kind\":\"completed\"}}}\n"
    );
    let compressed = zstd::stream::encode_all(Cursor::new(content.as_bytes()), 1).unwrap();
    std::fs::write(&path, compressed).unwrap();
    path
}

#[test]
fn dsh_compressed_session_feeds_semantic_detail_and_preserves_last_good_index() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let source = seed_dsh_conversation(home);
    let conn = store::open_memory().unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(report.files_failed == 0, "unexpected report: {report:?}");
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let session = page
        .rows
        .iter()
        .find(|row| row.source == "dsh" && row.session_id == "dsh-session-1")
        .unwrap();
    assert_eq!(session.project, "/workspace/project");
    assert_eq!(session.model, "deepseek-test");
    assert_eq!(session.support_status, "experimental");

    let detail = conversation::load_detail(&conn, home, "dsh", "dsh-session-1").unwrap();
    assert_eq!(usage_rows(&conn, "dsh", "dsh-session-1").len(), 1);
    assert_eq!(
        message_texts(&detail),
        vec![
            "Inspect the compressed session".to_string(),
            "The session is valid".to_string()
        ]
    );
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolCall
            && event.name.as_deref() == Some("read")
            && event
                .details
                .get("call_id")
                .and_then(serde_json::Value::as_str)
                == Some("call-read")
    }));
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolResult
            && event.text.as_deref() == Some("file contents")
    }));
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::SystemStatus
            && event.name.as_deref() == Some("turn_completed")
    }));

    std::fs::write(&source, b"not a zstd stream").unwrap();
    let broken = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(
        broken.conversation_issues.iter().any(|issue| {
            issue.source == "dsh"
                && issue.path.ends_with("session.jsonl.zstd")
                && !issue.message.contains("Inspect the compressed session")
        }),
        "missing body-free DSH diagnostic: {broken:?}"
    );
    let retained = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(retained.rows.iter().any(|row| {
        row.source == "dsh" && row.session_id == "dsh-session-1" && row.file_available
    }));
}

#[test]
fn droid_raw_sessions_link_cumulative_usage_and_degrade_sparse_fields() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let source = seed_droid_conversations(home);
    let conn = store::open_memory().unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(report.files_failed == 0, "unexpected report: {report:?}");
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let droid_rows = page
        .rows
        .iter()
        .filter(|row| row.source == "factory")
        .collect::<Vec<_>>();
    assert_eq!(droid_rows.len(), 2);
    assert!(droid_rows
        .iter()
        .all(|row| row.support_status == "experimental"));

    let detail = conversation::load_detail(&conn, home, "factory", "droid-session-1").unwrap();
    assert_eq!(detail.session.project, "/workspace/project");
    assert_eq!(detail.session.model, "claude-test");
    let usage = usage_rows(&conn, "factory", "droid-session-1");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].session_id, "droid-session-1");
    assert_eq!(message_texts(&detail)[0], "Inspect the Droid session");
    assert_eq!(message_texts(&detail).len(), 2);
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolCall
            && event.name.as_deref() == Some("Shell")
            && event
                .details
                .get("call_id")
                .and_then(serde_json::Value::as_str)
                == Some("call-shell")
    }));
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolResult
            && event.text.as_deref() == Some("tests passed")
    }));

    let sparse = conversation::load_detail(&conn, home, "factory", "droid-sparse").unwrap();
    assert!(message_texts(&sparse).is_empty());
    assert!(sparse.session.model.is_empty());
    assert!(usage_rows(&conn, "factory", "droid-sparse").is_empty());
    let sparse_call = sparse
        .events
        .iter()
        .find(|event| event.kind == ConversationEventKind::ToolCall)
        .unwrap();
    assert_eq!(
        sparse_call.capability_status,
        ConversationEventCapabilityStatus::MissingTimestamp
    );
    let degraded = sparse
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("capability_degraded"))
        .unwrap();
    assert_eq!(
        degraded.details.get("missing").unwrap(),
        &serde_json::json!(["user_message", "model", "tool_result", "timestamp"])
    );
    assert!(!sparse
        .events
        .iter()
        .any(|event| event.kind == ConversationEventKind::ToolResult));

    std::fs::write(&source, "{not-json").unwrap();
    let broken = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(broken.conversation_issues.iter().any(|issue| {
        issue.source == "factory"
            && issue.path.ends_with("droid-session-1.jsonl")
            && !issue.message.contains("Inspect the Droid session")
    }));
    let retained = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(retained.rows.iter().any(|row| {
        row.source == "factory" && row.session_id == "droid-session-1" && row.file_available
    }));
}
