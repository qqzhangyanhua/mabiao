use std::path::{Path, PathBuf};

use crate::conversation;
use crate::domain::{ConversationEventKind, ConversationExportFormat, ConversationQuery};
use crate::test_support::*;

fn write_text(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn seed_qwen_conversation(home: &Path) -> PathBuf {
    let path = home.join(".qwen/tmp/%2Fworkspace%2Fqwen/logs.json");
    let content = serde_json::json!([
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
        },
        {
            "sessionId": "qwen-session-1",
            "messageId": 2,
            "type": "future_qwen_record",
            "secret_body": "generic Qwen payload",
            "timestamp": "2026-08-21T09:02:00.000Z"
        },
        {
            "sessionId": "qwen-session-2",
            "messageId": 0,
            "type": "user",
            "message": "other session body",
            "timestamp": "2026-08-21T10:00:00.000Z"
        }
    ]);
    write_text(&path, &serde_json::to_string_pretty(&content).unwrap());
    path
}

fn seed_copilot_conversation(home: &Path) -> PathBuf {
    let path = home.join(".copilot/session-state/copilot-session-1/events.jsonl");
    let mut content = fixture("copilot-events.jsonl");
    content.push_str(
        "\n{\"type\":\"future.copilot_event\",\"id\":\"evt-future\",\"timestamp\":\"2026-08-10T15:13:00.000Z\",\"data\":{\"secret_body\":\"generic Copilot payload\"}}\n",
    );
    write_text(&path, &content);
    path
}

#[test]
fn configured_qwen_and_copilot_roots_feed_the_unified_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("empty-home");
    let external_home = temp.path().join("external-sources");
    seed_qwen_conversation(&external_home);
    seed_copilot_conversation(&external_home);
    let overrides = ingest::PathOverrides::from([
        ("QWEN_DATA_DIR", vec![external_home.join(".qwen")]),
        ("COPILOT_HOME", vec![external_home.join(".copilot")]),
    ]);
    let conn = store::open_memory().unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, &home, &overrides).unwrap();
    assert!(report.files_failed == 0, "unexpected report: {report:?}");
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(page
        .rows
        .iter()
        .any(|row| row.source == "qwen" && row.session_id == "qwen-session-1"));
    assert!(page.rows.iter().any(|row| {
        row.source == "copilot" && row.session_id == "c0ffee11-2222-4333-8444-555566667777"
    }));
}

#[test]
fn qwen_tokenless_log_feeds_partial_detail_export_search_and_missing_file_state() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let source_file = seed_qwen_conversation(home);
    let conn = store::open_memory().unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(report.files_failed == 0, "unexpected report: {report:?}");
    assert!(store::load_all(&conn)
        .unwrap()
        .iter()
        .all(|record| record.source.as_str() != "qwen"));
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let session = page
        .rows
        .iter()
        .find(|row| row.source == "qwen" && row.session_id == "qwen-session-1")
        .unwrap();
    assert_eq!(session.title, "Inspect the Qwen log");
    assert_eq!(session.project, "/workspace/qwen");
    assert_eq!(session.model, "");
    assert_eq!(session.capabilities, ["messages", "events"]);
    assert_eq!(session.support_status, "experimental");

    let detail = conversation::load_parsed_detail(&conn, home, "qwen", "qwen-session-1").unwrap();
    let messages = message_events(&detail);
    assert_eq!(messages.len(), 2);
    assert!(messages
        .iter()
        .all(|event| event.actor.map(ConversationEventActor::as_str) == Some("user")));
    assert!(usage_rows(&conn, "qwen", "qwen-session-1").is_empty());
    assert!(!messages
        .iter()
        .any(|event| event.actor.map(ConversationEventActor::as_str) == Some("assistant")));
    let degraded = detail
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("capability_degraded"))
        .unwrap();
    assert_eq!(
        degraded.details.get("missing").unwrap(),
        &serde_json::json!(["assistant_message", "model", "provider", "usage"])
    );
    let unknown = detail
        .events
        .iter()
        .find(|event| event.kind == ConversationEventKind::Unadapted)
        .unwrap();
    assert!(unknown.details.to_string().contains("generic Qwen payload"));
    assert!(report.conversation_issues.iter().all(|issue| {
        issue.source != "qwen" || !issue.message.contains("generic Qwen payload")
    }));

    let body_search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("Second body is not metadata".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(body_search.total, 0);
    let metadata_search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("qwen-session-1".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(metadata_search.total, 1);

    let markdown = conversation::build_export(
        &conn,
        home,
        "qwen",
        "qwen-session-1",
        ConversationExportFormat::Markdown,
    )
    .unwrap();
    let markdown = String::from_utf8(markdown.content).unwrap();
    assert!(markdown.contains("Inspect the Qwen log"));
    assert!(markdown.contains("generic Qwen payload"));
    let raw = conversation::build_export(
        &conn,
        home,
        "qwen",
        "qwen-session-1",
        ConversationExportFormat::Json,
    )
    .unwrap();
    assert!(raw.default_name.ends_with(".json"));
    let raw = String::from_utf8(raw.content).unwrap();
    assert!(raw.contains("generic Qwen payload"));
    assert!(!raw.contains("other session body"));

    std::fs::write(&source_file, "{not-json").unwrap();
    let broken = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(broken.conversation_issues.iter().any(|issue| {
        issue.source == "qwen"
            && issue.path.ends_with("logs.json")
            && !issue.message.contains("generic Qwen payload")
    }));
    let retained = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(retained.rows.iter().any(|row| {
        row.source == "qwen"
            && row.session_id == "qwen-session-1"
            && row.title == "Inspect the Qwen log"
            && row.file_available
    }));

    seed_qwen_conversation(home);
    std::fs::remove_file(source_file).unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let missing = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let missing = missing
        .rows
        .iter()
        .find(|row| row.source == "qwen" && row.session_id == "qwen-session-1")
        .unwrap();
    assert!(!missing.file_available);
    let error =
        conversation::load_parsed_detail(&conn, home, "qwen", "qwen-session-1").unwrap_err();
    assert!(error.contains("原文件已删除"), "unexpected error: {error}");
}

#[test]
fn copilot_events_feed_lifecycle_tools_code_changes_usage_and_missing_body_status() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let source_file = seed_copilot_conversation(home);
    let conn = store::open_memory().unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(report.files_failed == 0, "unexpected report: {report:?}");
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let session = page
        .rows
        .iter()
        .find(|row| {
            row.source == "copilot" && row.session_id == "c0ffee11-2222-4333-8444-555566667777"
        })
        .unwrap();
    assert_eq!(session.title, "c0ffee11-2222-4333-8444-555566667777");
    assert_eq!(session.project, "/Users/dev/ai-usage-stats");
    assert_eq!(session.model, "claude-sonnet-4.5");
    assert_eq!(session.capabilities, ["events", "usage"]);
    assert_eq!(session.support_status, "experimental");

    let detail = conversation::load_parsed_detail(
        &conn,
        home,
        "copilot",
        "c0ffee11-2222-4333-8444-555566667777",
    )
    .unwrap();
    assert!(message_texts(&detail).is_empty());
    let usage = usage_rows(&conn, "copilot", "c0ffee11-2222-4333-8444-555566667777");
    assert_eq!(usage.len(), 2);
    assert_eq!(usage[0].model, "claude-sonnet-4.5");
    assert_eq!(usage[0].input_tokens, 21_583);
    assert_eq!(usage[1].model, "gpt-5.4");
    assert_eq!(usage[1].input_tokens, 244_120);
    assert!(detail
        .events
        .iter()
        .all(|event| event.kind != ConversationEventKind::Message));
    assert_eq!(
        detail
            .events
            .iter()
            .filter(|event| event.name.as_deref() == Some("session_started"))
            .count(),
        2
    );
    let tool_call = detail
        .events
        .iter()
        .find(|event| event.kind == ConversationEventKind::ToolCall)
        .unwrap();
    assert_eq!(tool_call.name.as_deref(), Some("view"));
    assert_eq!(
        tool_call
            .details
            .get("call_id")
            .and_then(serde_json::Value::as_str),
        Some("call-1")
    );
    let tool_result = detail
        .events
        .iter()
        .find(|event| event.kind == ConversationEventKind::ToolResult)
        .unwrap();
    assert_eq!(tool_result.text.as_deref(), Some("ok"));
    let latest_shutdown = detail
        .events
        .iter()
        .rev()
        .find(|event| event.name.as_deref() == Some("session_shutdown"))
        .unwrap();
    assert_eq!(
        latest_shutdown
            .details
            .pointer("/codeChanges/linesAdded")
            .and_then(serde_json::Value::as_i64),
        Some(40)
    );
    assert_eq!(
        latest_shutdown
            .details
            .pointer("/codeChanges/filesModified/1")
            .and_then(serde_json::Value::as_str),
        Some("src/domain.rs")
    );
    let degraded = detail
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("capability_degraded"))
        .unwrap();
    assert_eq!(
        degraded.details.get("missing").unwrap(),
        &serde_json::json!(["user_message", "assistant_message"])
    );
    let unknown = detail
        .events
        .iter()
        .find(|event| event.kind == ConversationEventKind::Unadapted)
        .unwrap();
    assert!(unknown
        .details
        .to_string()
        .contains("generic Copilot payload"));
    assert!(report.conversation_issues.iter().all(|issue| {
        issue.source != "copilot" || !issue.message.contains("generic Copilot payload")
    }));

    let body_search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("generic Copilot payload".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(body_search.total, 0);
    let metadata_search = conversation::sessions_page(
        &conn,
        &ConversationQuery {
            search: Some("claude-sonnet-4.5".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(metadata_search.total, 1);

    let markdown = conversation::build_export(
        &conn,
        home,
        "copilot",
        "c0ffee11-2222-4333-8444-555566667777",
        ConversationExportFormat::Markdown,
    )
    .unwrap();
    let markdown = String::from_utf8(markdown.content).unwrap();
    assert!(markdown.contains("generic Copilot payload"));
    assert!(markdown.contains("linesAdded"));
    let raw = conversation::build_export(
        &conn,
        home,
        "copilot",
        "c0ffee11-2222-4333-8444-555566667777",
        ConversationExportFormat::Json,
    )
    .unwrap();
    assert!(raw.default_name.ends_with(".jsonl"));
    assert!(String::from_utf8(raw.content)
        .unwrap()
        .contains("generic Copilot payload"));

    std::fs::write(&source_file, "{not-json\n").unwrap();
    let broken = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(broken.conversation_issues.iter().any(|issue| {
        issue.source == "copilot"
            && issue.path.ends_with("events.jsonl")
            && !issue.message.contains("generic Copilot payload")
    }));
    let retained = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(retained.rows.iter().any(|row| {
        row.source == "copilot"
            && row.session_id == "c0ffee11-2222-4333-8444-555566667777"
            && row.model == "claude-sonnet-4.5"
            && row.file_available
    }));

    seed_copilot_conversation(home);
    std::fs::remove_file(source_file).unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let missing = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let missing = missing
        .rows
        .iter()
        .find(|row| {
            row.source == "copilot" && row.session_id == "c0ffee11-2222-4333-8444-555566667777"
        })
        .unwrap();
    assert!(!missing.file_available);
    let error = conversation::load_parsed_detail(
        &conn,
        home,
        "copilot",
        "c0ffee11-2222-4333-8444-555566667777",
    )
    .unwrap_err();
    assert!(error.contains("原文件已删除"), "unexpected error: {error}");
}
