use super::*;

#[test]
fn adapter_maps_lifecycle_kinds_and_typeless_messages() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pi-lifecycle.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","id":"pi-lifecycle","timestamp":"2026-07-13T09:09:03Z","cwd":"/workspace"}"#,
            "\n",
            r#"{"type":"thinking_level_change","timestamp":"2026-07-13T09:09:04Z","thinkingLevel":"high"}"#,
            "\n",
            r#"{"type":"title_change","timestamp":"2026-07-13T09:09:05Z","title":"处理需求"}"#,
            "\n",
            r#"{"type":"custom_message","customType":"subagent-notify","content":"Background task completed"}"#,
            "\n",
            r#"{"timestamp":"2026-07-13T09:09:06Z","role":"user","message":{"role":"user","content":[{"type":"text","text":"typeless prompt"}]}}"#,
            "\n",
            r#"{"type":"future_pi","payload":{"secret":"keep-unadapted"}}"#,
            "\n",
        ),
    )
    .unwrap();

    let parsed = parse(&path, false).unwrap();
    assert_eq!(parsed.session.session_id, "pi-lifecycle");
    assert_eq!(parsed.session.title, "处理需求");
    assert!(parsed.events.iter().any(|event| {
        event.kind == EventKind::SystemStatus
            && event.name.as_deref() == Some("thinking_level_change")
            && event.text.as_deref() == Some("high")
    }));
    assert!(parsed.events.iter().any(|event| {
        event.kind == EventKind::SystemStatus
            && event.name.as_deref() == Some("subagent-notify")
            && event.text.as_deref() == Some("Background task completed")
    }));
    assert!(parsed.events.iter().any(|event| {
        event.kind == EventKind::Message && event.text.as_deref() == Some("typeless prompt")
    }));
    assert!(parsed
        .events
        .iter()
        .any(|event| event.kind == EventKind::Unadapted
            && event.name.as_deref() == Some("future_pi")));
}
