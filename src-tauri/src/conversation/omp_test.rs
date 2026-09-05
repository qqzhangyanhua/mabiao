use super::{parse, EventActor, EventKind};

#[test]
fn adapter_maps_omp_lifecycle_and_skips_duplicate_tool_starts() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("omp-lifecycle.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","id":"omp-lifecycle","timestamp":"2026-08-31T14:04:33Z","cwd":"/workspace"}"#,
            "\n",
            r#"{"type":"title","title":"Deep scan"}"#,
            "\n",
            r#"{"type":"custom","customType":"tool_execution_start","data":{"toolCallId":"call-1","toolName":"todo","intent":"Init tasks"},"timestamp":"2026-08-31T14:05:21Z"}"#,
            "\n",
            r#"{"type":"custom","customType":"session_exit","data":{"reason":"sighup","kind":"signal"},"timestamp":"2026-08-31T14:06:00Z"}"#,
            "\n",
            r#"{"type":"credential_pin","provider":"xai-oauth","timestamp":"2026-08-31T14:05:22Z"}"#,
            "\n",
            r#"{"type":"message","timestamp":"2026-08-31T14:05:23Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"call-1","name":"todo","arguments":{"text":"plan"}}]}}"#,
            "\n",
            r#"{"type":"future_omp","payload":{"secret":"keep-unadapted"}}"#,
            "\n",
        ),
    )
    .unwrap();

    let parsed = parse(&path, false).unwrap();
    assert_eq!(parsed.session.session_id, "omp-lifecycle");
    assert_eq!(parsed.session.title, "Deep scan");
    assert_eq!(
        parsed
            .events
            .iter()
            .filter(|event| event.kind == EventKind::ToolCall)
            .count(),
        1
    );
    assert!(parsed.events.iter().any(|event| {
        event.kind == EventKind::SystemStatus
            && event.name.as_deref() == Some("session_exit")
            && event.text.as_deref() == Some("sighup")
    }));
    assert!(parsed.events.iter().any(|event| {
        event.kind == EventKind::SystemStatus
            && event.name.as_deref() == Some("credential_pin")
            && event.text.as_deref() == Some("xai-oauth")
    }));
    assert!(parsed.events.iter().any(|event| {
        event.kind == EventKind::Unadapted && event.name.as_deref() == Some("future_omp")
    }));
    assert!(parsed
        .events
        .iter()
        .all(|event| event.name.as_deref() != Some("tool_execution_start")));
}

#[test]
fn adapter_maps_failed_omp_tool_result_as_error_with_tool_name() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("omp-tool-fail.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","id":"omp-tool-fail","timestamp":"2026-08-31T14:04:33Z","cwd":"/workspace"}"#,
            "\n",
            r#"{"type":"message","timestamp":"2026-08-31T14:05:23Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"call-1","name":"bash","arguments":{"command":"false"}}]}}"#,
            "\n",
            r#"{"type":"message","timestamp":"2026-08-31T14:05:24Z","message":{"role":"toolResult","toolCallId":"call-1","toolName":"bash","isError":true,"content":[{"type":"text","text":"boom"}]}}"#,
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
    assert_eq!(failed.name.as_deref(), Some("bash"));
    assert_eq!(failed.actor, Some(EventActor::Tool));
    assert_eq!(failed.text.as_deref(), Some("boom"));
    assert!(parsed
        .events
        .iter()
        .all(|event| event.kind != EventKind::ToolResult));
}
