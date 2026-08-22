use std::path::{Path, PathBuf};

use crate::conversation;
use crate::domain::{ConversationEventKind, ConversationQuery};
use crate::test_support::*;

fn write_text(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn grok_updates() -> &'static str {
    concat!(
        "{\"timestamp\":1787100000,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-user\"},\"update\":{\"sessionUpdate\":\"user_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"Inspect the Grok updates\"}}}}\n",
        "{\"timestamp\":1787100001,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-agent-1\"},\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"The update \"}}}}\n",
        "{\"timestamp\":1787100002,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-agent-2\"},\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"is readable\"}}}}\n",
        "{\"timestamp\":1787100003,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-thought-1\"},\"update\":{\"sessionUpdate\":\"agent_thought_chunk\",\"content\":{\"type\":\"text\",\"text\":\"Check \"}}}}\n",
        "{\"timestamp\":1787100004,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-thought-2\"},\"update\":{\"sessionUpdate\":\"agent_thought_chunk\",\"content\":{\"type\":\"text\",\"text\":\"the stream\"}}}}\n",
        "{\"timestamp\":1787100005,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-tool\"},\"update\":{\"sessionUpdate\":\"tool_call\",\"toolCallId\":\"call-grok\",\"title\":\"read\",\"rawInput\":{\"path\":\"README.md\"}}}}\n",
        "{\"timestamp\":1787100006,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-tool-result\"},\"update\":{\"sessionUpdate\":\"tool_call_update\",\"toolCallId\":\"call-grok\",\"status\":\"completed\",\"content\":[{\"type\":\"content\",\"content\":{\"type\":\"text\",\"text\":\"file contents\"}}]}}}\n",
        "{\"timestamp\":1787100006.5,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-agent-3\"},\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"A separate reply\"}}}}\n",
        "{\"timestamp\":1787100007,\"method\":\"_x.ai/session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-turn-1\"},\"update\":{\"sessionUpdate\":\"turn_completed\",\"prompt_id\":\"prompt-1\",\"stop_reason\":\"running\",\"usage\":{\"inputTokens\":10,\"outputTokens\":2,\"totalTokens\":12,\"modelUsage\":{\"grok-test\":{\"inputTokens\":10,\"outputTokens\":2,\"totalTokens\":12}}}}}}\n",
        "{\"timestamp\":1787100008,\"method\":\"_x.ai/session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-turn-2\"},\"update\":{\"sessionUpdate\":\"turn_completed\",\"prompt_id\":\"prompt-1\",\"stop_reason\":\"end_turn\",\"usage\":{\"inputTokens\":12,\"outputTokens\":4,\"totalTokens\":16,\"modelUsage\":{\"grok-test\":{\"inputTokens\":12,\"outputTokens\":4,\"totalTokens\":16}}}}}}\n",
        "{\"timestamp\":1787100009,\"method\":\"session/update\",\"params\":{\"_meta\":{\"eventId\":\"event-future\"},\"update\":{\"sessionUpdate\":\"future_update\",\"secret_body\":\"generic Grok payload\"}}}\n"
    )
}

fn seed_grok_conversation(home: &Path, model: &str) -> (PathBuf, PathBuf) {
    let updates = home.join(".grok/sessions/%2Fworkspace%2Fgrok/grok-session-1/updates.jsonl");
    write_text(&updates, grok_updates());
    let summary = updates.parent().unwrap().join("summary.json");
    write_text(
        &summary,
        &serde_json::json!({ "current_model_id": model }).to_string(),
    );
    (updates, summary)
}

fn seed_kimi_conversation(home: &Path, project: &str) -> (PathBuf, PathBuf) {
    let session_id = "kimi-session-1";
    let wire = home.join(format!(
        ".kimi/sessions/project-hash/{session_id}/wire.jsonl"
    ));
    write_text(
        &wire,
        concat!(
            "{\"type\":\"metadata\",\"protocol_version\":1}\n",
            "{\"timestamp\":1787000000.0,\"message\":{\"type\":\"UserMessage\",\"payload\":{\"message_id\":\"user-1\",\"content\":[{\"type\":\"text\",\"text\":\"Inspect the Kimi wire\"}]}}}\n",
            "{\"timestamp\":1787000001.0,\"message\":{\"type\":\"ToolCall\",\"payload\":{\"tool_call_id\":\"call-kimi\",\"name\":\"read\",\"arguments\":{\"path\":\"README.md\"}}}}\n",
            "{\"timestamp\":1787000002.0,\"message\":{\"type\":\"ToolResult\",\"payload\":{\"tool_call_id\":\"call-kimi\",\"content\":\"file contents\"}}}\n",
            "{\"timestamp\":1787000003.0,\"message\":{\"type\":\"AssistantMessage\",\"payload\":{\"message_id\":\"assistant-1\",\"content\":[{\"type\":\"text\",\"text\":\"The wire is readable\"}]}}}\n",
            "{\"timestamp\":1787000004.0,\"message\":{\"type\":\"StatusUpdate\",\"payload\":{\"message_id\":\"status-1\",\"status\":\"working\",\"token_usage\":{\"input_other\":10,\"output\":2,\"input_cache_read\":3,\"input_cache_creation\":0}}}}\n",
            "{\"timestamp\":1787000005.0,\"message\":{\"type\":\"StatusUpdate\",\"payload\":{\"message_id\":\"status-1\",\"status\":\"done\",\"token_usage\":{\"input_other\":12,\"output\":4,\"input_cache_read\":3,\"input_cache_creation\":1}}}}\n",
            "{\"timestamp\":1787000006.0,\"message\":{\"type\":\"FutureWire\",\"payload\":{\"secret_body\":\"generic Kimi payload\"}}}\n"
        ),
    );
    let sidecar = home.join(".kimi/kimi.json");
    write_text(
        &sidecar,
        &serde_json::json!({
            "work_dirs": [{ "last_session_id": session_id, "path": project }]
        })
        .to_string(),
    );
    (wire, sidecar)
}

#[test]
fn configured_kimi_and_grok_roots_feed_the_unified_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("empty-home");
    let external_home = temp.path().join("external-sources");
    seed_kimi_conversation(&external_home, "/workspace/configured-kimi");
    seed_grok_conversation(&external_home, "configured-grok-model");
    let overrides = ingest::PathOverrides::from([
        ("KIMI_DATA_DIR", vec![external_home.join(".kimi")]),
        ("GROK_HOME", vec![external_home.join(".grok")]),
    ]);
    let conn = store::open_memory().unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, &home, &overrides).unwrap();
    assert!(report.files_failed == 0, "unexpected report: {report:?}");
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(page.rows.iter().any(|row| {
        row.source == "kimi"
            && row.session_id == "kimi-session-1"
            && row.project == "/workspace/configured-kimi"
    }));
    assert!(page.rows.iter().any(|row| {
        row.source == "grok"
            && row.session_id == "grok-session-1"
            && row.model == "configured-grok-model"
    }));
}

#[test]
fn kimi_wire_feeds_experimental_detail_and_tracks_trusted_sidecar_revision() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (wire, sidecar) = seed_kimi_conversation(home, "/workspace/kimi-one");
    let conn = store::open_memory().unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(report.files_failed == 0, "unexpected report: {report:?}");
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let session = page
        .rows
        .iter()
        .find(|row| row.source == "kimi" && row.session_id == "kimi-session-1")
        .unwrap();
    assert_eq!(session.project, "/workspace/kimi-one");
    assert_eq!(session.support_status, "experimental");
    assert_eq!(session.capabilities, ["messages", "events", "usage"]);

    let detail = conversation::load_detail(&conn, home, "kimi", "kimi-session-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Inspect the Kimi wire".to_string(),
            "The wire is readable".to_string()
        ]
    );
    let usage = usage_rows(&conn, "kimi", "kimi-session-1");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].input_tokens, 12);
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolCall
            && event.name.as_deref() == Some("read")
            && event
                .details
                .get("call_id")
                .and_then(serde_json::Value::as_str)
                == Some("call-kimi")
    }));
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolResult
            && event.text.as_deref() == Some("file contents")
    }));
    let statuses = detail
        .events
        .iter()
        .filter(|event| event.name.as_deref() == Some("done"))
        .collect::<Vec<_>>();
    assert_eq!(statuses.len(), 1);
    assert!(!detail
        .events
        .iter()
        .any(|event| event.name.as_deref() == Some("working")));
    let unknown = detail
        .events
        .iter()
        .find(|event| event.kind == ConversationEventKind::Unadapted)
        .unwrap();
    assert!(unknown.details.to_string().contains("generic Kimi payload"));
    let degraded = detail
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("capability_degraded"))
        .unwrap();
    assert_eq!(
        degraded.details.get("missing").unwrap(),
        &serde_json::json!(["model", "timestamp"])
    );

    seed_kimi_conversation(home, "/workspace/kimi-two");
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let refreshed = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(refreshed.rows.iter().any(|row| {
        row.source == "kimi"
            && row.session_id == "kimi-session-1"
            && row.project == "/workspace/kimi-two"
    }));

    std::fs::write(&sidecar, "{not-json").unwrap();
    let broken = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(broken.conversation_issues.iter().any(|issue| {
        issue.source == "kimi"
            && issue.path.ends_with("wire.jsonl")
            && !issue.message.contains("generic Kimi payload")
    }));
    let retained = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(retained.rows.iter().any(|row| {
        row.source == "kimi"
            && row.session_id == "kimi-session-1"
            && row.project == "/workspace/kimi-two"
    }));

    seed_kimi_conversation(home, "/workspace/kimi-two");
    std::fs::write(&wire, "{not-json\n").unwrap();
    let bad_wire = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(bad_wire.conversation_issues.iter().any(|issue| {
        issue.source == "kimi"
            && issue.path.ends_with("wire.jsonl")
            && !issue.message.contains("generic Kimi payload")
    }));
    let retained = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(retained.rows.iter().any(|row| {
        row.source == "kimi"
            && row.session_id == "kimi-session-1"
            && row.project == "/workspace/kimi-two"
    }));
}

#[test]
fn grok_updates_merge_streams_and_track_summary_without_changing_usage_identity() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let (updates, summary) = seed_grok_conversation(home, "grok-summary-one");
    let conn = store::open_memory().unwrap();

    let report = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(report.files_failed == 0, "unexpected report: {report:?}");
    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let session = page
        .rows
        .iter()
        .find(|row| row.source == "grok" && row.session_id == "grok-session-1")
        .unwrap();
    assert_eq!(session.project, "/workspace/grok");
    assert_eq!(session.model, "grok-summary-one");
    assert_eq!(session.support_status, "experimental");
    assert_eq!(session.capabilities, ["messages", "events", "usage"]);

    let detail = conversation::load_detail(&conn, home, "grok", "grok-session-1").unwrap();
    assert_eq!(
        message_texts(&detail),
        vec![
            "Inspect the Grok updates".to_string(),
            "The update is readable".to_string(),
            "A separate reply".to_string()
        ]
    );
    let usage = usage_rows(&conn, "grok", "grok-session-1");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].input_tokens, 12);
    assert!(!detail
        .events
        .iter()
        .any(|event| event.name.as_deref() == Some("capability_degraded")));
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::Plan
            && event.text.as_deref() == Some("Check the stream")
    }));
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolCall
            && event.name.as_deref() == Some("read")
            && event
                .details
                .get("call_id")
                .and_then(serde_json::Value::as_str)
                == Some("call-grok")
    }));
    assert!(detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolResult
            && event.text.as_deref() == Some("file contents")
    }));
    let turns = detail
        .events
        .iter()
        .filter(|event| event.name.as_deref() == Some("turn_completed"))
        .collect::<Vec<_>>();
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0]
            .details
            .get("stop_reason")
            .and_then(serde_json::Value::as_str),
        Some("end_turn")
    );
    let unknown = detail
        .events
        .iter()
        .find(|event| event.kind == ConversationEventKind::Unadapted)
        .unwrap();
    assert!(unknown.details.to_string().contains("generic Grok payload"));
    assert_eq!(
        detail
            .events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        detail.events.len()
    );

    seed_grok_conversation(home, "grok-summary-two");
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let refreshed = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(refreshed.rows.iter().any(|row| {
        row.source == "grok"
            && row.session_id == "grok-session-1"
            && row.model == "grok-summary-two"
    }));

    std::fs::write(&summary, "{not-json").unwrap();
    let bad_sidecar = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(bad_sidecar.conversation_issues.iter().any(|issue| {
        issue.source == "grok"
            && issue.path.ends_with("updates.jsonl")
            && !issue.message.contains("generic Grok payload")
    }));
    let retained = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(retained.rows.iter().any(|row| {
        row.source == "grok"
            && row.session_id == "grok-session-1"
            && row.model == "grok-summary-two"
    }));

    write_text(
        &summary,
        &serde_json::json!({ "current_model_id": "grok-summary-two" }).to_string(),
    );
    std::fs::write(&updates, "{not-json\n").unwrap();
    let bad_updates = ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    assert!(bad_updates.conversation_issues.iter().any(|issue| {
        issue.source == "grok"
            && issue.path.ends_with("updates.jsonl")
            && !issue.message.contains("generic Grok payload")
    }));
    let retained = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(retained.rows.iter().any(|row| {
        row.source == "grok"
            && row.session_id == "grok-session-1"
            && row.model == "grok-summary-two"
    }));
}
