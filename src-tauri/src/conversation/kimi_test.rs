use std::path::{Path, PathBuf};

use super::{index, EventKind};

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
fn finish_prep_assigns_kimi_native_event_ids() {
    let temp = tempfile::tempdir().unwrap();
    let path = seed(temp.path(), "working", false);
    let status = index(&path).unwrap().conversations[0]
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("working"))
        .unwrap()
        .event_id
        .clone();
    assert!(
        status.starts_with("kimi:kimi-native-id:"),
        "Kimi 钩子应分配原生事件 id，得到 {status}"
    );
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

#[test]
fn agents_layout_shares_session_id_and_skips_usage_records() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("kimi-code");
    let path = root.join("sessions/wd/sess/agents/main/wire.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines = [
        serde_json::json!({"timestamp":1.0,"message":{"type":"UserMessage","payload":{"message_id":"user-1","content":[{"type":"text","text":"hello from kimi code"}]}}}),
        serde_json::json!({"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":10,"output":2,"inputCacheRead":4,"inputCacheCreation":1},"time":1710000005000_i64}),
        serde_json::json!({"type":"forked","agentId":"main","time":1710000004000_i64}),
        serde_json::json!({"type":"turn.prompt","time":1710000007000_i64,"input":[{"type":"text","text":"still unadapted"}]}),
    ];
    let content = lines
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, content).unwrap();
    std::fs::write(
        root.join("kimi.json"),
        r#"{"work_dirs":[{"last_session_id":"sess","path":"/from/kimi-json"}]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("sessions/wd/sess/state.json"),
        r#"{"cwd":"/from/state","workDir":"/ignored"}"#,
    )
    .unwrap();

    let parsed = index(&path).unwrap();
    let conversation = &parsed.conversations[0];
    assert_eq!(conversation.session.session_id, "sess");
    assert_eq!(conversation.session.project, "/from/state");
    assert!(conversation.events.iter().any(|event| {
        event.kind == EventKind::Message && event.text.as_deref() == Some("hello from kimi code")
    }));
    assert!(conversation
        .events
        .iter()
        .any(|event| event.event_id.starts_with("kimi:sess:")));
    assert!(conversation.events.iter().any(|event| {
        event.kind == EventKind::Message && event.text.as_deref() == Some("still unadapted")
    }));
    assert!(parsed.diagnostics.is_empty());

    let sub = root.join("sessions/wd/sess/agents/agent-0/wire.jsonl");
    std::fs::create_dir_all(sub.parent().unwrap()).unwrap();
    std::fs::write(
        &sub,
        "{\"type\":\"usage.record\",\"model\":\"kimi-code/k3\",\"usage\":{\"inputOther\":1,\"output\":1,\"inputCacheRead\":0,\"inputCacheCreation\":0},\"time\":1710000008000}\n",
    )
    .unwrap();
    let sub_parsed = index(&sub).unwrap();
    assert_eq!(sub_parsed.conversations[0].session.session_id, "sess");
    assert_eq!(sub_parsed.conversations[0].session.project, "/from/state");
    assert!(sub_parsed.diagnostics.is_empty());
}

#[test]
fn kimi_code_wire_keeps_prompt_and_reply_inside_the_time_window() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("kimi-code");
    let path = root.join("sessions/wd/sess/agents/main/wire.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines = [
        serde_json::json!({"type":"metadata","protocol_version":"1.5","created_at":1710000000000_i64}),
        serde_json::json!({"type":"runtime.set_binding","time":1710000000275_i64}),
        serde_json::json!({"type":"profile.bind","systemPrompt":"secret system prompt","time":1710000000311_i64}),
        serde_json::json!({"type":"turn.prompt","input":[{"type":"text","text":"who are you"}],"origin":{"kind":"user"},"time":1710000000328_i64}),
        serde_json::json!({"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"who are you"}],"origin":{"kind":"user"}},"time":1710000000329_i64}),
        serde_json::json!({"type":"agent.message.appended","message":{"message":{"role":"user","content":[{"type":"text","text":"who are you"}]}},"time":1710000000325_i64}),
        serde_json::json!({"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"injected clock"}],"origin":{"kind":"injection","variant":"date_change"}},"time":1710000000340_i64}),
        serde_json::json!({"type":"llm.request","model":"deepseek-flash","time":1710000000346_i64}),
        serde_json::json!({"type":"usage.record","model":"kimi-code/k3","time":1710000001509_i64}),
        serde_json::json!({"type":"context.append_loop_event","event":{"type":"step.begin","step":1},"time":1710000000343_i64}),
        serde_json::json!({"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"think","think":"check identity"}},"time":1710000001510_i64}),
        serde_json::json!({"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":"I am Kimi"}},"time":1710000001510_i64}),
        serde_json::json!({"type":"context.append_loop_event","event":{"type":"step.end","finishReason":"stop"},"time":1710000001511_i64}),
        serde_json::json!({"type":"agent.message.appended","message":{"message":{"role":"assistant","content":[{"type":"text","text":"I am Kimi"}]}},"time":1710000001510_i64}),
        serde_json::json!({"type":"prompt.completed","finishedAt":"2024-03-09T16:00:07Z","time":1710000007000_i64}),
        serde_json::json!({"type":"future.event","secret_body":"must not enter diagnostics"}),
    ];
    let content = lines
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, content).unwrap();
    std::fs::write(
        root.join("sessions/wd/sess/state.json"),
        r#"{"cwd":"D:\\AI\\mabiao"}"#,
    )
    .unwrap();

    let parsed = index(&path).unwrap();
    let conversation = &parsed.conversations[0];
    assert_eq!(conversation.session.project, "D:\\AI\\mabiao");
    assert_eq!(conversation.session.model, "deepseek-flash");
    assert_eq!(conversation.session.started_at, "2024-03-09T16:00:00+00:00");
    assert_eq!(conversation.session.ended_at, "2024-03-09T16:00:07+00:00");
    assert_eq!(conversation.session.title, "who are you");

    let messages = conversation
        .events
        .iter()
        .filter(|event| event.kind == EventKind::Message)
        .map(|event| event.text.clone().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        vec!["who are you".to_string(), "I am Kimi".to_string()]
    );
    assert!(conversation.events.iter().any(|event| {
        event.kind == EventKind::Plan && event.text.as_deref() == Some("check identity")
    }));
    let rendered = conversation
        .events
        .iter()
        .map(|event| event.text.clone().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!rendered.contains("injected clock"));
    assert!(!rendered.contains("secret system prompt"));
    assert!(!rendered.contains("must not enter diagnostics"));
    assert_eq!(parsed.diagnostics.len(), 1);
    assert!(parsed.diagnostics[0].message.contains("future.event"));
    assert!(parsed
        .diagnostics
        .iter()
        .all(|issue| !issue.message.contains("must not enter diagnostics")));
    assert!(super::source_revision(&path).unwrap().ends_with(":wire-v2"));
}
