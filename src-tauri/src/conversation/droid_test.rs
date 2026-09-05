use super::{index, EventKind, EventStatus};

#[test]
fn adapter_uses_droid_filename_identity_and_reports_partial_records_structurally() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("droid-sparse.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"call-1\",\"name\":\"Read\",\"input\":{\"path\":\"README.md\"}}]}\n",
            "{\"type\":\"future_record\",\"secret_body\":\"must not enter diagnostics\"}\n"
        ),
    )
    .unwrap();

    let batch = index(&path).unwrap();
    let parsed = &batch.conversations[0];
    assert_eq!(parsed.session.session_id, "droid-sparse");
    assert!(parsed.session.model.is_empty());
    assert!(parsed.messages.is_empty());
    let call = parsed
        .events
        .iter()
        .find(|event| event.kind == EventKind::ToolCall)
        .unwrap();
    assert_eq!(call.capability_status, EventStatus::MissingTimestamp);
    assert_eq!(call.text.as_deref(), Some("README.md"));
    let unknown = parsed
        .events
        .iter()
        .find(|event| event.kind == EventKind::Unadapted)
        .unwrap();
    assert!(!unknown
        .details
        .to_string()
        .contains("must not enter diagnostics"));
    assert!(batch
        .diagnostics
        .iter()
        .all(|issue| !issue.message.contains("must not enter diagnostics")));

    std::fs::write(&path, "{not-json\n").unwrap();
    assert!(index(&path).is_err());
}

#[test]
fn adapter_maps_session_start_todo_and_compaction_kinds() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("droid-lifecycle.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session_start","id":"droid-lifecycle","title":"清理端口"}"#,
            "\n",
            r#"{"type":"todo_state","timestamp":"2025-10-10T08:46:05.756Z","todos":{"todos":[{"content":"Find PID","status":"in_progress"},{"content":"Kill process","status":"pending"}]}}"#,
            "\n",
            r#"{"type":"compaction_state","timestamp":"2025-10-10T08:47:00Z","summaryText":"long summary"}"#,
            "\n",
            r#"{"type":"future_record","secret_body":"must not enter diagnostics"}"#,
            "\n",
        ),
    )
    .unwrap();

    let parsed = &index(&path).unwrap().conversations[0];
    assert_eq!(parsed.session.title, "清理端口");
    assert!(parsed.events.iter().any(|event| {
        event.kind == EventKind::SystemStatus && event.name.as_deref() == Some("session_started")
    }));
    let todo = parsed
        .events
        .iter()
        .find(|event| event.kind == EventKind::Plan)
        .unwrap();
    assert_eq!(todo.name.as_deref(), Some("todo_state"));
    assert_eq!(todo.text.as_deref(), Some("Find PID\nKill process"));
    assert!(parsed.events.iter().any(|event| {
        event.kind == EventKind::SystemStatus && event.name.as_deref() == Some("compaction_state")
    }));
    assert!(parsed
        .events
        .iter()
        .any(|event| event.kind == EventKind::Unadapted));
}
