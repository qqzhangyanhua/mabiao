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

#[test]
fn finish_prep_appends_inferred_capability_degradation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.jsonl.zstd");
    write_compressed(
        &path,
        concat!(
            "{\"type\":\"session\",\"id\":\"dsh-degrade\",\"cwd\":\"/workspace\"}\n",
            "{\"type\":\"tool/call\",\"seq\":1,\"data\":{\"callId\":\"missing-result\",\"name\":\"read\",\"arguments\":\"{}\"}}\n"
        ),
    );

    let parsed = &index(&path).unwrap().conversations[0];
    let call = parsed
        .events
        .iter()
        .find(|event| event.kind == EventKind::ToolCall)
        .unwrap();
    assert!(
        !call.event_id.starts_with("dsh:"),
        "只追加推断式能力降级时事件 id 应走溯源，得到 {}",
        call.event_id
    );
    let degraded = parsed
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("capability_degraded"))
        .unwrap();
    assert_eq!(
        degraded.details.get("missing").unwrap(),
        &serde_json::json!(["user_message", "model", "tool_result", "timestamp"])
    );
}

#[test]
fn discovery_reads_v4_files_and_skips_legacy_copies_next_to_them() {
    let temp = tempfile::tempdir().unwrap();
    let content = "{\"type\":\"session\",\"id\":\"dsh-v4\",\"cwd\":\"/workspace\"}\n";
    for (dir, files) in [
        ("both", vec!["session.jsonl.zstd", "session.v4.jsonl.zstd"]),
        ("legacy", vec!["session.jsonl.zstd"]),
        ("v4", vec!["session.v4.jsonl.zstd"]),
    ] {
        let dir = temp.path().join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for file in files {
            write_compressed(&dir.join(file), content);
        }
    }

    let found = crate::conversation::discover_dsh(&[temp.path().to_path_buf()]).unwrap();
    let relative: Vec<_> = found
        .iter()
        .map(|path| {
            path.strip_prefix(temp.path())
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        relative,
        [
            "both/session.v4.jsonl.zstd",
            "legacy/session.jsonl.zstd",
            "v4/session.v4.jsonl.zstd"
        ]
    );
}

#[test]
fn end_seed_written_at_save_time_does_not_stretch_the_session_range() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.v4.jsonl.zstd");
    write_compressed(
        &path,
        concat!(
            "{\"type\":\"session\",\"version\":4,\"id\":\"dsh-seeded\",\"createdAt\":1787000000000,\"cwd\":\"/workspace\"}\n",
            "{\"type\":\"user/message\",\"seq\":1,\"time\":1787000001000,\"data\":{\"content\":[{\"type\":\"text\",\"text\":\"hi\"}],\"source\":{\"kind\":\"user\"}}}\n",
            "{\"type\":\"session/end-seed\",\"seq\":2,\"time\":1790000000000,\"data\":{}}\n"
        ),
    );

    let batch = index(&path).unwrap();
    let session = &batch.conversations[0].session;
    assert_eq!(session.started_at, "2026-08-17T20:53:20+00:00");
    assert_eq!(session.ended_at, "2026-08-17T20:53:21+00:00");
}

#[test]
fn only_v4_files_get_the_range_revision_suffix() {
    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join("session.jsonl.zstd");
    let v4 = temp.path().join("session.v4.jsonl.zstd");
    write_compressed(&legacy, "{}\n");
    write_compressed(&v4, "{}\n");

    assert!(!super::source_revision(&legacy)
        .unwrap()
        .contains(":range-v2"));
    assert!(super::source_revision(&v4).unwrap().ends_with(":range-v2"));
}
