use std::path::Path;

use serde_json::Value;

use super::{index, EventKind};

fn write_events(path: &Path, prepend_unknown: bool) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut lines = vec![
        serde_json::json!({
            "type": "tool.execution_start",
            "id": "event-tool",
            "timestamp": "2026-08-22T09:00:00Z",
            "data": { "tool_call_id": "call-one", "tool": "view" }
        }),
        serde_json::json!({
            "type": "tool.execution_complete",
            "id": "event-result",
            "timestamp": "2026-08-22T09:00:01Z",
            "data": { "tool_call_id": "call-one", "result": "ok" }
        }),
        serde_json::json!({
            "type": "session.shutdown",
            "id": "event-shutdown",
            "timestamp": "2026-08-22T09:01:00Z",
            "data": { "currentModel": "gpt-test", "codeChanges": { "linesAdded": 1 } }
        }),
        serde_json::json!({
            "type": "future.copilot",
            "id": "event-future",
            "timestamp": "2026-08-22T09:02:00Z",
            "data": { "secret_body": "raw copilot body" }
        }),
    ];
    if prepend_unknown {
        lines.insert(
            0,
            serde_json::json!({
                "type": "future.copilot",
                "id": "event-prefix",
                "timestamp": "2026-08-22T08:59:00Z",
                "data": { "secret_body": "prepended copilot body" }
            }),
        );
    }
    let content = lines
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, content).unwrap();
}

#[test]
fn adapter_uses_copilot_parent_identity_and_stable_event_ids() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp
        .path()
        .join("session-state/copilot-parent-id/events.jsonl");
    write_events(&path, false);

    let first = index(&path).unwrap();
    let conversation = &first.conversations[0];
    assert_eq!(conversation.session.session_id, "copilot-parent-id");
    assert_eq!(conversation.session.model, "gpt-test");
    assert_eq!(conversation.session.capabilities, ["events", "usage"]);
    let stable_id = conversation
        .events
        .iter()
        .find(|event| event.kind == EventKind::ToolCall)
        .unwrap()
        .event_id
        .clone();

    write_events(&path, true);
    let updated = index(&path).unwrap();
    let conversation = &updated.conversations[0];
    assert_eq!(
        conversation
            .events
            .iter()
            .find(|event| event.kind == EventKind::ToolCall)
            .unwrap()
            .event_id,
        stable_id
    );
    assert!(conversation.events.iter().any(|event| {
        event.kind == EventKind::Unadapted && event.details.to_string().contains("raw copilot body")
    }));
    assert!(updated
        .diagnostics
        .iter()
        .all(|issue| !issue.message.contains("copilot body")));
    let degraded = conversation
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("capability_degraded"))
        .unwrap();
    assert!(degraded
        .details
        .get("missing")
        .and_then(Value::as_array)
        .unwrap()
        .contains(&Value::String("usage".to_string())));

    std::fs::write(&path, "{not-json\n").unwrap();
    assert!(index(&path).is_err());
}
