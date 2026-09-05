use super::{parse, tool_payload_failed, EventActor, EventKind};

#[test]
fn adapter_maps_redacted_thinking_as_plan() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("claude-redacted.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"user","sessionId":"claude-redacted","timestamp":"2026-09-01T10:00:00Z","message":{"role":"user","content":"hello"}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"claude-redacted","timestamp":"2026-09-01T10:00:01Z","message":{"role":"assistant","model":"claude-sonnet-test","content":[{"type":"redacted_thinking"},{"type":"text","text":"done"}]}}"#,
            "\n",
        ),
    )
    .unwrap();

    let parsed = parse(&path, false).unwrap();
    assert_eq!(parsed.session.session_id, "claude-redacted");
    let redacted = parsed
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("redacted_thinking"))
        .unwrap();
    assert_eq!(redacted.kind, EventKind::Plan);
    assert!(parsed
        .events
        .iter()
        .all(|event| event.kind != EventKind::Unadapted));
}

#[test]
fn adapter_maps_failed_tool_result_as_error_with_tool_name() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("claude-tool-fail.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"user","sessionId":"claude-tool-fail","timestamp":"2026-09-01T10:00:00Z","cwd":"/workspace","message":{"role":"user","content":"run"}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"claude-tool-fail","timestamp":"2026-09-01T10:00:01Z","message":{"role":"assistant","model":"claude-sonnet-test","content":[{"type":"tool_use","id":"tool-1","name":"Bash","input":{"command":"false"}}]}}"#,
            "\n",
            r#"{"type":"user","sessionId":"claude-tool-fail","timestamp":"2026-09-01T10:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","is_error":true,"content":"Exit code 1"}]}}"#,
            "\n",
            r#"{"type":"future_claude","payload":{"secret":"keep-unadapted"}}"#,
            "\n",
        ),
    )
    .unwrap();

    let parsed = parse(&path, false).unwrap();
    let failed = parsed
        .events
        .iter()
        .find(|event| event.kind == EventKind::Error)
        .unwrap();
    assert_eq!(failed.name.as_deref(), Some("Bash"));
    assert_eq!(failed.actor, Some(EventActor::Tool));
    assert_eq!(failed.text.as_deref(), Some("Exit code 1"));
    assert!(parsed.events.iter().any(|event| {
        event.kind == EventKind::Unadapted && event.name.as_deref() == Some("future_claude")
    }));
    assert!(parsed
        .events
        .iter()
        .all(|event| event.kind != EventKind::ToolResult));
}

#[test]
fn tool_payload_failed_covers_status_error_and_is_error_flag() {
    assert!(tool_payload_failed(
        &serde_json::json!({ "is_error": true })
    ));
    assert!(tool_payload_failed(
        &serde_json::json!({ "status": "error" })
    ));
    assert!(tool_payload_failed(
        &serde_json::json!({ "status": "failed" })
    ));
    assert!(!tool_payload_failed(
        &serde_json::json!({ "status": "completed" })
    ));
}
