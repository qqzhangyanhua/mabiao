use super::{
    parse, tool_payload_failed, ConversationEvent, EventActor, EventKind, ParsedConversation,
};
use crate::test_support::fixture;
use serde_json::{json, Value};

#[test]
fn adapter_maps_redacted_thinking_as_plan() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("claude-redacted.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"user","sessionId":"claude-redacted","timestamp":"2026-09-01T10:00:00Z","message":{"role":"user","content":"hello"}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"claude-redacted","timestamp":"2026-09-01T10:00:01Z","message":{"role":"assistant","model":"claude-sonnet-test","content":[{"type":"redacted_thinking"},{"type":"text","text":"done"}]}}"#,
            "\n",
        ),
    )
    .unwrap();

    let parsed = parse(&path, false).unwrap();
    assert_eq!(parsed.session.session_id, "claude-redacted");
    let redacted = parsed
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("redacted_thinking"))
        .unwrap();
    assert_eq!(redacted.kind, EventKind::Plan);
    assert!(parsed
        .events
        .iter()
        .all(|event| event.kind != EventKind::Unadapted));
}

#[test]
fn adapter_maps_failed_tool_result_as_error_with_tool_name() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("claude-tool-fail.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"user","sessionId":"claude-tool-fail","timestamp":"2026-09-01T10:00:00Z","cwd":"/workspace","message":{"role":"user","content":"run"}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"claude-tool-fail","timestamp":"2026-09-01T10:00:01Z","message":{"role":"assistant","model":"claude-sonnet-test","content":[{"type":"tool_use","id":"tool-1","name":"Bash","input":{"command":"false"}}]}}"#,
            "\n",
            r#"{"type":"user","sessionId":"claude-tool-fail","timestamp":"2026-09-01T10:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","is_error":true,"content":"Exit code 1"}]}}"#,
            "\n",
            r#"{"type":"future_claude","payload":{"secret":"keep-unadapted"}}"#,
            "\n",
        ),
    )
    .unwrap();

    let parsed = parse(&path, false).unwrap();
    let failed = parsed
        .events
        .iter()
        .find(|event| event.kind == EventKind::Error)
        .unwrap();
    assert_eq!(failed.name.as_deref(), Some("Bash"));
    assert_eq!(failed.actor, Some(EventActor::Tool));
    assert_eq!(failed.text.as_deref(), Some("Exit code 1"));
    assert!(parsed.events.iter().any(|event| {
        event.kind == EventKind::Unadapted && event.name.as_deref() == Some("future_claude")
    }));
    assert!(parsed
        .events
        .iter()
        .all(|event| event.kind != EventKind::ToolResult));
}

#[test]
fn tool_payload_failed_covers_status_error_and_is_error_flag() {
    assert!(tool_payload_failed(
        &serde_json::json!({ "is_error": true })
    ));
    assert!(tool_payload_failed(
        &serde_json::json!({ "status": "error" })
    ));
    assert!(tool_payload_failed(
        &serde_json::json!({ "status": "failed" })
    ));
    assert!(!tool_payload_failed(
        &serde_json::json!({ "status": "completed" })
    ));
}

fn parse_claude(lines: &[Value]) -> ParsedConversation {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.jsonl");
    let mut content = String::new();
    for line in lines {
        content.push_str(&line.to_string());
        content.push('\n');
    }
    std::fs::write(&path, content).unwrap();
    parse(&path, false).unwrap()
}

fn user_line(session_id: &str, timestamp: &str, content: &str) -> Value {
    json!({
        "type": "user",
        "sessionId": session_id,
        "timestamp": timestamp,
        "cwd": "/workspace",
        "message": { "role": "user", "content": content }
    })
}

fn meta_user_line(session_id: &str, timestamp: &str, content: &str) -> Value {
    json!({
        "type": "user",
        "isMeta": true,
        "sessionId": session_id,
        "timestamp": timestamp,
        "cwd": "/workspace",
        "message": { "role": "user", "content": content }
    })
}

fn named_status<'a>(parsed: &'a ParsedConversation, name: &str) -> &'a ConversationEvent {
    parsed
        .events
        .iter()
        .find(|event| event.kind == EventKind::SystemStatus && event.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("missing system status `{name}`"))
}

fn user_texts(parsed: &ParsedConversation) -> Vec<&str> {
    parsed
        .messages
        .iter()
        .filter(|message| message.role == "user")
        .map(|message| message.text.as_str())
        .collect()
}

fn user_event_texts(parsed: &ParsedConversation) -> Vec<&str> {
    parsed
        .events
        .iter()
        .filter(|event| event.kind == EventKind::Message && event.actor == Some(EventActor::User))
        .filter_map(|event| event.text.as_deref())
        .collect()
}

const CAVEAT: &str = "<local-command-caveat>Caveat: The messages below were generated by the user while running local commands.</local-command-caveat>";
const CLEAR_COMMAND: &str =
    "<command-name>/clear</command-name>\n<command-message>clear</command-message>";
const COMPACT_COMMAND: &str =
    "<command-name>/compact</command-name>\n<command-message>compact</command-message>";

#[test]
fn adapter_skips_local_command_caveat_for_title_and_marks_system_status() {
    let parsed = parse_claude(&[
        user_line("claude-caveat", "2026-09-01T10:00:00Z", CAVEAT),
        user_line(
            "claude-caveat",
            "2026-09-01T10:00:01Z",
            "帮我看看工作区的改动",
        ),
    ]);

    assert_eq!(parsed.session.title, "帮我看看工作区的改动");
    assert_eq!(user_texts(&parsed), vec!["帮我看看工作区的改动"]);
    let status = named_status(&parsed, "caveat");
    assert_eq!(status.actor, None);
    assert_eq!(status.text.as_deref(), Some(CAVEAT));
}

#[test]
fn adapter_skips_command_name_for_title_but_keeps_user_message() {
    let parsed = parse_claude(&[
        user_line("claude-clear", "2026-09-01T10:00:00Z", CLEAR_COMMAND),
        user_line("claude-clear", "2026-09-01T10:00:01Z", COMPACT_COMMAND),
        user_line("claude-clear", "2026-09-01T10:00:02Z", "修登录"),
    ]);

    assert_eq!(parsed.session.title, "修登录");
    assert_eq!(
        user_texts(&parsed),
        vec![CLEAR_COMMAND, COMPACT_COMMAND, "修登录"]
    );
    assert_eq!(
        user_event_texts(&parsed),
        vec![CLEAR_COMMAND, COMPACT_COMMAND, "修登录"]
    );
    assert!(parsed
        .events
        .iter()
        .filter(|event| event.kind == EventKind::Message && event.actor == Some(EventActor::User))
        .all(|event| event.name.is_none()));
}

#[test]
fn adapter_skips_is_meta_for_title_and_marks_system_status() {
    let parsed = parse_claude(&[
        meta_user_line("claude-meta", "2026-09-01T10:00:00Z", "系统残渣"),
        user_line("claude-meta", "2026-09-01T10:00:01Z", "真提问"),
    ]);

    assert_eq!(parsed.session.title, "真提问");
    assert_eq!(user_texts(&parsed), vec!["真提问"]);
    let status = named_status(&parsed, "meta");
    assert_eq!(status.actor, None);
    assert_eq!(status.text.as_deref(), Some("系统残渣"));
}

#[test]
fn adapter_uses_last_nonempty_custom_or_ai_title() {
    let parsed = parse_claude(&[
        user_line("claude-titles", "2026-09-01T10:00:00Z", "首条真提问"),
        json!({
            "type": "ai-title",
            "sessionId": "claude-titles",
            "aiTitle": "AI 起的名字"
        }),
        json!({
            "type": "custom-title",
            "sessionId": "claude-titles",
            "customTitle": ""
        }),
        json!({
            "type": "custom-title",
            "sessionId": "claude-titles",
            "customTitle": "后来改的名字"
        }),
    ]);

    assert_eq!(parsed.session.title, "后来改的名字");
    assert_eq!(
        named_status(&parsed, "ai-title").text.as_deref(),
        Some("AI 起的名字")
    );
    let custom_titles: Vec<_> = parsed
        .events
        .iter()
        .filter(|event| {
            event.kind == EventKind::SystemStatus && event.name.as_deref() == Some("custom-title")
        })
        .map(|event| event.text.as_deref())
        .collect();
    assert_eq!(custom_titles, vec![None, Some("后来改的名字")]);
}

#[test]
fn adapter_prefers_custom_title_over_first_real_user_message() {
    let parsed = parse_claude(&[
        user_line("claude-named", "2026-09-01T10:00:00Z", "fix something"),
        json!({
            "type": "custom-title",
            "sessionId": "claude-named",
            "customTitle": "fix-login-bug"
        }),
    ]);

    assert_eq!(parsed.session.title, "fix-login-bug");
    assert_eq!(user_texts(&parsed), vec!["fix something"]);
}

#[test]
fn adapter_uses_ai_title_when_custom_title_is_absent() {
    let parsed = parse_claude(&[
        user_line("claude-ai-title", "2026-09-01T10:00:00Z", "首条真提问"),
        json!({
            "type": "ai-title",
            "sessionId": "claude-ai-title",
            "aiTitle": "Understanding Claude Code Architecture"
        }),
    ]);

    assert_eq!(
        parsed.session.title,
        "Understanding Claude Code Architecture"
    );
}

#[test]
fn adapter_falls_back_to_session_id_when_no_qualifying_title() {
    let parsed = parse_claude(&[
        user_line("only-residue", "2026-09-01T10:00:00Z", CAVEAT),
        user_line("only-residue", "2026-09-01T10:00:01Z", CLEAR_COMMAND),
        meta_user_line("only-residue", "2026-09-01T10:00:02Z", "系统残渣"),
    ]);

    assert_eq!(parsed.session.title, "only-residue");
    assert_eq!(user_texts(&parsed), vec![CLEAR_COMMAND]);
}

#[test]
fn adapter_keeps_user_message_that_only_mentions_caveat_tag() {
    let parsed = parse_claude(&[user_line(
        "claude-mention",
        "2026-09-01T10:00:00Z",
        "请解释 <local-command-caveat> 是什么",
    )]);

    assert_eq!(parsed.session.title, "请解释 <local-command-caveat> 是什么");
    assert_eq!(
        user_texts(&parsed),
        vec!["请解释 <local-command-caveat> 是什么"]
    );
    assert!(parsed
        .events
        .iter()
        .all(|event| event.name.as_deref() != Some("caveat")));
}

#[test]
fn adapter_falls_back_to_session_id_when_peeled_prompt_is_empty() {
    let parsed = parse_claude(&[
        user_line("empty-prompt", "2026-09-01T10:00:00Z", CLEAR_COMMAND),
        user_line(
            "empty-prompt",
            "2026-09-01T10:00:01Z",
            "<user_query>  </user_query>",
        ),
    ]);

    assert_eq!(parsed.session.title, "empty-prompt");
}

#[test]
fn adapter_keeps_ordinary_first_user_message_as_title() {
    let parsed = parse_claude(&[user_line(
        "claude-plain",
        "2026-09-01T10:00:00Z",
        "Inspect the project",
    )]);

    assert_eq!(parsed.session.title, "Inspect the project");
    assert_eq!(user_texts(&parsed), vec!["Inspect the project"]);
    assert!(parsed
        .events
        .iter()
        .all(|event| !matches!(event.name.as_deref(), Some("caveat" | "meta"))));
}

#[test]
fn adapter_keeps_existing_claude_fixture_title() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("claude-conversation.jsonl");
    std::fs::write(&path, fixture("claude-conversation.jsonl")).unwrap();
    let parsed = parse(&path, false).unwrap();
    assert_eq!(parsed.session.title, "Inspect the project");
}
