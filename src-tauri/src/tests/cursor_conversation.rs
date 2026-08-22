use std::path::{Path, PathBuf};

use crate::domain::{
    ConversationAgentLinkStatus, ConversationEventKind, ConversationQuery, CursorSessionSummaryDto,
    CursorSessionToolRow, Source,
};
use crate::test_support::*;
use crate::{conversation, cursor_session};

fn write_text(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn cursor_transcript_path(home: &Path, session_id: &str) -> PathBuf {
    home.join(".cursor/projects/Users-workspace-project/agent-transcripts")
        .join(session_id)
        .join(format!("{session_id}.jsonl"))
}

fn seed_cursor_usage(home: &Path, session_id: &str, captured_at: &str) {
    let path = home
        .join(".cursor-agent-usage")
        .join(format!("{session_id}.jsonl"));
    write_text(
        &path,
        &format!(
            "{}\n{}\n",
            serde_json::json!({
                "type":"system",
                "subtype":"init",
                "model":"cursor-test-model",
                "cwd":"/workspace/project",
                "session_id":session_id
            }),
            serde_json::json!({
                "type":"result",
                "subtype":"success",
                "is_error":false,
                "session_id":session_id,
                "request_id":format!("request-{session_id}"),
                "duration_ms":1000,
                "usage":{"inputTokens":10,"outputTokens":5,"cacheReadTokens":2,"cacheWriteTokens":1},
                "captured_at":captured_at
            })
        ),
    );
}

fn seed_cursor_conversations(home: &Path) -> PathBuf {
    let parent = cursor_transcript_path(home, "sess-parent");
    write_text(
        &parent,
        concat!(
            "{\"role\":\"user\",\"timestamp\":\"2026-08-22T00:00:00Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Inspect the project\"}]}}\n",
            "{\"role\":\"assistant\",\"timestamp\":\"2026-08-22T00:00:01Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Reading now\"},{\"type\":\"tool_use\",\"id\":\"call-read\",\"name\":\"Read\",\"input\":{\"path\":\"src/lib.rs\"}}]}}\n",
            "{\"type\":\"turn_ended\",\"timestamp\":\"2026-08-22T00:00:02Z\",\"status\":\"success\"}\n"
        ),
    );
    let child = parent
        .parent()
        .unwrap()
        .join("subagents")
        .join("child-1.jsonl");
    write_text(
        &child,
        concat!(
            "{\"role\":\"user\",\"timestamp\":\"2026-08-22T00:00:00.500Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Check the child task\"}]}}\n",
            "{\"role\":\"assistant\",\"timestamp\":\"2026-08-22T00:00:01.500Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Child complete\"}]}}\n",
            "{\"type\":\"turn_ended\",\"timestamp\":\"2026-08-22T00:00:01.750Z\",\"status\":\"success\"}\n"
        ),
    );
    let transcript_only = cursor_transcript_path(home, "sess-transcript-only");
    write_text(
        &transcript_only,
        "{\"role\":\"user\",\"timestamp\":\"2026-08-22T01:00:00Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"No token wrapper\"}]}}\n",
    );
    parent
}

fn behavior_metrics(
    summary: &CursorSessionSummaryDto,
) -> (i64, i64, i64, i64, Vec<CursorSessionToolRow>) {
    (
        summary.session_count,
        summary.turn_count,
        summary.user_prompt_count,
        summary.subagent_count,
        summary.top_tools.clone(),
    )
}

#[test]
fn cursor_transcripts_usage_and_behavior_feed_one_exact_conversation_detail() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let parent = seed_cursor_conversations(home);
    seed_cursor_usage(home, "sess-parent", "2026-08-22T00:00:03Z");
    seed_cursor_usage(home, "sess-usage-only", "2026-08-22T02:00:00Z");
    let conn = store::open_memory().unwrap();

    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let summary_before = cursor_session::load_summary(&conn).unwrap();
    let metrics_before = behavior_metrics(&summary_before);
    assert_eq!(summary_before.session_count, 2);
    assert_eq!(summary_before.subagent_count, 1);
    assert_eq!(summary_before.turn_count, 2);

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let cursor_rows = page
        .rows
        .iter()
        .filter(|row| row.source == Source::CursorAgent.as_str())
        .collect::<Vec<_>>();
    assert_eq!(cursor_rows.len(), 3);
    assert!(cursor_rows
        .iter()
        .any(|row| { row.session_id == "sess-parent" && row.model == "cursor-test-model" }));
    assert!(cursor_rows
        .iter()
        .any(|row| row.session_id == "sess-transcript-only" && row.file_available));
    assert!(cursor_rows
        .iter()
        .any(|row| row.session_id == "sess-usage-only" && !row.file_available));
    assert!(!cursor_rows.iter().any(|row| row.session_id == "child-1"));

    ingest::rebuild_cache(&conn, home, Some(Source::CursorAgent)).unwrap();
    assert_eq!(
        behavior_metrics(&cursor_session::load_summary(&conn).unwrap()),
        metrics_before
    );
    assert_eq!(
        conversation::sessions_page(&conn, &ConversationQuery::default())
            .unwrap()
            .rows
            .iter()
            .filter(|row| row.source == "cursor_agent")
            .count(),
        3
    );

    let parent_detail =
        conversation::load_detail(&conn, home, "cursor_agent", "sess-parent").unwrap();
    let parent_usage = usage_rows(&conn, "cursor_agent", "sess-parent");
    assert_eq!(parent_usage.len(), 1);
    assert_eq!(parent_usage[0].session_id, "sess-parent");
    assert!(message_texts(&parent_detail)
        .iter()
        .any(|text| text == "Inspect the project"));
    assert!(parent_detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::ToolCall && event.name.as_deref() == Some("Read")
    }));
    assert!(!parent_detail.events.iter().any(|event| {
        event.kind == ConversationEventKind::SystemStatus
            && event.name.as_deref() == Some("cursor_session_stats")
    }));
    let parent_behavior = parent_detail
        .cursor_behavior
        .as_ref()
        .expect("parent conversation should carry cursor behavior");
    assert_eq!(parent_behavior.session.session_id, "sess-parent");
    assert!(parent_behavior
        .tools
        .iter()
        .any(|tool| tool.name == "Read" && tool.call_count >= 1));
    assert!(parent_behavior
        .read_paths
        .iter()
        .any(|path| path.ends_with("src/lib.rs")));
    assert_eq!(parent_detail.agent_relations.children.len(), 1);
    let child = &parent_detail.agent_relations.children[0];
    assert_eq!(child.session_id.as_deref(), Some("child-1"));
    assert_eq!(child.status, ConversationAgentLinkStatus::Linked);
    let child_session = child.session.as_ref().unwrap();
    let child_detail =
        conversation::load_detail(&conn, home, "cursor_agent", &child_session.session_id).unwrap();
    assert!(message_texts(&child_detail)
        .iter()
        .any(|text| text == "Child complete"));
    assert_eq!(
        child_detail
            .agent_relations
            .parent
            .as_ref()
            .and_then(|parent| parent.session_id.as_deref()),
        Some("sess-parent")
    );

    let transcript_only =
        conversation::load_detail(&conn, home, "cursor_agent", "sess-transcript-only").unwrap();
    assert!(usage_rows(&conn, "cursor_agent", "sess-transcript-only").is_empty());
    assert_eq!(message_texts(&transcript_only)[0], "No token wrapper");

    let usage_only =
        conversation::load_detail(&conn, home, "cursor_agent", "sess-usage-only").unwrap();
    assert_eq!(
        usage_rows(&conn, "cursor_agent", "sess-usage-only").len(),
        1
    );
    assert!(message_texts(&usage_only).is_empty());
    assert!(usage_only.cursor_behavior.is_none());
    assert!(usage_only.events.iter().any(|event| {
        event.kind == ConversationEventKind::SystemStatus
            && event.name.as_deref() == Some("transcript_missing")
    }));
    assert!(!usage_only.session.file_available);

    assert_eq!(
        behavior_metrics(&cursor_session::load_summary(&conn).unwrap()),
        metrics_before
    );

    std::fs::remove_file(parent).unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    let retained = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    assert!(retained.rows.iter().any(|row| {
        row.source == "cursor_agent" && row.session_id == "sess-parent" && !row.file_available
    }));
    let missing = conversation::load_detail(&conn, home, "cursor_agent", "sess-parent").unwrap();
    assert_eq!(usage_rows(&conn, "cursor_agent", "sess-parent").len(), 1);
    assert!(message_texts(&missing).is_empty());
    assert!(missing.cursor_behavior.is_none());
    assert!(missing.events.iter().any(|event| {
        event.kind == ConversationEventKind::SystemStatus
            && event.name.as_deref() == Some("transcript_missing")
    }));
}

#[test]
fn cursor_catalog_surfaces_rows_when_conversation_clock_is_empty() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_cursor_conversations(home);
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();
    conn.execute(
        "UPDATE conversation_sessions SET started_at = '', ended_at = '' WHERE source = 'cursor_agent'",
        [],
    )
    .unwrap();

    let page = conversation::sessions_page(&conn, &ConversationQuery::default()).unwrap();
    let cursor = page
        .rows
        .iter()
        .filter(|row| row.source == Source::CursorAgent.as_str())
        .collect::<Vec<_>>();
    assert!(
        !cursor.is_empty(),
        "empty conversation clocks should still appear via cursor_sessions times"
    );
    assert!(cursor.iter().all(|row| !row.ended_at.is_empty()));
}
