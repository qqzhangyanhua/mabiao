use super::{index, index_suffix, ConversationEvent, EventActor, EventKind};
use crate::test_support::fixture;

fn write_fixture(temp: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = temp.join(name);
    std::fs::write(&path, fixture(name)).unwrap();
    path
}

fn offset_after_lines(content: &str, line_count: usize) -> (u64, u32) {
    let bytes = content.as_bytes();
    let mut seen = 0usize;
    let mut index = 0usize;
    while index < bytes.len() && seen < line_count {
        if bytes[index] == b'\n' {
            seen += 1;
        }
        index += 1;
    }
    (index as u64, seen as u32)
}

fn event_view(event: &ConversationEvent) -> (EventKind, Option<&str>, Option<&str>, u32) {
    (
        event.kind,
        event.name.as_deref(),
        event.text.as_deref(),
        event.source_sequence,
    )
}

#[test]
fn adapter_uses_legacy_header_id_when_session_meta_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollout-legacy.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"id":"9f9688f4-edb6-4e92-9311-5bdf6b508616","timestamp":"2025-09-05T17:07:46.739Z"}"#,
            "\n",
            r#"{"type":"message","role":"user","content":[{"type":"input_text","text":"你好"}]}"#,
            "\n",
        ),
    )
    .unwrap();
    let batch = index(&path).unwrap();
    assert_eq!(batch.conversations.len(), 1);
    assert_eq!(
        batch.conversations[0].session.session_id,
        "9f9688f4-edb6-4e92-9311-5bdf6b508616"
    );
}

#[test]
fn adapter_indexes_codex_fixture_into_session_events_and_cursor() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_fixture(temp.path(), "codex-semantic-events.jsonl");
    let batch = index(&path).unwrap();
    assert_eq!(batch.conversations.len(), 1);
    assert!(batch.diagnostics.is_empty());

    let parsed = &batch.conversations[0];
    assert_eq!(parsed.session.session_id, "semantic-1");
    assert_eq!(parsed.session.project, "/workspace/semantic-project");
    assert_eq!(parsed.session.model, "gpt-5.7-codex");
    assert_eq!(parsed.session.title, "实现语义时间线");
    assert_eq!(parsed.session.started_at, "2026-08-21T00:00:00Z");
    assert_eq!(parsed.session.ended_at, "2026-08-21T00:00:11Z");
    assert_eq!(
        parsed
            .events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        [
            "system_status",
            "model_change",
            "message",
            "message",
            "plan",
            "tool_call",
            "tool_result",
            "model_change",
            "error",
            "unadapted",
        ]
    );
    assert_eq!(parsed.events[2].text.as_deref(), Some("实现语义时间线"));
    assert_eq!(parsed.events[3].text.as_deref(), Some("我先检查现有实现。"));
    assert_eq!(parsed.events[4].text.as_deref(), Some("按层实现"));
    assert_eq!(parsed.events[5].name.as_deref(), Some("read_file"));
    assert_eq!(parsed.events[6].text.as_deref(), Some("fn main() {}"));
    assert_eq!(parsed.events[7].name.as_deref(), Some("gpt-5.7-codex"));
    assert_eq!(parsed.events[8].text.as_deref(), Some("工具执行失败"));
    assert_eq!(parsed.events[9].name.as_deref(), Some("future_event"));
    assert!(parsed
        .events
        .iter()
        .all(|event| { !matches!(event.name.as_deref(), Some("token_count" | "heartbeat")) }));

    let cursor = parsed.index_cursor.expect("index cursor");
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(cursor.byte_offset, content.len() as i64);
    assert_eq!(
        cursor.line,
        i64::from(offset_after_lines(&content, usize::MAX).1)
    );
}

#[test]
fn adapter_indexes_codex_suffix_matching_suffix_content_and_full_tail() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_fixture(temp.path(), "codex-semantic-events.jsonl");
    let content = std::fs::read_to_string(&path).unwrap();
    let (offset, start_line) = offset_after_lines(&content, 3);
    assert!(offset > 0 && (offset as usize) < content.len());

    let full = &index(&path).unwrap().conversations[0];
    let from_offset = index_suffix(&path, offset, start_line, "semantic-1").unwrap();
    let suffix_only_path = temp.path().join("suffix-only.jsonl");
    std::fs::write(&suffix_only_path, &content[offset as usize..]).unwrap();
    let from_suffix_only = index_suffix(&suffix_only_path, 0, 0, "semantic-1").unwrap();

    let expected_tail = full
        .events
        .iter()
        .filter(|event| event.source_sequence >= start_line)
        .map(event_view)
        .collect::<Vec<_>>();
    assert_eq!(
        from_offset
            .events
            .iter()
            .map(event_view)
            .collect::<Vec<_>>(),
        expected_tail
    );
    assert_eq!(from_offset.session.session_id, "semantic-1");
    assert_eq!(
        from_offset
            .events
            .iter()
            .map(|event| (
                event.kind,
                event.name.clone(),
                event.text.clone(),
                event.source_sequence - start_line
            ))
            .collect::<Vec<_>>(),
        from_suffix_only
            .events
            .iter()
            .map(|event| (
                event.kind,
                event.name.clone(),
                event.text.clone(),
                event.source_sequence
            ))
            .collect::<Vec<_>>()
    );

    let from_offset_cursor = from_offset.index_cursor.expect("suffix cursor");
    let suffix_only_cursor = from_suffix_only.index_cursor.expect("suffix-only cursor");
    assert_eq!(
        from_offset_cursor.byte_offset,
        offset as i64 + suffix_only_cursor.byte_offset
    );
    assert_eq!(
        from_offset_cursor.line,
        i64::from(start_line) + suffix_only_cursor.line
    );
    assert_eq!(from_offset_cursor.byte_offset, content.len() as i64);
}

#[test]
fn adapter_indexes_codex_suffix_reports_session_id_from_suffix_meta() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("suffix-meta.jsonl");
    let prefix = concat!(
        r#"{"type":"session_meta","timestamp":"2026-08-20T00:00:00Z","payload":{"id":"conv-1"}}"#,
        "\n",
    );
    let suffix = concat!(
        r#"{"type":"session_meta","timestamp":"2026-08-20T00:04:00Z","payload":{"id":"conv-other"}}"#,
        "\n",
        r#"{"type":"response_item","timestamp":"2026-08-20T00:04:01Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hijacked"}]}}"#,
        "\n",
    );
    std::fs::write(&path, format!("{prefix}{suffix}")).unwrap();

    let parsed = index_suffix(&path, prefix.len() as u64, 1, "conv-1").unwrap();
    assert_eq!(parsed.session.session_id, "conv-other");
    assert!(parsed
        .events
        .iter()
        .any(|event| event.text.as_deref() == Some("hijacked")));
}

#[test]
fn adapter_indexes_codex_suffix_stops_before_incomplete_trailing_line() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("codex-incomplete.jsonl");
    let mut content = fixture("codex-conversation.jsonl");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    let complete_len = content.len();
    let complete_lines = offset_after_lines(&content, usize::MAX).1;
    content.push_str(
        r#"{"type":"response_item","timestamp":"2026-08-20T00:04:00Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"stream"#,
    );
    std::fs::write(&path, &content).unwrap();

    let parsed = index_suffix(&path, 0, 0, "conv-1").unwrap();
    assert_eq!(parsed.session.session_id, "conv-1");
    assert!(parsed
        .events
        .iter()
        .all(|event| event.text.as_deref() != Some("stream")));
    let cursor = parsed.index_cursor.expect("incomplete-tail cursor");
    assert_eq!(cursor.byte_offset, complete_len as i64);
    assert_eq!(cursor.line, i64::from(complete_lines));

    let issue = match index(&path) {
        Err(issue) => issue,
        Ok(_) => panic!("expected full index to reject incomplete trailing JSON"),
    };
    assert!(issue.message.contains("JSON 无效"));
    assert_eq!(issue.event_type.as_deref(), Some("json_line"));
}

#[test]
fn adapter_maps_legacy_response_items_and_skips_internal_records() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("legacy.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"id":"legacy-1","timestamp":"2025-09-05T17:07:46.739Z"}"#,
            "\n",
            r#"{"record_type":"state"}"#,
            "\n",
            r#"{"type":"message","timestamp":"2025-09-05T17:07:47Z","role":"user","content":[{"type":"input_text","text":"列出文件"}]}"#,
            "\n",
            r#"{"type":"function_call","timestamp":"2025-09-05T17:07:48Z","name":"shell","arguments":"{\"command\":[\"ls\"]}","call_id":"call-legacy"}"#,
            "\n",
            r#"{"type":"function_call_output","timestamp":"2025-09-05T17:07:49Z","call_id":"call-legacy","output":"src"}"#,
            "\n",
            r#"{"type":"reasoning","timestamp":"2025-09-05T17:07:50Z","summary":[{"type":"summary_text","text":"先看目录"}]}"#,
            "\n",
            r#"{"type":"compacted","timestamp":"2025-09-05T17:08:00Z","payload":{"message":"handoff"}}"#,
            "\n",
            r#"{"type":"response_item","timestamp":"2025-09-05T17:08:01Z","payload":{"type":"ghost_snapshot","ghost_commit":{"id":"abc"}}}"#,
            "\n",
            r#"{"type":"future_event","payload":{"secret":"keep-unadapted"}}"#,
            "\n",
        ),
    )
    .unwrap();

    let parsed = &index(&path).unwrap().conversations[0];
    assert_eq!(parsed.session.session_id, "legacy-1");
    assert_eq!(parsed.session.title, "列出文件");
    let kinds = parsed
        .events
        .iter()
        .map(|event| {
            (
                event.kind.as_str(),
                event.name.as_deref(),
                event.text.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            ("message", None, Some("列出文件")),
            ("tool_call", Some("shell"), Some("{\"command\":[\"ls\"]}")),
            ("tool_result", Some("shell"), Some("src")),
            ("plan", None, Some("先看目录")),
            ("system_status", Some("compacted"), None),
            ("system_status", Some("ghost_snapshot"), None),
            ("unadapted", Some("future_event"), None),
        ]
    );
    assert!(parsed
        .events
        .iter()
        .all(|event| event.name.as_deref() != Some("unknown")));
}

#[test]
fn adapter_maps_codex_remaining_kinds_and_keeps_unknown_unadapted() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("codex-remaining.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session_meta","timestamp":"2026-08-21T00:00:00Z","payload":{"id":"remain-1","cwd":"/workspace"}}"#,
            "\n",
            r#"{"type":"world_state","timestamp":"2026-08-21T00:00:01Z","payload":{"full":false}}"#,
            "\n",
            r#"{"type":"inter_agent_communication_metadata","timestamp":"2026-08-21T00:00:02Z","payload":{"trigger_turn":true}}"#,
            "\n",
            r#"{"type":"event_msg","timestamp":"2026-08-21T00:00:03Z","payload":{"type":"thread_goal_updated","goal":{"objective":"覆盖 i18n"}}}"#,
            "\n",
            r#"{"type":"event_msg","timestamp":"2026-08-21T00:00:04Z","payload":{"type":"thread_name_updated","thread_name":"Review"}}"#,
            "\n",
            r#"{"type":"event_msg","timestamp":"2026-08-21T00:00:05Z","payload":{"type":"item_completed","item":{"type":"UserMessage"}}}"#,
            "\n",
            r#"{"type":"response_item","timestamp":"2026-08-21T00:00:06Z","payload":{"type":"function_call","name":"exec_command","call_id":"call-ok","arguments":"{}"}}"#,
            "\n",
            r#"{"type":"response_item","timestamp":"2026-08-21T00:00:07Z","payload":{"type":"function_call_output","call_id":"call-ok","output":"ok"}}"#,
            "\n",
            r#"{"type":"event_msg","timestamp":"2026-08-21T00:00:08Z","payload":{"type":"exec_command_end","call_id":"call-ok","exit_code":0,"aggregated_output":"ok"}}"#,
            "\n",
            r#"{"type":"event_msg","timestamp":"2026-08-21T00:00:09Z","payload":{"type":"exec_command_end","call_id":"call-fail","exit_code":1,"aggregated_output":"boom"}}"#,
            "\n",
            r#"{"type":"event_msg","timestamp":"2026-08-21T00:00:10Z","payload":{"type":"view_image_tool_call","call_id":"call-img","path":"/tmp/a.png"}}"#,
            "\n",
            r#"{"type":"response_item","timestamp":"2026-08-21T00:00:11Z","payload":{"type":"tool_search_call","call_id":"call-search","arguments":{"query":"spawn"}}}"#,
            "\n",
            r#"{"type":"response_item","timestamp":"2026-08-21T00:00:12Z","payload":{"type":"agent_message","content":[{"type":"output_text","text":"collab hello"}]}}"#,
            "\n",
            r#"{"type":"future_event","payload":{"secret":"keep-unadapted"}}"#,
            "\n",
        ),
    )
    .unwrap();

    let parsed = &index(&path).unwrap().conversations[0];
    let kinds = parsed
        .events
        .iter()
        .map(|event| {
            (
                event.kind.as_str(),
                event.name.as_deref(),
                event.text.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            ("system_status", Some("session_started"), None),
            ("system_status", Some("world_state"), None),
            (
                "system_status",
                Some("inter_agent_communication_metadata"),
                None
            ),
            ("plan", Some("thread_goal_updated"), Some("覆盖 i18n")),
            ("system_status", Some("thread_name_updated"), Some("Review")),
            ("tool_call", Some("exec_command"), Some("{}")),
            ("tool_result", Some("exec_command"), Some("ok")),
            ("error", Some("exec_command"), Some("boom")),
            ("tool_call", Some("view_image"), Some("/tmp/a.png")),
            ("tool_call", Some("tool_search_call"), Some("spawn")),
            ("message", None, Some("collab hello")),
            ("unadapted", Some("future_event"), None),
        ]
    );
    assert!(parsed
        .events
        .iter()
        .all(|event| event.name.as_deref() != Some("item_completed")));
    let failed = parsed
        .events
        .iter()
        .find(|event| event.kind == EventKind::Error)
        .unwrap();
    assert_eq!(failed.actor, Some(EventActor::Tool));
}

#[test]
fn adapter_index_rejects_missing_codex_session_id() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("missing-id.jsonl");
    std::fs::write(&path, "{}\n").unwrap();
    let issue = match index(&path) {
        Err(issue) => issue,
        Ok(_) => panic!("expected missing Codex session ID to fail"),
    };
    assert!(issue.message.contains("会话 ID"));
    assert_eq!(issue.event_type.as_deref(), Some("session_meta"));
    assert!(issue.line.is_none());
}
