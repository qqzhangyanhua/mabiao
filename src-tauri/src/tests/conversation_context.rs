use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::params;

use crate::domain::{
    ConversationContextInjectionStatus, ConversationContextItem, ConversationContextKind,
    ConversationContextLayer, ConversationContextLoadMode, ConversationEventAnchor,
    ConversationEventKind, Source,
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
    seed_grok_session_at(home, "%2Fworkspace%2Fgrok", session_id)
}

fn seed_grok_session_at(home: &Path, encoded_cwd: &str, session_id: &str) -> PathBuf {
    let path = home
        .join(".grok/sessions")
        .join(encoded_cwd)
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

fn write_grok_prompt_context(updates: &Path, files: &[(&str, &str, &str)]) {
    let agents: Vec<serde_json::Value> = files
        .iter()
        .map(|(name, path, content)| {
            serde_json::json!({
                "file_name": name,
                "file_path": path,
                "content": content,
            })
        })
        .collect();
    write_text(
        &updates.parent().unwrap().join("prompt_context.json"),
        &serde_json::json!({
            "version": 1,
            "agents_md_files": agents,
        })
        .to_string(),
    );
}

fn write_grok_events(updates: &Path, events: &[serde_json::Value]) {
    let body = events
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    write_text(
        &updates.parent().unwrap().join("events.jsonl"),
        &format!("{body}\n"),
    );
}

fn append_grok_tool_call(updates: &Path, title: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(updates)
        .unwrap();
    writeln!(
        file,
        "{}",
        serde_json::json!({
            "timestamp": 1787100007u64,
            "method": "session/update",
            "params": {
                "_meta": {"eventId": "tool-mcp"},
                "update": {
                    "sessionUpdate": "tool_call",
                    "title": title,
                    "name": title,
                    "toolCallId": "call-mcp-1"
                }
            }
        })
    )
    .unwrap();
}

fn mcp_item<'a>(items: &'a [&ConversationContextItem], id: &str) -> &'a ConversationContextItem {
    items
        .iter()
        .find(|item| item.kind == ConversationContextKind::McpServer && item.id == id)
        .copied()
        .unwrap_or_else(|| panic!("missing injected MCP {id} in {items:?}"))
}

fn scan_grok(home: &Path, project: &Path) -> Vec<ConversationContextItem> {
    crate::instructions::grok_disk::scan(home, project)
}

fn item_scope(item: &ConversationContextItem) -> Option<&str> {
    item.meta
        .as_ref()
        .and_then(|meta| meta.get("config_scope"))
        .and_then(|value| value.as_str())
}

fn refresh_grok(conn: &rusqlite::Connection, home: &Path) {
    crate::conversation::refresh(
        conn,
        Source::Grok,
        &ingest::source_scan_dirs(home, Source::Grok),
    )
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
        if item.layer == ConversationContextLayer::Injected {
            continue;
        }
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
            "non-injected context item must not claim injection: {blob}"
        );
    }
    for note in notes.iter().flatten() {
        assert!(
            !note.contains("已注入"),
            "non-injected note must not claim injection: {note}"
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
    assert!(
        observed
            .iter()
            .any(|item| item.kind == ConversationContextKind::Skill && item.id == "deploy"),
        "Skill tool_use input.skill should appear as an observed skill"
    );

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
    let observed = layer_items(&manifest.items, ConversationContextLayer::Observed);
    assert!(
        observed
            .iter()
            .any(|item| item.kind == ConversationContextKind::Skill && item.id == "deploy"),
        "Skill tool_use is a transcript trace, not an invented skill"
    );
    assert_eq!(
        observed
            .iter()
            .filter(|item| item.kind == ConversationContextKind::Skill)
            .count(),
        1,
        "user/assistant prose must not invent extra skills"
    );
}

#[test]
fn grok_disk_lists_existing_home_files_and_omits_project_agents() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    write_text(&home.path().join(".grok/AGENTS.md"), "# grok-global\n");
    write_text(&home.path().join(".grok/rules/style.md"), "prefer rust\n");
    write_text(&home.path().join(".grok/rules/ignore.txt"), "skip\n");
    write_text(
        &home.path().join(".grok/skills/commit/SKILL.md"),
        "---\nname: commit\n---\n",
    );
    write_text(
        &home.path().join(".grok/config.toml"),
        "not-an-instruction\n",
    );
    write_text(&project.path().join("AGENTS.md"), "# project-agents\n");
    write_text(
        &project.path().join(".cursor/rules/style.mdc"),
        "cursor-only\n",
    );

    let items = scan_grok(home.path(), project.path());
    let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
    assert!(ids.contains(&"~/.grok/AGENTS.md"));
    assert!(ids.contains(&"~/.grok/rules/style.md"));
    assert!(ids.contains(&"user:commit"));
    assert!(items
        .iter()
        .all(|item| item.layer == ConversationContextLayer::OnDiskPossible));
    assert!(items.iter().all(|item| !item.label.contains("已注入")));
    assert!(!ids.iter().any(|id| id.contains("ignore.txt")));
    assert!(!ids.iter().any(|id| id.contains("config.toml")));
    assert!(!ids.contains(&"AGENTS.md"));
    assert!(!ids.iter().any(|id| id.contains("style.mdc")));
    assert!(items
        .iter()
        .all(|item| item.kind != ConversationContextKind::McpServer));

    let empty_home = tempfile::tempdir().unwrap();
    write_text(&empty_home.path().join("AGENTS.md"), "# not grok\n");
    let empty = scan_grok(empty_home.path(), Path::new(""));
    assert!(empty.is_empty(), "{empty:?}");
}

#[test]
fn grok_disk_lists_user_skills_honoring_paths_ignore_and_disabled() {
    let home = tempfile::tempdir().unwrap();
    write_text(
        &home.path().join(".grok/config.toml"),
        concat!(
            "[skills]\n",
            "paths = [\"~/team-skills\"]\n",
            "ignore = [\"~/team-skills/wip\"]\n",
            "disabled = [\"secret\"]\n",
        ),
    );
    write_text(
        &home.path().join(".grok/skills/commit/SKILL.md"),
        "---\nname: commit\n---\n",
    );
    write_text(
        &home.path().join(".grok/skills/secret/SKILL.md"),
        "---\nname: secret\n---\n",
    );
    write_text(
        &home.path().join("team-skills/ok/SKILL.md"),
        "---\nname: ok\n---\n",
    );
    write_text(
        &home.path().join("team-skills/wip/hidden/SKILL.md"),
        "---\nname: hidden\n---\n",
    );
    write_text(
        &home.path().join(".cursor/skills-cursor/shell/SKILL.md"),
        "---\nname: shell\n---\n",
    );

    let items = scan_grok(home.path(), Path::new(""));
    let skill_ids = kind_ids(
        &items.iter().collect::<Vec<_>>(),
        ConversationContextKind::Skill,
    );
    assert!(skill_ids.contains(&"user:commit".into()));
    assert!(skill_ids.contains(&"config:ok".into()));
    assert!(skill_ids.contains(&"user:secret".into()));
    let secret = items
        .iter()
        .find(|item| item.id == "user:secret")
        .expect("disabled skill stays listed");
    assert_eq!(
        secret
            .meta
            .as_ref()
            .and_then(|meta| meta.get("disabled"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert!(!skill_ids.iter().any(|id| id.contains("hidden")));
    assert!(!skill_ids.iter().any(|id| id.contains("shell")));
}

#[test]
fn grok_disk_follows_compat_skill_switches() {
    let home = tempfile::tempdir().unwrap();
    write_text(
        &home.path().join(".grok/skills/native/SKILL.md"),
        "---\nname: native\n---\n",
    );
    write_text(
        &home.path().join(".claude/skills/from-claude/SKILL.md"),
        "---\nname: from-claude\n---\n",
    );
    write_text(
        &home.path().join(".cursor/skills/from-cursor/SKILL.md"),
        "---\nname: from-cursor\n---\n",
    );

    let enabled = scan_grok(home.path(), Path::new(""));
    let enabled_ids = kind_ids(
        &enabled.iter().collect::<Vec<_>>(),
        ConversationContextKind::Skill,
    );
    assert!(enabled_ids.contains(&"user:native".into()));
    assert!(enabled_ids.contains(&"claude:from-claude".into()));
    assert!(enabled_ids.contains(&"cursor:from-cursor".into()));

    write_text(
        &home.path().join(".grok/config.toml"),
        concat!(
            "[compat.claude]\n",
            "skills = false\n",
            "[compat.cursor]\n",
            "skills = false\n",
        ),
    );
    let disabled = scan_grok(home.path(), Path::new(""));
    let disabled_ids = kind_ids(
        &disabled.iter().collect::<Vec<_>>(),
        ConversationContextKind::Skill,
    );
    assert_eq!(disabled_ids, vec!["user:native".to_string()]);
}

#[test]
fn grok_disk_merges_mcp_sources_with_config_toml_winning() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".git")).unwrap();
    write_text(
        &home.path().join(".grok/config.toml"),
        concat!(
            "[mcp_servers.dup]\n",
            "command = \"toml\"\n",
            "[mcp_servers.toml-only]\n",
            "command = \"toml\"\n",
        ),
    );
    write_text(
        &home.path().join(".claude.json"),
        r#"{"mcpServers":{"dup":{"command":"claude"},"claude-only":{"command":"claude"}}}"#,
    );
    write_text(
        &home.path().join(".cursor/mcp.json"),
        r#"{"mcpServers":{"dup":{"command":"cursor"},"cursor-only":{"command":"cursor"}}}"#,
    );
    write_text(
        &project.path().join(".mcp.json"),
        r#"{"mcpServers":{"dup":{"command":"json"},"json-only":{"command":"json"}}}"#,
    );

    let items = scan_grok(home.path(), project.path());
    let mcp: Vec<_> = items
        .iter()
        .filter(|item| item.kind == ConversationContextKind::McpServer)
        .collect();
    let ids = kind_ids(&mcp, ConversationContextKind::McpServer);
    assert!(ids.contains(&"grok:dup".into()));
    assert!(ids.contains(&"grok:toml-only".into()));
    assert!(ids.contains(&"claude:claude-only".into()));
    assert!(ids.contains(&"cursor:cursor-only".into()));
    assert!(ids.contains(&"mcp_json:json-only".into()));
    assert!(!ids
        .iter()
        .any(|id| id.ends_with(":dup") && !id.starts_with("grok:")));
    let dup = mcp.iter().find(|item| item.label == "dup").unwrap();
    assert_eq!(item_scope(dup), Some("grok"));
    assert_eq!(
        item_scope(mcp.iter().find(|item| item.label == "claude-only").unwrap()),
        Some("claude")
    );
    assert_eq!(
        item_scope(mcp.iter().find(|item| item.label == "cursor-only").unwrap()),
        Some("cursor")
    );
    assert_eq!(
        item_scope(mcp.iter().find(|item| item.label == "json-only").unwrap()),
        Some("mcp_json")
    );
}

#[test]
fn grok_disk_toml_disabled_mcp_occupies_name_against_lower_sources() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".git")).unwrap();
    write_text(
        &home.path().join(".grok/config.toml"),
        concat!(
            "[mcp_servers.dup]\n",
            "command = \"toml\"\n",
            "enabled = false\n",
            "[mcp_servers.toml-only]\n",
            "command = \"toml\"\n",
        ),
    );
    write_text(
        &home.path().join(".claude.json"),
        r#"{"mcpServers":{"dup":{"command":"claude"},"claude-only":{"command":"claude"}}}"#,
    );

    let items = scan_grok(home.path(), project.path());
    let ids = kind_ids(
        &items.iter().collect::<Vec<_>>(),
        ConversationContextKind::McpServer,
    );
    assert!(ids.contains(&"grok:toml-only".into()));
    assert!(ids.contains(&"claude:claude-only".into()));
    assert!(!ids.iter().any(|id| id.contains("dup")));
}

#[test]
fn grok_disk_project_toml_deepest_wins_over_user_and_git_root() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    let nested = repo.path().join("nested");
    write_text(
        &home.path().join(".grok/config.toml"),
        concat!(
            "[mcp_servers.shared]\n",
            "command = \"user\"\n",
            "[mcp_servers.user-only]\n",
            "command = \"user\"\n",
        ),
    );
    write_text(
        &repo.path().join(".grok/config.toml"),
        concat!(
            "[mcp_servers.shared]\n",
            "command = \"root\"\n",
            "[mcp_servers.root-only]\n",
            "command = \"root\"\n",
        ),
    );
    write_text(
        &nested.join(".grok/config.toml"),
        concat!("[mcp_servers.shared]\n", "command = \"deep\"\n",),
    );

    let items = scan_grok(home.path(), &nested);
    let mcp: Vec<_> = items
        .iter()
        .filter(|item| item.kind == ConversationContextKind::McpServer)
        .collect();
    let ids = kind_ids(&mcp, ConversationContextKind::McpServer);
    assert!(ids.contains(&"grok-project:shared".into()));
    assert!(ids.contains(&"grok-project:root-only".into()));
    assert!(ids.contains(&"grok:user-only".into()));
    let shared = mcp.iter().find(|item| item.label == "shared").unwrap();
    assert_eq!(item_scope(shared), Some("grok-project"));
    assert!(shared
        .path
        .as_deref()
        .is_some_and(|path| path.ends_with("nested/.grok/config.toml")));
}

#[test]
fn grok_disk_compat_mcp_off_skips_vendor_sources() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".git")).unwrap();
    write_text(
        &home.path().join(".grok/config.toml"),
        concat!(
            "[compat.claude]\n",
            "mcps = false\n",
            "[compat.cursor]\n",
            "mcps = false\n",
            "[mcp_servers.toml-only]\n",
            "command = \"toml\"\n",
        ),
    );
    write_text(
        &home.path().join(".claude.json"),
        r#"{"mcpServers":{"claude-only":{"command":"claude"}}}"#,
    );
    write_text(
        &home.path().join(".cursor/mcp.json"),
        r#"{"mcpServers":{"cursor-only":{"command":"cursor"}}}"#,
    );
    write_text(
        &project.path().join(".mcp.json"),
        r#"{"mcpServers":{"json-only":{"command":"json"}}}"#,
    );

    let items = scan_grok(home.path(), project.path());
    let ids = kind_ids(
        &items.iter().collect::<Vec<_>>(),
        ConversationContextKind::McpServer,
    );
    assert_eq!(
        ids,
        vec![
            "grok:toml-only".to_string(),
            "mcp_json:json-only".to_string()
        ]
    );
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
    assert!(
        layer_items(&manifest.items, ConversationContextLayer::Injected).is_empty(),
        "missing prompt_context.json must not invent injected instructions"
    );
    assert!(manifest.on_disk_note.as_deref().is_some_and(|note| {
        note.contains("可能生效")
            && note.contains("skills")
            && note.contains("MCP")
            && !note.contains("已注入")
            && !note.contains("未扫描")
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
    assert!(observed
        .iter()
        .any(|item| item.kind == ConversationContextKind::SystemStatus
            && item.id == "subagent_spawned"));
    assert!(observed
        .iter()
        .any(|item| item.kind == ConversationContextKind::SystemStatus
            && item.id == "subagent_finished"));
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
fn grok_detail_lists_skills_and_mcp_from_session_project() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".git")).unwrap();
    let project_cwd = project.path().to_string_lossy().into_owned();
    let encoded = urlencoding::encode(&project_cwd);
    seed_grok_session_at(home, encoded.as_ref(), "sess-grok-skill-mcp");
    write_text(
        &home.join(".grok/skills/commit/SKILL.md"),
        "---\nname: commit\n---\n",
    );
    write_text(
        &home.join(".grok/config.toml"),
        "[mcp_servers.docs]\ncommand = \"npx\"\n",
    );
    write_text(
        &project.path().join(".mcp.json"),
        r#"{"mcpServers":{"proj":{"command":"npx"}}}"#,
    );

    let conn = store::open_memory().unwrap();
    refresh_grok(&conn, home);
    let detail =
        crate::conversation::load_detail(&conn, home, "grok", "sess-grok-skill-mcp").unwrap();
    let manifest = detail.context_manifest.unwrap();
    assert_no_injected(
        &manifest.items,
        &[
            manifest.observed_note.as_deref(),
            manifest.on_disk_note.as_deref(),
        ],
    );
    let possible = layer_items(&manifest.items, ConversationContextLayer::OnDiskPossible);
    assert!(kind_ids(&possible, ConversationContextKind::Skill).contains(&"user:commit".into()));
    assert!(kind_ids(&possible, ConversationContextKind::McpServer).contains(&"grok:docs".into()));
    assert!(
        kind_ids(&possible, ConversationContextKind::McpServer).contains(&"mcp_json:proj".into())
    );
}

#[test]
fn grok_context_omits_missing_home_files_and_does_not_invent_agents() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let project = tempfile::tempdir().unwrap();
    seed_grok_session(home, "sess-grok-empty");
    write_text(&project.path().join("AGENTS.md"), "project-only\n");
    write_text(
        &home.join(".grok/sessions/README.md"),
        "not an instruction\n",
    );

    let conn = store::open_memory().unwrap();
    refresh_grok(&conn, home);

    let detail = crate::conversation::load_detail(&conn, home, "grok", "sess-grok-empty").unwrap();
    let manifest = detail.context_manifest.unwrap();
    let possible = layer_items(&manifest.items, ConversationContextLayer::OnDiskPossible);
    assert!(possible.is_empty(), "{possible:?}");
    assert!(manifest.on_disk_note.as_deref().is_some_and(|note| {
        note.contains("未发现")
            && note.contains("skills")
            && note.contains("MCP")
            && !note.contains("已注入")
            && !note.contains("未扫描")
    }));
    assert!(!manifest.items.iter().any(|item| item.id == "AGENTS.md"
        || item.label == "AGENTS.md"
        || item.id.contains("project-only")));
}

#[test]
fn cursor_missing_transcript_does_not_treat_synthetic_status_as_observed() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_text(
        &home.join(".cursor-agent-usage/sess-usage-only.jsonl"),
        concat!(
            "{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"cursor-test-model\",\"cwd\":\"/workspace/project\",\"session_id\":\"sess-usage-only\"}\n",
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"session_id\":\"sess-usage-only\",\"request_id\":\"request-sess-usage-only\",\"duration_ms\":1000,\"usage\":{\"inputTokens\":10,\"outputTokens\":5,\"cacheReadTokens\":2,\"cacheWriteTokens\":1},\"captured_at\":\"2026-08-22T02:00:00Z\"}\n"
        ),
    );
    let conn = store::open_memory().unwrap();
    ingest::ingest_all_with_overrides(&conn, home, &Default::default()).unwrap();

    let detail =
        crate::conversation::load_detail(&conn, home, "cursor_agent", "sess-usage-only").unwrap();
    let manifest = detail
        .context_manifest
        .as_ref()
        .expect("cursor detail should still carry a context manifest");
    let observed = layer_items(&manifest.items, ConversationContextLayer::Observed);
    assert!(
        observed.is_empty(),
        "synthetic transcript_missing must not count as observed: {observed:?}"
    );
    assert!(manifest
        .observed_note
        .as_deref()
        .is_some_and(|note| { note.contains("无法确认") && !note.contains("已注入") }));
    assert!(observed.iter().all(|item| item.id != "transcript_missing"));
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

#[test]
fn grok_detail_lists_injected_agents_md_from_prompt_context() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    const PROJECT_BODY: &str = "UNIQUE_GROK_INJECT_BODY_project";
    const GLOBAL_BODY: &str = "UNIQUE_GROK_INJECT_BODY_global";
    let injected_updates = seed_grok_session(home, "sess-grok-injected");
    write_grok_prompt_context(
        &injected_updates,
        &[
            ("AGENTS.md", "/workspace/proj/AGENTS.md", PROJECT_BODY),
            ("Agents.md", "/tmp/home/.grok/Agents.md", GLOBAL_BODY),
        ],
    );
    seed_grok_session(home, "sess-grok-plain");
    let bad_updates = seed_grok_session(home, "sess-grok-bad-ctx");
    write_text(
        &bad_updates.parent().unwrap().join("prompt_context.json"),
        "{not-json",
    );

    let conn = store::open_memory().unwrap();
    refresh_grok(&conn, home);

    let injected =
        crate::conversation::load_detail(&conn, home, "grok", "sess-grok-injected").unwrap();
    let plain = crate::conversation::load_detail(&conn, home, "grok", "sess-grok-plain").unwrap();
    let bad = crate::conversation::load_detail(&conn, home, "grok", "sess-grok-bad-ctx").unwrap();
    assert_eq!(injected.event_count, plain.event_count);
    assert_eq!(bad.event_count, plain.event_count);

    let manifest = injected
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
    assert!(manifest
        .injected_note
        .as_deref()
        .is_some_and(|note| note.contains("已注入")));
    assert!(manifest
        .on_disk_note
        .as_deref()
        .is_some_and(|note| !note.contains("已注入")));

    let items = layer_items(&manifest.items, ConversationContextLayer::Injected);
    assert_eq!(items.len(), 2, "{items:?}");
    assert!(items.iter().all(|item| {
        item.kind == ConversationContextKind::Instruction
            && item.load_mode == Some(ConversationContextLoadMode::Always)
    }));

    let project = items
        .iter()
        .find(|item| item.path.as_deref() == Some("/workspace/proj/AGENTS.md"))
        .expect("project AGENTS.md should be injected");
    assert_eq!(project.id, "/workspace/proj/AGENTS.md");
    assert_eq!(project.label, "AGENTS.md");
    assert_eq!(
        project.char_count,
        Some(PROJECT_BODY.chars().count() as u64)
    );

    let global = items
        .iter()
        .find(|item| item.path.as_deref() == Some("/tmp/home/.grok/Agents.md"))
        .expect("user-level Agents.md should be injected");
    assert_eq!(global.label, "Agents.md");
    assert_eq!(global.char_count, Some(GLOBAL_BODY.chars().count() as u64));

    let dto_json = serde_json::to_string(&injected).unwrap();
    assert!(
        !dto_json.contains(PROJECT_BODY) && !dto_json.contains(GLOBAL_BODY),
        "injection body must not enter the detail DTO"
    );
    let event_hits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM conversation_events WHERE coalesce(text, '') LIKE '%UNIQUE_GROK_INJECT_BODY%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        event_hits, 0,
        "injection body must not enter conversation_events"
    );

    let plain_items = layer_items(
        &plain.context_manifest.as_ref().unwrap().items,
        ConversationContextLayer::Injected,
    );
    assert!(plain_items.is_empty(), "{plain_items:?}");
    let bad_items = layer_items(
        &bad.context_manifest.as_ref().unwrap().items,
        ConversationContextLayer::Injected,
    );
    assert!(bad_items.is_empty(), "{bad_items:?}");
}

#[test]
fn grok_detail_lists_injected_mcp_three_states_and_noise_set_diff() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    const PROJECT_BODY: &str = "UNIQUE_GROK_MCP_INSTRUCTION";
    const ERROR_BODY: &str = "UNIQUE_MCP_ERROR_BODY handshake failed";
    let updates = seed_grok_session(home, "sess-grok-mcp");
    write_grok_prompt_context(
        &updates,
        &[("AGENTS.md", "/workspace/proj/AGENTS.md", PROJECT_BODY)],
    );
    append_grok_tool_call(&updates, "alpha_docs");
    write_grok_events(
        &updates,
        &[
            serde_json::json!({
                "ts": "2026-09-08T00:00:00Z",
                "type": "mcp_config_resolved",
                "servers": [
                    {"name": "docs", "transport": "stdio", "source": "local"},
                    {"name": "idle", "transport": "stdio", "source": "local"},
                    {"name": "figma", "transport": "http", "source": "local"},
                    {"name": "chrome", "transport": "stdio", "source": "local"},
                    {"name": "legacy", "transport": "stdio", "source": "local"}
                ],
                "disabled": ["legacy"]
            }),
            serde_json::json!({
                "ts": "2026-09-08T00:00:01Z",
                "type": "mcp_server_connected",
                "server_name": "docs",
                "transport": "stdio",
                "tool_count": 2,
                "tools": ["alpha_docs", "beta_ping"]
            }),
            serde_json::json!({
                "ts": "2026-09-08T00:00:02Z",
                "type": "mcp_server_connected",
                "server_name": "idle",
                "transport": "stdio",
                "tool_count": 1,
                "tools": ["zzz_idle"]
            }),
            serde_json::json!({
                "ts": "2026-09-08T00:00:03Z",
                "type": "mcp_server_failed",
                "server_name": "figma",
                "transport": "http",
                "error_type": "auth_required",
                "error_message": ERROR_BODY
            }),
            serde_json::json!({
                "ts": "2026-09-08T00:00:03Z",
                "type": "mcp_server_failed",
                "server_name": "chrome",
                "transport": "stdio",
                "error_type": "handshake_failed",
                "error_message": ERROR_BODY
            }),
            serde_json::json!({
                "ts": "2026-09-08T00:00:04Z",
                "type": "mcp_init_completed",
                "total_servers": 5,
                "succeeded": 2,
                "failed": 2,
                "auth_required": 1,
                "total_tools": 3,
                "duration_ms": 12,
                "is_reinit": false,
                "failed_servers": ["figma", "chrome"]
            }),
        ],
    );

    let conn = store::open_memory().unwrap();
    refresh_grok(&conn, home);
    let detail = crate::conversation::load_detail(&conn, home, "grok", "sess-grok-mcp").unwrap();
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
    assert_eq!(
        manifest.mcp_init_summary.as_deref(),
        Some("配置 5 台 / 连上 2 台 / 失败 2 台 / 共注入 3 个工具")
    );

    let injected = layer_items(&manifest.items, ConversationContextLayer::Injected);
    let instruction = injected
        .iter()
        .find(|item| item.kind == ConversationContextKind::Instruction)
        .expect("instruction should still be injected");
    assert!(!instruction.is_noise, "{instruction:?}");

    let docs = mcp_item(&injected, "docs");
    assert_eq!(
        docs.injection_status,
        Some(ConversationContextInjectionStatus::Connected)
    );
    assert_eq!(docs.load_mode, Some(ConversationContextLoadMode::Always));
    assert_eq!(
        docs.char_count,
        Some("alpha_docs,beta_ping".chars().count() as u64)
    );
    assert!(!docs.is_noise, "called MCP must not be noise: {docs:?}");
    assert_eq!(
        docs.meta
            .as_ref()
            .and_then(|meta| meta.get("tool_count"))
            .and_then(|value| value.as_u64()),
        Some(2)
    );

    let idle = mcp_item(&injected, "idle");
    assert_eq!(
        idle.injection_status,
        Some(ConversationContextInjectionStatus::Connected)
    );
    assert!(idle.is_noise, "zero-call connected MCP is noise: {idle:?}");
    assert_eq!(idle.char_count, Some("zzz_idle".chars().count() as u64));

    let figma = mcp_item(&injected, "figma");
    assert_eq!(
        figma.injection_status,
        Some(ConversationContextInjectionStatus::AuthRequired)
    );
    assert!(!figma.is_noise, "failed MCP must not take the noise flag");
    assert_eq!(figma.char_count, None);
    assert_eq!(
        figma
            .meta
            .as_ref()
            .and_then(|meta| meta.get("error_type"))
            .and_then(|value| value.as_str()),
        Some("auth_required")
    );

    let chrome = mcp_item(&injected, "chrome");
    assert_eq!(
        chrome.injection_status,
        Some(ConversationContextInjectionStatus::Failed)
    );
    assert!(!chrome.is_noise);
    assert_eq!(chrome.char_count, None);
    assert_eq!(
        chrome
            .meta
            .as_ref()
            .and_then(|meta| meta.get("error_type"))
            .and_then(|value| value.as_str()),
        Some("handshake_failed")
    );

    let legacy = mcp_item(&injected, "legacy");
    assert_eq!(
        legacy.injection_status,
        Some(ConversationContextInjectionStatus::Disabled)
    );
    assert!(!legacy.is_noise);
    assert_eq!(legacy.char_count, None);

    let dto_json = serde_json::to_string(&detail).unwrap();
    assert!(
        !dto_json.contains(ERROR_BODY) && !dto_json.contains("error_message"),
        "MCP error message must not enter the detail DTO"
    );
    let event_hits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM conversation_events WHERE coalesce(text, '') LIKE '%UNIQUE_MCP_ERROR_BODY%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        event_hits, 0,
        "MCP error message must not enter conversation_events"
    );
}

#[test]
fn grok_detail_treats_events_jsonl_mcp_tool_call_as_observed() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let updates = seed_grok_session(home, "sess-grok-mcp-call");
    write_grok_events(
        &updates,
        &[
            serde_json::json!({
                "type": "mcp_config_resolved",
                "servers": [{"name": "idle", "transport": "stdio", "source": "local"}],
                "disabled": []
            }),
            serde_json::json!({
                "type": "mcp_server_connected",
                "server_name": "idle",
                "tools": ["zzz_idle"]
            }),
            serde_json::json!({
                "type": "mcp_init_completed",
                "total_servers": 1,
                "succeeded": 1,
                "failed": 0,
                "total_tools": 1
            }),
            serde_json::json!({
                "type": "mcp_tool_call_started",
                "server_name": "idle",
                "tool_name": "zzz_idle",
                "call_id": "idle__zzz_idle"
            }),
        ],
    );

    let conn = store::open_memory().unwrap();
    refresh_grok(&conn, home);
    let detail =
        crate::conversation::load_detail(&conn, home, "grok", "sess-grok-mcp-call").unwrap();
    let manifest = detail.context_manifest.as_ref().unwrap();
    let injected = layer_items(&manifest.items, ConversationContextLayer::Injected);
    let idle = mcp_item(&injected, "idle");
    assert!(
        !idle.is_noise,
        "events.jsonl mcp_tool_call should count as observed: {idle:?}"
    );
}

#[test]
fn grok_detail_omits_mcp_summary_without_mcp_events() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_grok_session(home, "sess-grok-no-mcp");
    let conn = store::open_memory().unwrap();
    refresh_grok(&conn, home);
    let detail = crate::conversation::load_detail(&conn, home, "grok", "sess-grok-no-mcp").unwrap();
    let manifest = detail.context_manifest.as_ref().unwrap();
    assert_eq!(manifest.mcp_init_summary, None);
    assert!(
        layer_items(&manifest.items, ConversationContextLayer::Injected)
            .iter()
            .all(|item| item.kind != ConversationContextKind::McpServer)
    );
}
