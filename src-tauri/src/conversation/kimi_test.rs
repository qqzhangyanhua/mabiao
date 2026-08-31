use super::*;

fn seed(temp: &Path, status: &str, include_update: bool) -> PathBuf {
    let root = temp.join("kimi");
    let path = root.join("sessions/hash/kimi-native-id/wire.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut content = format!(
        "{{\"timestamp\":1787000000.0,\"message\":{{\"type\":\"StatusUpdate\",\"payload\":{{\"message_id\":\"status-native\",\"status\":\"{status}\"}}}}}}\n{{\"timestamp\":1787000001.0,\"message\":{{\"type\":\"FutureWire\",\"payload\":{{\"secret_body\":\"raw generic body\"}}}}}}\n"
    );
    if include_update {
        content.push_str("{\"timestamp\":1787000002.0,\"message\":{\"type\":\"StatusUpdate\",\"payload\":{\"message_id\":\"status-native\",\"status\":\"done\"}}}\n");
    }
    std::fs::write(&path, content).unwrap();
    std::fs::write(root.join("kimi.json"), "{\"work_dirs\":[]}").unwrap();
    path
}

#[test]
fn adapter_merges_kimi_native_identity_and_keeps_unknown_json_out_of_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    let path = seed(temp.path(), "working", false);
    let first = index(&path).unwrap();
    let first_status = first.conversations[0]
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("working"))
        .unwrap();
    let stable_id = first_status.event_id.clone();

    seed(temp.path(), "working", true);
    let updated = index(&path).unwrap();
    let statuses = updated.conversations[0]
        .events
        .iter()
        .filter(|event| matches!(event.name.as_deref(), Some("working" | "done")))
        .collect::<Vec<_>>();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].name.as_deref(), Some("done"));
    assert_eq!(statuses[0].event_id, stable_id);
    let unknown = updated.conversations[0]
        .events
        .iter()
        .find(|event| event.kind == EventKind::Unadapted)
        .unwrap();
    assert!(unknown.details.to_string().contains("raw generic body"));
    assert!(updated
        .diagnostics
        .iter()
        .all(|issue| !issue.message.contains("raw generic body")));

    std::fs::write(&path, "{not-json\n").unwrap();
    assert!(index(&path).is_err());
}

#[test]
fn adapter_maps_high_frequency_wire_kinds() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("kimi");
    let path = root.join("sessions/hash/kimi-lifecycle/wire.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines = [
        serde_json::json!({"timestamp":1.0,"message":{"type":"TurnBegin","payload":{"user_input":[{"type":"text","text":"hello"}]}}}),
        serde_json::json!({"timestamp":2.0,"message":{"type":"ContentPart","payload":{"type":"think","think":"consider the greeting"}}}),
        serde_json::json!({"timestamp":3.0,"message":{"type":"ContentPart","payload":{"type":"text","text":"hi there"}}}),
        serde_json::json!({"timestamp":4.0,"message":{"type":"StepBegin","payload":{"n":1}}}),
        serde_json::json!({"timestamp":5.0,"message":{"type":"ApprovalRequest","payload":{"description":"Run ls","action":"run command"}}}),
        serde_json::json!({"timestamp":6.0,"message":{"type":"FutureWire","payload":{"secret_body":"raw generic body"}}}),
    ];
    let content = lines
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, content).unwrap();
    std::fs::write(root.join("kimi.json"), "{\"work_dirs\":[]}").unwrap();

    let parsed = index(&path).unwrap();
    let events = &parsed.conversations[0].events;
    assert!(events.iter().any(|event| {
        event.kind == EventKind::Message && event.text.as_deref() == Some("hello")
    }));
    assert!(events.iter().any(|event| {
        event.kind == EventKind::Plan && event.text.as_deref() == Some("consider the greeting")
    }));
    assert!(events.iter().any(|event| {
        event.kind == EventKind::Message && event.text.as_deref() == Some("hi there")
    }));
    assert!(events.iter().any(|event| {
        event.kind == EventKind::SystemStatus && event.name.as_deref() == Some("StepBegin")
    }));
    assert!(events.iter().any(|event| {
        event.kind == EventKind::SystemStatus
            && event.name.as_deref() == Some("ApprovalRequest")
            && event.text.as_deref() == Some("Run ls")
    }));
    assert!(events
        .iter()
        .any(|event| event.kind == EventKind::Unadapted));
}
