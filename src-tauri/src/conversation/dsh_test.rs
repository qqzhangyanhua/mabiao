use std::io::Cursor;
use std::path::Path;

use super::{index, EventKind, EventStatus};

fn write_compressed(path: &Path, content: &str) {
    let compressed = zstd::stream::encode_all(Cursor::new(content.as_bytes()), 1).unwrap();
    std::fs::write(path, compressed).unwrap();
}

#[test]
fn adapter_requires_dsh_identity_and_degrades_unknown_records_without_bodies() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.jsonl.zstd");
    write_compressed(
        &path,
        concat!(
            "{\"type\":\"session\",\"id\":\"dsh-sparse\",\"cwd\":\"/workspace\"}\n",
            "{\"type\":\"user/message\",\"seq\":1,\"data\":{\"content\":[{\"type\":\"text\",\"text\":\"injected context\"}],\"source\":{\"kind\":\"plugin\",\"plugin\":\"test\"}}}\n",
            "{\"type\":\"tool/call\",\"seq\":2,\"data\":{\"callId\":\"missing-result\",\"name\":\"read\",\"arguments\":\"{}\"}}\n",
            "{\"type\":\"future/event\",\"seq\":3,\"secret_body\":\"must not enter diagnostics\"}\n"
        ),
    );

    let batch = index(&path).unwrap();
    let parsed = &batch.conversations[0];
    assert_eq!(parsed.session.session_id, "dsh-sparse");
    assert!(parsed.session.model.is_empty());
    assert!(parsed.messages.is_empty());
    let context = parsed
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("plugin"))
        .unwrap();
    assert_eq!(context.kind, EventKind::SystemStatus);
    assert_eq!(context.text.as_deref(), Some("injected context"));
    let degraded = parsed
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("capability_degraded"))
        .unwrap();
    assert_eq!(
        degraded.details.get("missing").unwrap(),
        &serde_json::json!(["user_message", "model", "tool_result", "timestamp"])
    );
    let call = parsed
        .events
        .iter()
        .find(|event| event.kind == EventKind::ToolCall)
        .unwrap();
    assert_eq!(call.capability_status, EventStatus::MissingTimestamp);
    assert!(!parsed
        .events
        .iter()
        .any(|event| event.kind == EventKind::ToolResult));
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

    write_compressed(&path, "{\"type\":\"session\",\"cwd\":\"/workspace\"}\n");
    assert!(index(&path).is_err());
}
