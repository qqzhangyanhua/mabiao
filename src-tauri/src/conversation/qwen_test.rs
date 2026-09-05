use std::path::Path;

use super::{detail, index, EventKind};

fn write_records(path: &Path, prepend_unknown: bool) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut records = vec![
        serde_json::json!({
            "sessionId": "qwen-one",
            "messageId": 7,
            "type": "user",
            "message": "first prompt",
            "timestamp": "2026-08-21T09:00:00Z"
        }),
        serde_json::json!({
            "sessionId": "qwen-one",
            "messageId": 8,
            "type": "future",
            "secret_body": "raw qwen body",
            "timestamp": "2026-08-21T09:01:00Z"
        }),
        serde_json::json!({
            "sessionId": "qwen-two",
            "messageId": 1,
            "type": "user",
            "message": "second session",
            "timestamp": "2026-08-21T09:02:00Z"
        }),
    ];
    if prepend_unknown {
        records.insert(
            0,
            serde_json::json!({
                "sessionId": "qwen-one",
                "messageId": 6,
                "type": "future",
                "secret_body": "prepended qwen body",
                "timestamp": "2026-08-21T08:59:00Z"
            }),
        );
    }
    std::fs::write(path, serde_json::to_vec(&records).unwrap()).unwrap();
}

#[test]
fn adapter_groups_qwen_sessions_and_uses_stable_message_identity() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("%2Fworkspace%2Fqwen/logs.json");
    write_records(&path, false);

    let first = index(&path).unwrap();
    assert_eq!(first.conversations.len(), 2);
    let first_session = first
        .conversations
        .iter()
        .find(|conversation| conversation.session.session_id == "qwen-one")
        .unwrap();
    assert_eq!(first_session.session.project, "/workspace/qwen");
    assert_eq!(first_session.session.capabilities, ["messages", "events"]);
    let stable_id = first_session
        .events
        .iter()
        .find(|event| event.kind == EventKind::Message)
        .unwrap()
        .event_id
        .clone();

    write_records(&path, true);
    let updated = index(&path).unwrap();
    let first_session = updated
        .conversations
        .iter()
        .find(|conversation| conversation.session.session_id == "qwen-one")
        .unwrap();
    assert_eq!(
        first_session
            .events
            .iter()
            .find(|event| event.kind == EventKind::Message)
            .unwrap()
            .event_id,
        stable_id
    );
    assert!(first_session.events.iter().any(|event| {
        event.kind == EventKind::Unadapted && event.details.to_string().contains("raw qwen body")
    }));
    assert!(updated
        .diagnostics
        .iter()
        .all(|issue| !issue.message.contains("qwen body")));

    let second = detail(&path, "qwen-two", false).unwrap();
    assert_eq!(second.messages.len(), 1);
    assert_eq!(second.messages[0].text, "second session");

    std::fs::write(&path, "{}").unwrap();
    assert!(index(&path).is_err());

    std::fs::write(&path, "[]").unwrap();
    let empty = index(&path).unwrap();
    assert!(empty.conversations.is_empty());
    assert!(empty.diagnostics.is_empty());
}
