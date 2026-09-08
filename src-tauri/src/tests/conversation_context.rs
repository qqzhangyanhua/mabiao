use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::params;

use crate::domain::{
    ConversationContextItem, ConversationContextKind, ConversationContextLayer,
    ConversationEventAnchor, ConversationEventKind, Source,
};
use crate::test_support::*;

fn write_text(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn seed_cursor_transcript(home: &Path, session_id: &str) -> PathBuf {
    let path = home
        .join(".cursor/projects/Users-workspace-project/agent-transcripts")
        .join(session_id)
        .join(format!("{session_id}.jsonl"));
    write_text(
        &path,
        concat!(
            "{\"role\":\"user\",\"timestamp\":\"2026-09-08T00:00:00Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Please follow AGENTS.md and use the deploy skill\"}]}}\n",
            "{\"role\":\"assistant\",\"timestamp\":\"2026-09-08T00:00:01Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Working\"},{\"type\":\"tool_use\",\"id\":\"call-read-1\",\"name\":\"Read\",\"input\":{\"path\":\"src/lib.rs\"}},{\"type\":\"tool_use\",\"id\":\"call-read-2\",\"name\":\"Read\",\"input\":{\"path\":\"src/main.rs\"}},{\"type\":\"tool_use\",\"id\":\"call-grep\",\"name\":\"Grep\",\"input\":{\"pattern\":\"TODO\"}},{\"type\":\"tool_use\",\"id\":\"call-skill\",\"name\":\"Skill\",\"input\":{\"skill\":\"deploy\"}}]}}\n",
            "{\"type\":\"turn_ended\",\"timestamp\":\"2026-09-08T00:00:02Z\",\"status\":\"success\"}\n",
            "{\"type\":\"turn_ended\",\"timestamp\":\"2026-09-08T00:00:03Z\",\"status\":\"error\",\"error\":\"shell failed\"}\n"
        ),
    );
    path
}

fn refresh_cursor(conn: &rusqlite::Connection, home: &Path) {
    crate::conversation::refresh(conn, Source::CursorAgent, &[home.join(".cursor/projects")])
        .unwrap();
}

fn seed_grok_session(home: &Path, session_id: &str) -> PathBuf {
    let path = home
        .join(".grok/sessions/%2Fworkspace%2Fgrok")
        .join(session_id)
        .join("updates.jsonl");
    write_text(
        &path,
        concat!(
            "{\"timestamp\":1787100000,\"method\":\"session/update\",\"params\":{\"_meta\":{\"promptId\":\"prompt-1\",\"eventId\":\"event-user\"},\"update\":{\"sessionUpdate\":\"user_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"Please follow AGENTS.md and use the deploy skill\"}}}}\n",
            "{\"timestamp\":1787100001,\"method\":\"session/update\",\"params\":{\"_meta\":{\"eventId\":\"tool-read-1\"},\"update\":{\"sessionUpdate\":\"tool_call\",\"title\":\"Read\",\"toolCallId\":\"call-read-1\"}}}\n",
            "{\"timestamp\":1787100002,\"method\":\"session/update\",\"params\":{\"_meta\":{\"eventId\":\"tool-read-2\"},\"update\":{\"sessionUpdate\":\"tool_call\",\"title\":\"Read\",\"toolCallId\":\"call-read-2\"}}}\n",
            "{\"timestamp\":1787100003,\"method\":\"session/update\",\"params\":{\"_meta\":{\"eventId\":\"tool-grep\"},\"update\":{\"sessionUpdate\":\"tool_call\",\"title\":\"grep\",\"name\":\"grep\",\"toolCallId\":\"call-grep\"}}}\n",
            "{\"timestamp\":1787100004,\"method\":\"session/update\",\"params\":{\"_meta\":{\"eventId\":\"spawn\"},\"update\":{\"sessionUpdate\":\"subagent_spawned\",\"description\":\"Spec review\",\"subagent_type\":\"general-purpose\"}}}\n",
            "{\"timestamp\":1787100005,\"method\":\"session/update\",\"params\":{\"_meta\":{\"eventId\":\"finished\"},\"update\":{\"sessionUpdate\":\"subagent_finished\",\"status\":\"completed\"}}}\n",
            "{\"timestamp\":1787100006,\"method\":\"session/update\",\"params\":{\"_meta\":{\"eventId\":\"turn\",\"promptId\":\"prompt-1\"},\"update\":{\"sessionUpdate\":\"turn_completed\",\"prompt_id\":\"prompt-1\",\"stop_reason\":\"end_turn\"}}}\n"
        ),
    );
    write_text(
        &path.parent().unwrap().join("summary.json"),
        r#"{"current_model_id":"grok-test"}"#,
    );
    path
}

fn refresh_grok(conn: &rusqlite::Connection, home: &Path) {
    crate::conversation::refresh(conn, Source::Grok, &ingest::source_scan_dirs(home, Source::Grok))
        .unwrap();
}

fn layer_items(
    items: &[ConversationContextItem],
    layer: ConversationContextLayer,
) -> Vec<&ConversationContextItem> {
    items.iter().filter(|item| item.layer == layer).collect()
}

fn kind_ids(items: &[&ConversationContextItem], kind: ConversationContextKind) -> Vec<String> {
    items
        .iter()
        .filter(|item| item.kind == kind)
        .map(|item| item.id.clone())
        .collect()
}

fn assert_no_injected(items: &[ConversationContextItem], notes: &[Option<&str>]) {
    for item in items {
        let blob = format!(
            "{} {} {} {}",
            item.layer.as_str(),
            item.kind.as_str(),
            item.label,
            item.meta
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default()
        );
        assert!(
            !blob.contains("已注入"),
            "context item must not claim injection: {blob}"
        );
    }
    for note in notes.iter().flatten() {
        assert!(
            !note.contains("已注入"),
            "note must not claim injection: {note}"
        );
        if note.contains("磁盘") || note.contains("可能") {
            assert!(
                note.contains("可能生效") || note.contains("未发现") || note.contains("无法确认"),
                "{note}"
            );
        }
    }
}

#[test]
fn cursor_disk_lists_existing_project_files_and_omits_missing_paths() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    write_text(&project.path().join("AGENTS.md"), "# agents\n");
    write_text(
        &project.path().join(".cursor/rules/style.mdc"),
        "---\ndescription: style\n---\n",
    );
    write_text(
        &project.path().join(".cursor/skills/deploy/SKILL.md"),
        "---\nname: deploy\n---\n# deploy\n",
    );
    write_text(
        &project.path().join(".cursor/mcp.json"),
        r#"{"mcpServers":{"project-docs":{"command":"npx"}}}"#,
    );
    write_text(
        &home.path().join(".cursor/mcp.json"),
        r#"{"mcpServers":{"user-search":{"command":"uvx"}}}"#,
    );
    write_text(
        &home.path().join(".cursor/skills/review/SKILL.md"),
        "---\nname: review\n---\n",
    );
    write_text(
        &home.path().join(".cursor/skills-cursor/env-setup/SKILL.md"),
        "---\nname: env-setup\n---\n",
    );

    let items = crate::instructions::cursor_disk::scan(home.path(), project.path());
    let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
    assert!(ids.contains(&"AGENTS.md"));
    assert!(ids.contains(&".cursor/rules/style.mdc"));
    assert!(ids.contains(&"project:deploy"));
    assert!(ids.contains(&"user:review"));
    assert!(ids.contains(&"project:project-docs"));
    assert!(ids.contains(&"user:user-search"));
    assert!(items
        .iter()
        .all(|item| item.layer == ConversationContextLayer::OnDiskPossible));
    assert!(items.iter().all(|item| !item.label.contains("已注入")));
    assert!(!ids.iter().any(|id| id.contains("env-setup")));
    assert!(!ids.iter().any(|id| id.contains("CLAUDE.md")));

    let empty = tempfile::tempdir().unwrap();
    write_text(&empty.path().join("AGENTS.md"), "# only\n");
    let sparse = crate::instructions::cursor_disk::scan(home.path(), empty.path());
    let sparse_kinds: Vec<_> = sparse.iter().map(|item| item.kind).collect();
    assert!(sparse
        .iter()
        .any(|item| item.kind == ConversationContextKind::Instruction && item.id == "AGENTS.md"));
    assert!(!sparse_kinds.contains(&ConversationContextKind::Rule));
    assert!(
        !sparse
            .iter()
            .any(|item| item.kind == ConversationContextKind::Skill
                && item.id.starts_with("project:"))
    );
    assert!(!sparse
        .iter()
        .any(|item| item.kind == ConversationContextKind::McpServer
            && item.id.starts_with("project:")));
}

#[test]
fn cursor_detail_context_manifest_matches_timeline_tools_and_disk_scan() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let project = tempfile::tempdir().unwrap();
    seed_cursor_transcript(home, "sess-ctx");
    write_text(&project.path().join("AGENTS.md"), "# agents\n");
    write_text(
        &project.path().join(".cursor/rules/rust.mdc"),
        "always use rust\n",
    );
    write_text(
        &project.path().join(".cursor/skills/deploy/SKILL.md"),
        "---\nname: deploy\n---\n",
    );
    write_text(
        &project.path().join(".cursor/mcp.json"),
        r#"{"mcpServers":{"docs":{"url":"https://example.test/mcp"}}}"#,
    );
    write_text(
        &home.join(".cursor/mcp.json"),
        r#"{"mcpServers":{"search":{"command":"uvx"}}}"#,
    );

    let conn = store::open_memory().unwrap();
    refresh_cursor(&conn, home);
    conn.execute(
        "UPDATE conversation_sessions SET project = ?1 WHERE source = 'cursor_agent' AND session_id = 'sess-ctx'",
        params![project.path().to_string_lossy()],
    )
    .unwrap();

    let detail = crate::conversation::load_detail(&conn, home, "cursor_agent", "sess-ctx").unwrap();
    let manifest = detail
        .context_manifest
        .as_ref()
        .expect("cursor detail should carry a context manifest");
    assert_no_injected(
        &manifest.items,
        &[
            manifest.observed_note.as_deref(),
            manifest.on_disk_note.as_deref(),
        ],
    );
    assert!(manifest
        .on_disk_note
        .as_deref()
        .is_some_and(|note| note.contains("可能生效") && !note.contains("已注入")));

    let events = crate::conversation::load_events(
        &conn,
        home,
        "cursor_agent",
        "sess-ctx",
        ConversationEventAnchor::First,
        200,
    )
    .unwrap();
    let mut timeline_tools = BTreeMap::<String, u64>::new();
    for event in &events.events {
        if event.kind == ConversationEventKind::ToolCall {
            if let Some(name) = event.name.as_deref().filter(|name| !name.is_empty()) {
                *timeline_tools.entry(name.to_string()).or_default() += 1;
            }
        }
    }
    assert_eq!(timeline_tools.get("Read").copied(), Some(2));
    assert_eq!(timeline_tools.get("Grep").copied(), Some(1));
    assert_eq!(timeline_tools.get("Skill").copied(), Some(1));

    let observed = layer_items(&manifest.items, ConversationContextLayer::Observed);
    let mut observed_tools = BTreeMap::<String, u64>::new();
    for item in &observed {
        if item.kind != ConversationContextKind::Tool {
            continue;
        }
        let count = item
            .meta
            .as_ref()
            .and_then(|meta| meta.get("call_count"))
            .and_then(|value| value.as_u64())
            .expect("observed tool should carry call_count");
        observed_tools.insert(item.id.clone(), count);
    }
    assert_eq!(observed_tools, timeline_tools);
    assert!(observed.iter().any(
        |item| item.kind == ConversationContextKind::SystemStatus && item.id == "turn_success"
    ));
    assert!(observed
        .iter()
        .any(|item| item.kind == ConversationContextKind::Error && item.id == "turn_error"));
    assert!(!observed
        .iter()
        .any(|item| item.label.contains("AGENTS.md") || item.id.contains("AGENTS.md")));

    let possible = layer_items(&manifest.items, ConversationContextLayer::OnDiskPossible);
    assert!(kind_ids(&possible, ConversationContextKind::Instruction).contains(&"AGENTS.md".into()));
    assert!(kind_ids(&possible, ConversationContextKind::Rule)
        .iter()
        .any(|id| id.ends_with("rust.mdc")));
    assert!(kind_ids(&possible, ConversationContextKind::Skill).contains(&"project:deploy".into()));
    assert!(
        kind_ids(&possible, ConversationContextKind::McpServer).contains(&"project:docs".into())
    );
    assert!(kind_ids(&possible, ConversationContextKind::McpServer).contains(&"user:search".into()));
}

#[test]
fn cursor_context_omits_missing_disk_paths_and_does_not_invent_skills() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let project = tempfile::tempdir().unwrap();
    seed_cursor_transcript(home, "sess-empty-disk");
    write_text(&project.path().join("README.md"), "not an instruction\n");

    let conn = store::open_memory().unwrap();
    refresh_cursor(&conn, home);
    conn.execute(
        "UPDATE conversation_sessions SET project = ?1 WHERE source = 'cursor_agent' AND session_id = 'sess-empty-disk'",
        params![project.path().to_string_lossy()],
    )
    .unwrap();

    let detail =
        crate::conversation::load_detail(&conn, home, "cursor_agent", "sess-empty-disk").unwrap();
    let manifest = detail.context_manifest.unwrap();
    let possible = layer_items(&manifest.items, ConversationContextLayer::OnDiskPossible);
    assert!(possible.is_empty(), "{possible:?}");
    assert!(manifest
        .on_disk_note
        .as_deref()
        .is_some_and(|note| note.contains("未发现") && !note.contains("已注入")));
    assert!(
        layer_items(&manifest.items, ConversationContextLayer::Observed)
            .iter()
            .all(|item| item.kind != ConversationContextKind::Skill)
    );
}

#[test]
fn grok_disk_lists_existing_home_files_and_omits_project_and_unverified_kinds() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    write_text(&home.path().join(".grok/AGENTS.md"), "# grok-global\n");
    write_text(&home.path().join(".grok/rules/style.md"), "prefer rust\n");
    write_text(&home.path().join(".grok/rules/ignore.txt"), "skip\n");
    write_text(&home.path().join(".grok/config.toml"), "not-an-instruction\n");
    write_text(&project.path().join("AGENTS.md"), "# project-agents\n");
    write_text(
        &project.path().join(".cursor/rules/style.mdc"),
        "cursor-only\n",
    );
    write_text(
        &home.path().join(".cursor/skills/review/SKILL.md"),
        "---\nname: review\n---\n",
    );

    let items = crate::instructions::grok_disk::scan(home.path());
    let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
    assert!(ids.contains(&"~/.grok/AGENTS.md"));
    assert!(ids.contains(&"~/.grok/rules/style.md"));
    assert!(items
        .iter()
        .all(|item| item.layer == ConversationContextLayer::OnDiskPossible));
    assert!(items.iter().all(|item| !item.label.contains("已注入")));
    assert!(!ids.iter().any(|id| id.contains("ignore.txt")));
    assert!(!ids.iter().any(|id| id.contains("config.toml")));
    assert!(!ids.iter().any(|id| *id == "AGENTS.md"));
    assert!(!ids.iter().any(|id| id.contains("style.mdc")));
    assert!(!ids.iter().any(|id| id.contains("review")));
    assert!(items
        .iter()
        .all(|item| item.kind != ConversationContextKind::McpServer
            && item.kind != ConversationContextKind::Skill));

    let empty_home = tempfile::tempdir().unwrap();
    write_text(&empty_home.path().join("AGENTS.md"), "# not grok\n");
    let empty = crate::instructions::grok_disk::scan(empty_home.path());
    assert!(empty.is_empty(), "{empty:?}");
}

#[test]
fn grok_detail_context_manifest_matches_timeline_tools_and_home_disk() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let project = tempfile::tempdir().unwrap();
    seed_grok_session(home, "sess-grok-ctx");
    write_text(&home.join(".grok/AGENTS.md"), "# grok-global\n");
    write_text(&home.join(".grok/rules/style.md"), "prefer rust\n");
    write_text(&project.path().join("AGENTS.md"), "# project-root\n");

    let conn = store::open_memory().unwrap();
    refresh_grok(&conn, home);

    let detail = crate::conversation::load_detail(&conn, home, "grok", "sess-grok-ctx").unwrap();
    let manifest = detail
        .context_manifest
        .as_ref()
        .expect("grok detail should carry a context manifest");
    assert_no_injected(
        &manifest.items,
        &[
            manifest.observed_note.as_deref(),
            manifest.on_disk_note.as_deref(),
        ],
    );
    assert!(manifest.on_disk_note.as_deref().is_some_and(|note| {
        note.contains("可能生效") && note.contains("未扫描") && !note.contains("已注入")
    }));

    let events = crate::conversation::load_events(
        &conn,
        home,
        "grok",
        "sess-grok-ctx",
        ConversationEventAnchor::First,
        200,
    )
    .unwrap();
    let mut timeline_tools = BTreeMap::<String, u64>::new();
    for event in &events.events {
        if event.kind == ConversationEventKind::ToolCall {
            if let Some(name) = event.name.as_deref().filter(|name| !name.is_empty()) {
                *timeline_tools.entry(name.to_string()).or_default() += 1;
            }
        }
    }
    assert_eq!(timeline_tools.get("Read").copied(), Some(2));
    assert_eq!(timeline_tools.get("grep").copied(), Some(1));

    let observed = layer_items(&manifest.items, ConversationContextLayer::Observed);
    let mut observed_tools = BTreeMap::<String, u64>::new();
    for item in &observed {
        if item.kind != ConversationContextKind::Tool {
            continue;
        }
        let count = item
            .meta
            .as_ref()
            .and_then(|meta| meta.get("call_count"))
            .and_then(|value| value.as_u64())
            .expect("observed tool should carry call_count");
        observed_tools.insert(item.id.clone(), count);
    }
    assert_eq!(observed_tools, timeline_tools);
    assert!(observed.iter().any(
        |item| item.kind == ConversationContextKind::SystemStatus && item.id == "subagent_spawned"
    ));
    assert!(observed.iter().any(
        |item| item.kind == ConversationContextKind::SystemStatus && item.id == "subagent_finished"
    ));
    assert!(!observed
        .iter()
        .any(|item| item.label.contains("AGENTS.md") || item.id.contains("AGENTS.md")));
    assert!(observed
        .iter()
        .all(|item| item.kind != ConversationContextKind::Skill));

    let possible = layer_items(&manifest.items, ConversationContextLayer::OnDiskPossible);
    assert!(kind_ids(&possible, ConversationContextKind::Instruction)
        .contains(&"~/.grok/AGENTS.md".into()));
    assert!(kind_ids(&possible, ConversationContextKind::Rule)
        .contains(&"~/.grok/rules/style.md".into()));
    let project_agents = project.path().join("AGENTS.md");
    assert!(!possible.iter().any(|item| {
        item.id == "AGENTS.md"
            || item
                .path
                .as_deref()
                .is_some_and(|path| Path::new(path) == project_agents.as_path())
    }));
}

#[test]
fn grok_context_omits_missing_home_files_and_does_not_invent_agents() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let project = tempfile::tempdir().unwrap();
    seed_grok_session(home, "sess-grok-empty");
    write_text(&project.path().join("AGENTS.md"), "project-only\n");
    write_text(&home.join(".grok/sessions/README.md"), "not an instruction\n");

    let conn = store::open_memory().unwrap();
    refresh_grok(&conn, home);

    let detail = crate::conversation::load_detail(&conn, home, "grok", "sess-grok-empty").unwrap();
    let manifest = detail.context_manifest.unwrap();
    let possible = layer_items(&manifest.items, ConversationContextLayer::OnDiskPossible);
    assert!(possible.is_empty(), "{possible:?}");
    assert!(manifest.on_disk_note.as_deref().is_some_and(|note| {
        note.contains("未发现")
            && note.contains("未扫描")
            && !note.contains("已注入")
            && note.contains("~/.grok")
    }));
    assert!(!manifest.items.iter().any(|item| item.id == "AGENTS.md"
        || item.label == "AGENTS.md"
        || item.id.contains("project-only")));
}

#[test]
fn non_cursor_detail_leaves_shared_manifest_slot_empty() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let path = home.join(".codex/sessions/2026/08/rollout-conv-1.jsonl");
    write_text(
        &path,
        concat!(
            "{\"type\":\"session_meta\",\"timestamp\":\"2026-08-21T00:00:00Z\",\"payload\":{\"id\":\"conv-1\",\"cwd\":\"/tmp/proj\"}}\n",
            "{\"type\":\"response_item\",\"timestamp\":\"2026-08-21T00:00:01Z\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hi\"}]}}\n"
        ),
    );
    let conn = store::open_memory().unwrap();
    crate::conversation::refresh_codex(&conn, home).unwrap();
    let detail = crate::conversation::load_detail(&conn, home, "codex", "conv-1").unwrap();
    assert!(detail.context_manifest.is_none());
}
