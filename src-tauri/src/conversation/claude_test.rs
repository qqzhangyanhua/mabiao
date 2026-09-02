use super::*;

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
