use super::*;

fn seed(temp: &Path, include_update: bool) -> PathBuf {
    let root = temp.join("grok/sessions/project/grok-native-id");
    let path = root.join("updates.jsonl");
    std::fs::create_dir_all(&root).unwrap();
    let mut content = concat!(
        "{\"timestamp\":1787100000,\"method\":\"_x.ai/session/update\",\"params\":{\"_meta\":{\"eventId\":\"turn-event-one\"},\"update\":{\"sessionUpdate\":\"turn_completed\",\"prompt_id\":\"prompt-native\",\"stop_reason\":\"running\"}}}\n",
        "{\"timestamp\":1787100001,\"method\":\"session/update\",\"params\":{\"_meta\":{\"eventId\":\"future-event\"},\"update\":{\"sessionUpdate\":\"future_update\",\"secret_body\":\"raw generic body\"}}}\n"
    )
    .to_string();
    if include_update {
        content.push_str("{\"timestamp\":1787100002,\"method\":\"_x.ai/session/update\",\"params\":{\"_meta\":{\"eventId\":\"turn-event-two\"},\"update\":{\"sessionUpdate\":\"turn_completed\",\"prompt_id\":\"prompt-native\",\"stop_reason\":\"end_turn\"}}}\n");
    }
    std::fs::write(&path, content).unwrap();
    std::fs::write(
        root.join("summary.json"),
        "{\"current_model_id\":\"grok-test\"}",
    )
    .unwrap();
    path
}

fn chunk_value(
    kind: &str,
    prompt_id: Option<&str>,
    event_id: &str,
    message_id: Option<&str>,
    text: &str,
) -> Value {
    let mut update = serde_json::json!({
        "sessionUpdate": kind,
        "content": { "type": "text", "text": text },
    });
    if let (Some(message_id), Value::Object(object)) = (message_id, &mut update) {
        object.insert(
            "message_id".to_string(),
            Value::String(message_id.to_string()),
        );
    }
    serde_json::json!({
        "params": {
            "_meta": { "promptId": prompt_id, "eventId": event_id },
            "update": update,
        }
    })
}

#[test]
fn chunk_aggregation_respects_native_ids_and_contiguous_fallback_boundaries() {
    let values = vec![
        (
            0,
            chunk_value("agent_message_chunk", Some("p"), "a1", None, "A"),
        ),
        (
            1,
            chunk_value("agent_message_chunk", Some("p"), "a2", None, "B"),
        ),
        (
            2,
            serde_json::json!({ "params": { "update": { "sessionUpdate": "tool_call" } } }),
        ),
        (
            3,
            chunk_value("agent_message_chunk", Some("p"), "a3", None, "C"),
        ),
        (
            4,
            chunk_value("agent_message_chunk", Some("q"), "a4", None, "D"),
        ),
        (
            5,
            chunk_value("agent_thought_chunk", Some("q"), "t1", None, "T"),
        ),
        (
            6,
            chunk_value("agent_message_chunk", Some("p"), "a5", Some("m1"), "E"),
        ),
        (
            7,
            serde_json::json!({ "params": { "update": { "sessionUpdate": "tool_call" } } }),
        ),
        (
            8,
            chunk_value("agent_message_chunk", Some("p"), "a6", Some("m1"), "F"),
        ),
        (9, chunk_value("user_message_chunk", None, "u1", None, "X")),
        (10, chunk_value("user_message_chunk", None, "u2", None, "Y")),
    ];

    let (line_keys, chunks) = aggregate_chunks(&values);
    assert_eq!(line_keys[&0], line_keys[&1]);
    assert_ne!(line_keys[&0], line_keys[&3]);
    assert_ne!(line_keys[&3], line_keys[&4]);
    assert_ne!(line_keys[&4], line_keys[&5]);
    assert_eq!(line_keys[&6], line_keys[&8]);
    assert_ne!(line_keys[&9], line_keys[&10]);
    assert_eq!(chunks[&line_keys[&0]].text, "AB");
    assert_eq!(chunks[&line_keys[&6]].text, "EF");
}

#[test]
fn adapter_merges_grok_prompt_identity_and_keeps_unknown_json_out_of_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    let path = seed(temp.path(), false);
    let first = index(&path).unwrap();
    let first_turn = first.conversations[0]
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("turn_completed"))
        .unwrap();
    let stable_id = first_turn.event_id.clone();

    seed(temp.path(), true);
    let updated = index(&path).unwrap();
    let turns = updated.conversations[0]
        .events
        .iter()
        .filter(|event| event.name.as_deref() == Some("turn_completed"))
        .collect::<Vec<_>>();
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0].details.get("stop_reason").and_then(Value::as_str),
        Some("end_turn")
    );
    assert_eq!(turns[0].event_id, stable_id);
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
fn adapter_maps_high_frequency_lifecycle_kinds() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("grok/sessions/project/grok-lifecycle");
    let path = root.join("updates.jsonl");
    std::fs::create_dir_all(&root).unwrap();
    let lines = [
        serde_json::json!({"timestamp":1,"method":"session/update","params":{"_meta":{"eventId":"h1"},"update":{"sessionUpdate":"hook_execution","event_name":"pre_tool_use","tool_name":"read_file","runs":[{"name":"hook-a","status":{"status":"success"}}]}}}),
        serde_json::json!({"timestamp":2,"method":"session/update","params":{"_meta":{"eventId":"t1"},"update":{"sessionUpdate":"task_backgrounded","description":"type-check","command":"pnpm test"}}}),
        serde_json::json!({"timestamp":3,"method":"session/update","params":{"_meta":{"eventId":"c1"},"update":{"sessionUpdate":"task_completed","task_snapshot":{"command":"pnpm test","output":"ok"}}}}),
        serde_json::json!({"timestamp":4,"method":"session/update","params":{"_meta":{"eventId":"r1"},"update":{"sessionUpdate":"retry_state","type":"retrying","reason":"request error"}}}),
        serde_json::json!({"timestamp":5,"method":"session/update","params":{"_meta":{"eventId":"s1"},"update":{"sessionUpdate":"session_recap","summary":"Closed the issues"}}}),
        serde_json::json!({"timestamp":6,"method":"session/update","params":{"_meta":{"eventId":"a1"},"update":{"sessionUpdate":"subagent_spawned","description":"Spec review","subagent_type":"general-purpose"}}}),
        serde_json::json!({"timestamp":7,"method":"session/update","params":{"_meta":{"eventId":"b1"},"update":{"sessionUpdate":"subagent_finished","status":"completed"}}}),
        serde_json::json!({"timestamp":8,"method":"session/update","params":{"_meta":{"eventId":"f1"},"update":{"sessionUpdate":"future_update","secret_body":"still unknown"}}}),
    ];
    let content = lines
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, content).unwrap();
    std::fs::write(
        root.join("summary.json"),
        "{\"current_model_id\":\"grok-test\"}",
    )
    .unwrap();

    let parsed = index(&path).unwrap();
    let events = &parsed.conversations[0].events;
    let hook = events
        .iter()
        .find(|event| event.name.as_deref() == Some("pre_tool_use"))
        .unwrap();
    assert_eq!(hook.kind, EventKind::SystemStatus);
    assert_eq!(hook.text.as_deref(), Some("read_file"));
    assert_eq!(
        events
            .iter()
            .find(|event| event.name.as_deref() == Some("task_backgrounded"))
            .and_then(|event| event.text.as_deref()),
        Some("type-check")
    );
    assert_eq!(
        events
            .iter()
            .find(|event| event.name.as_deref() == Some("task_completed"))
            .and_then(|event| event.text.as_deref()),
        Some("pnpm test")
    );
    let retry = events
        .iter()
        .find(|event| event.name.as_deref() == Some("retrying"))
        .unwrap();
    assert_eq!(retry.kind, EventKind::SystemStatus);
    assert_eq!(retry.text.as_deref(), Some("request error"));
    assert_eq!(
        events
            .iter()
            .find(|event| event.name.as_deref() == Some("session_recap"))
            .and_then(|event| event.text.as_deref()),
        Some("Closed the issues")
    );
    assert!(events.iter().any(|event| {
        event.kind == EventKind::SystemStatus && event.name.as_deref() == Some("subagent_spawned")
    }));
    assert!(events.iter().any(|event| {
        event.kind == EventKind::SystemStatus && event.name.as_deref() == Some("subagent_finished")
    }));
    let unknown = events
        .iter()
        .find(|event| event.kind == EventKind::Unadapted)
        .unwrap();
    assert_eq!(unknown.name.as_deref(), Some("future_update"));
    assert!(parsed
        .diagnostics
        .iter()
        .all(|issue| !issue.message.contains("hook_execution")));
    assert!(parsed
        .diagnostics
        .iter()
        .any(|issue| issue.message.contains("future_update")));
}
