use crate::test_support::*;

#[test]
fn codex_adapter_counts_last_token_usage_not_cumulative() {
    let records = codex::parse_codex_jsonl(
        &fixture_lines(&fixture("codex.jsonl")),
        "/Users/zhangyanhua/.codex/sessions/rollout.jsonl",
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source, Source::Codex);
    assert_eq!(records[0].model, "gpt-5.1-codex");
    assert_eq!(records[0].provider, "codex_local_access");
    assert_eq!(
        records[0].project,
        "/Users/zhangyanhua/AI/chord-creator-studio"
    );
    assert_eq!(
        records[0].session_id,
        "019a9618-5abf-7892-be63-df90ece3d676"
    );
    assert_eq!(records[0].input_tokens, 8904);
    assert_eq!(records[0].cache_read_tokens, 1024);
    assert_eq!(records[0].output_tokens, 592);
    assert_eq!(records[0].total_tokens, 9496);
    assert_eq!(records[1].input_tokens, 9509);
    assert_eq!(records[1].output_tokens, 108);
    assert_eq!(records[1].reasoning_tokens, 64);
    assert_eq!(records[1].total_tokens, 9617);
    let summed: i64 = records.iter().map(|r| r.total_tokens).sum();
    assert_eq!(summed, 19113);
    assert_ne!(summed, 9496 + 19113);
}

#[test]
fn codex_adapter_falls_back_to_total_token_usage_delta() {
    let records = codex::parse_codex_jsonl(
        &fixture_lines(&fixture("codex-total-only.jsonl")),
        "/Users/zhangyanhua/.codex/sessions/rollout.jsonl",
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].input_tokens, 100);
    assert_eq!(records[0].cache_read_tokens, 50);
    assert_eq!(records[0].output_tokens, 10);
    assert_eq!(records[1].input_tokens, 50);
    assert_eq!(records[1].cache_read_tokens, 25);
    assert_eq!(records[1].output_tokens, 5);
    let summed: i64 = records.iter().map(|r| r.input_tokens).sum();
    assert_eq!(summed, 150);
}

#[test]
fn claude_adapter_maps_usage_and_project_dir() {
    let records = claude::parse_claude_jsonl(
        &fixture_lines(&fixture("claude.jsonl")),
        "/Users/zhangyanhua/.claude/projects/-Users-zhangyanhua-AI-TradingAgents-CN/04868551-34c3-4588-b984-6ae9a5d95f8a.jsonl",
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source, Source::Claude);
    assert_eq!(records[0].model, "claude-sonnet-5");
    assert_eq!(
        records[0].session_id,
        "04868551-34c3-4588-b984-6ae9a5d95f8a"
    );
    assert_eq!(records[0].project, "/Users/zhangyanhua/AI/TradingAgents-CN");
    assert_eq!(records[0].input_tokens, 0);
    assert_eq!(records[0].output_tokens, 62);
    assert_eq!(records[0].cache_read_tokens, 0);
    assert_eq!(records[0].cache_creation_tokens, 56332);
    assert_eq!(records[0].reasoning_tokens, 0);
    assert_eq!(records[0].total_tokens, 56394);
    assert_eq!(records[1].input_tokens, 120);
    assert_eq!(records[1].output_tokens, 40);
    assert_eq!(records[1].cache_read_tokens, 56332);
    assert_eq!(records[1].cache_creation_tokens, 0);
    assert_eq!(records[1].total_tokens, 56492);
    assert!((records[0].native_cost.unwrap() - 0.0123).abs() < 1e-9);
    assert!((records[1].native_cost.unwrap() - 0.0081).abs() < 1e-9);
}

#[test]
fn claude_adapter_uses_structured_agent_id_for_child_usage() {
    let records = claude::parse_claude_jsonl(
        &fixture_lines(&fixture("claude-subagent-conversation.jsonl")),
        "/Users/example/.claude/projects/-workspace-app/session/subagents/agent-claude-child-1.jsonl",
    );

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].session_id, "claude-child-1");
    assert_eq!(records[0].input_tokens, 3);
    assert_eq!(records[0].output_tokens, 2);
}

#[test]
fn claude_adapter_dedups_message_id_and_skips_zero_usage() {
    let records = claude::parse_claude_jsonl(
        &fixture_lines(&fixture("claude-dedup.jsonl")),
        "/Users/zhangyanhua/.claude/projects/-Users-zhangyanhua-AI-cli/s-claude.jsonl",
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].input_tokens, 2);
    assert_eq!(records[0].output_tokens, 80);
    assert_eq!(records[0].cache_read_tokens, 48719);
    assert_eq!(records[0].cache_creation_tokens, 2061);
    assert!((records[0].native_cost.unwrap() - 0.05).abs() < 1e-9);
    assert_eq!(records[1].input_tokens, 10);
    assert_eq!(records[1].output_tokens, 4);
    assert!(records[1].native_cost.is_none());
}

#[test]
fn pi_adapter_uses_native_cost() {
    let records = pi::parse_pi_jsonl(
        &fixture_lines(&fixture("pi.jsonl")),
        "/Users/zhangyanhua/.pi/agent/sessions/--Users-zhangyanhua-workCode-ruoyi-ui-vue3--/s.jsonl",
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source, Source::Pi);
    assert_eq!(records[0].model, "gpt-5.5");
    assert_eq!(records[0].provider, "subapi");
    assert_eq!(
        records[0].project,
        "/Users/zhangyanhua/workCode/ruoyi-ui-vue3"
    );
    assert_eq!(
        records[0].session_id,
        "019f5abc-b360-79e4-bd7d-9a794da8cfc5"
    );
    assert_eq!(records[0].input_tokens, 12658);
    assert_eq!(records[0].output_tokens, 35);
    assert_eq!(records[0].cache_read_tokens, 0);
    assert_eq!(records[0].cache_creation_tokens, 0);
    assert_eq!(records[0].reasoning_tokens, 12);
    assert_eq!(records[0].total_tokens, 12693);
    assert_eq!(records[0].native_cost, Some(0.06434));
    assert_eq!(records[1].input_tokens, 517);
    assert_eq!(records[1].output_tokens, 41);
    assert_eq!(records[1].cache_read_tokens, 12288);
    assert_eq!(records[1].cache_creation_tokens, 0);
    assert_eq!(records[1].reasoning_tokens, 13);
    assert_eq!(records[1].total_tokens, 12846);
    assert!((records[1].native_cost.unwrap() - 0.009959).abs() < 1e-9);
}

#[test]
fn pi_adapter_skips_zero_token_assistant_messages() {
    // fixture 里追加了一条 usage 四分项全 0 的 assistant 消息（a3），
    // 与其它 adapter（claude/codex/gemini/opencode）保持一致：不计入会话/费用统计。
    let records = pi::parse_pi_jsonl(
        &fixture_lines(&fixture("pi.jsonl")),
        "/Users/zhangyanhua/.pi/agent/sessions/--Users-zhangyanhua-workCode-ruoyi-ui-vue3--/s.jsonl",
    );
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|r| r.total_tokens > 0));
}

#[test]
fn omp_adapter_uses_native_cost_and_skips_zero_usage() {
    let records = omp::parse_omp_jsonl(
        &fixture_lines(&fixture("omp.jsonl")),
        "/workspace/.omp/agent/sessions/-workspace-app/2026-08-31T10-00-00-000Z_01a00000-1111-7000-8000-aaaaaaaaaaaa.jsonl",
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source, Source::Omp);
    assert_eq!(records[0].model, "grok-4.6");
    assert_eq!(records[0].provider, "xai-oauth");
    assert_eq!(records[0].project, "/workspace/app");
    assert_eq!(
        records[0].session_id,
        "01a00000-1111-7000-8000-aaaaaaaaaaaa"
    );
    assert_eq!(records[0].input_tokens, 10);
    assert_eq!(records[0].output_tokens, 5);
    assert_eq!(records[0].cache_read_tokens, 100);
    assert_eq!(records[0].cache_creation_tokens, 2);
    assert_eq!(records[0].reasoning_tokens, 3);
    assert_eq!(records[0].total_tokens, 120);
    assert_eq!(records[0].native_cost, Some(0.01));
    assert_eq!(records[1].input_tokens, 20);
    assert_eq!(records[1].output_tokens, 8);
    assert_eq!(records[1].cache_read_tokens, 50);
    assert_eq!(records[1].total_tokens, 79);
    assert_eq!(records[1].native_cost, Some(0.02));
    assert!(records.iter().all(|record| record.total_tokens > 0));
}

#[test]
fn omp_adapter_attributes_subagent_usage_to_parent_session() {
    let dir = tempfile::tempdir().unwrap();
    let parent_stem = "2026-08-31T10-00-00-000Z_01a00000-1111-7000-8000-aaaaaaaaaaaa";
    let cwd = dir.path().join("-workspace-app");
    std::fs::create_dir_all(cwd.join(parent_stem)).unwrap();
    let parent = cwd.join(format!("{parent_stem}.jsonl"));
    let nested = cwd.join(parent_stem).join("Scout.jsonl");
    std::fs::write(&parent, fixture("omp.jsonl")).unwrap();
    std::fs::write(
        &nested,
        concat!(
            r#"{"type":"session","version":3,"id":"scout-1","timestamp":"2026-08-31T10:01:00.000Z","cwd":"/workspace/app"}"#,
            "\n",
            r#"{"type":"message","id":"a1","timestamp":"2026-08-31T10:01:01.000Z","message":{"role":"assistant","provider":"xai-oauth","model":"grok-4.6","usage":{"input":7,"output":3,"cacheRead":0,"cacheWrite":0,"reasoning":0,"totalTokens":10,"cost":{"total":0.003}}}}"#,
            "\n",
        ),
    )
    .unwrap();

    let records = omp::parse(&nested, dir.path()).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].session_id,
        "01a00000-1111-7000-8000-aaaaaaaaaaaa"
    );
    assert_eq!(records[0].input_tokens, 7);
    assert_eq!(records[0].native_cost, Some(0.003));
}

#[test]
fn omp_sidecar_fingerprint_changes_when_parent_jsonl_appears() {
    let dir = tempfile::tempdir().unwrap();
    let parent_stem = "2026-08-31T10-00-00-000Z_01a00000-1111-7000-8000-aaaaaaaaaaaa";
    let cwd = dir.path().join("-workspace-app");
    std::fs::create_dir_all(cwd.join(parent_stem)).unwrap();
    let nested = cwd.join(parent_stem).join("Scout.jsonl");
    std::fs::write(&nested, "{}\n").unwrap();

    let missing = omp::sidecar_fingerprint(&nested, &[]);
    assert_eq!(missing, "missing");

    std::fs::write(
        cwd.join(format!("{parent_stem}.jsonl")),
        fixture("omp.jsonl"),
    )
    .unwrap();
    let present = omp::sidecar_fingerprint(&nested, &[]);
    assert_ne!(present, "missing");
    assert_ne!(present, missing);
}

#[test]
fn opencode_adapter_skips_user_and_keeps_native_cost() {
    let raw = fixture("opencode-messages.json");
    let values: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
    let rows: Vec<OpencodeMessage> = values
        .into_iter()
        .map(|v| OpencodeMessage {
            session_id: v["session_id"].as_str().unwrap().to_string(),
            source_file: "opencode.db".to_string(),
            data: v["data"].clone(),
        })
        .collect();
    let records = parse_opencode_messages(&rows);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source, Source::Opencode);
    assert_eq!(records[0].model, "gemini-claude-sonnet-4-5-thinking");
    assert_eq!(records[0].provider, "anthropic");
    assert_eq!(
        records[0].project,
        "/Users/zhangyanhua/workCode/project_front"
    );
    assert_eq!(records[0].session_id, "ses_4064c35bcffeKnRpPdbo4Ege2l");
    assert_eq!(records[0].input_tokens, 20882);
    assert_eq!(records[0].output_tokens, 138);
    assert_eq!(records[0].cache_read_tokens, 100);
    assert_eq!(records[0].cache_creation_tokens, 20);
    assert_eq!(records[0].reasoning_tokens, 0);
    assert_eq!(records[0].total_tokens, 21140);
    assert_eq!(records[0].native_cost, Some(0.42));
}

#[test]
fn opencode_adapter_ignores_zero_native_cost() {
    let rows = [OpencodeMessage {
        session_id: "s1".to_string(),
        source_file: "opencode.db".to_string(),
        data: serde_json::json!({
            "role": "assistant",
            "modelID": "mimo-v2.5-pro",
            "time": { "created": 1, "completed": 2 },
            "tokens": { "input": 1000, "output": 200 },
            "cost": 0.0
        }),
    }];
    let records = parse_opencode_messages(&rows);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].input_tokens, 1000);
    assert_eq!(records[0].output_tokens, 200);
    assert_eq!(records[0].native_cost, None);
}

#[test]
fn kimi_adapter_keeps_last_status_update_per_turn() {
    let records = kimi::parse_kimi_wire(
        &fixture("kimi-wire.jsonl"),
        "/Users/zhangyanhua/.kimi/sessions/hash/bd1ab6fc-768d-4cff-b4c4-221a583c3af8/wire.jsonl",
        "/Users/zhangyanhua/workCode/app-storage",
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source, Source::Kimi);
    assert_eq!(
        records[0].session_id,
        "bd1ab6fc-768d-4cff-b4c4-221a583c3af8"
    );
    assert_eq!(
        records[0].project,
        "/Users/zhangyanhua/workCode/app-storage"
    );
    assert_eq!(records[0].model, "");
    assert_eq!(records[0].input_tokens, 3000);
    assert_eq!(records[0].output_tokens, 200);
    assert_eq!(records[0].cache_read_tokens, 4352);
    assert_eq!(records[0].cache_creation_tokens, 10);
    assert_eq!(records[0].reasoning_tokens, 0);
    assert_eq!(records[0].total_tokens, 7562);
    assert_ne!(records[0].input_tokens, 2547);
    assert_ne!(records[0].output_tokens, 142);
    assert_eq!(records[1].input_tokens, 330);
    assert_eq!(records[1].output_tokens, 339);
    assert_eq!(records[1].cache_read_tokens, 6656);
    assert_eq!(records[1].cache_creation_tokens, 0);
    assert_eq!(records[1].total_tokens, 7325);
}

#[test]
fn dsh_adapter_reads_final_assistant_turn_not_chunks() {
    let records = dsh::parse_dsh_jsonl(
        &fixture("dsh.jsonl"),
        "/Users/zhangyanhua/.dsh/sessions/--Users-zhangyanhua-AI-pi--/session.jsonl.zstd",
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source, Source::Dsh);
    assert_eq!(records[0].model, "deepseek-v4-flash");
    assert_eq!(records[0].provider, "deepseek-official");
    assert_eq!(records[0].project, "/Users/zhangyanhua/AI/pi");
    assert_eq!(
        records[0].session_id,
        "session-f1cbbe01-e379-4152-8d13-46440f595d2d"
    );
    assert_eq!(records[0].input_tokens, 13672);
    assert_eq!(records[0].output_tokens, 442);
    assert_eq!(records[0].cache_read_tokens, 0);
    assert_eq!(records[0].cache_creation_tokens, 0);
    assert_eq!(records[0].reasoning_tokens, 321);
    assert_eq!(records[0].total_tokens, 14435);
    assert_ne!(records[0].input_tokens, 1);
    assert_eq!(records[1].input_tokens, 1603);
    assert_eq!(records[1].output_tokens, 430);
    assert_eq!(records[1].cache_read_tokens, 14080);
    assert_eq!(records[1].reasoning_tokens, 281);
    assert_eq!(records[1].total_tokens, 16394);
}

#[test]
fn dsh_adapter_reads_compressed_session_as_usage_records() {
    let raw = fixture("dsh.jsonl");
    let compressed = zstd::encode_all(raw.as_bytes(), 0).unwrap();
    let records = dsh::parse_dsh_zstd(&compressed, "session.jsonl.zstd").unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].input_tokens, 13672);
    assert_eq!(records[0].total_tokens, 14435);
    assert_eq!(records[1].cache_read_tokens, 14080);
}

#[test]
fn gemini_adapter_maps_chat_tokens() {
    let records = gemini::parse_gemini_session(
        &fixture("gemini-session.json"),
        "/Users/zhangyanhua/.gemini/tmp/ruoyi-ui-vue3/chats/session-2026-03-07.json",
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source, Source::Gemini);
    assert_eq!(records[0].model, "gemini-3-flash-preview");
    assert_eq!(
        records[0].session_id,
        "2392a2f0-142a-407e-a08f-8f37781ba76c"
    );
    assert_eq!(records[0].project, "ruoyi-ui-vue3");
    assert_eq!(records[0].input_tokens, 13354);
    assert_eq!(records[0].output_tokens, 662);
    assert_eq!(records[0].cache_read_tokens, 0);
    assert_eq!(records[0].cache_creation_tokens, 0);
    assert_eq!(records[0].reasoning_tokens, 285);
    assert_eq!(records[0].total_tokens, 14301);
}

#[test]
fn grok_adapter_decodes_project_and_dedups_prompt() {
    let records = grok::parse_grok_updates(
        &fixture("grok-updates.jsonl"),
        "/Users/zhangyanhua/.grok/sessions/%2FUsers%2Fzhangyanhua%2FAI%2FTradingAgents-CN/019fd235/updates.jsonl",
        "grok-4.5",
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source, Source::Grok);
    assert_eq!(records[0].project, "/Users/zhangyanhua/AI/TradingAgents-CN");
    assert_eq!(records[0].session_id, "019fd235");
    assert_eq!(records[0].model, "grok-4.5");
    assert_eq!(records[0].input_tokens, 0);
    assert_eq!(records[0].output_tokens, 0);
    assert_eq!(records[0].cache_read_tokens, 0);
    assert_eq!(records[0].cache_creation_tokens, 0);
    assert_eq!(records[0].reasoning_tokens, 0);
    assert_eq!(records[0].total_tokens, 26857);
    assert_ne!(records[0].total_tokens, 15681);
    assert_eq!(records[1].total_tokens, 71351);
}

#[test]
fn grok_adapter_reads_turn_completed_usage_not_context_total() {
    let records = grok::parse_grok_updates(
        &fixture("grok-turn-completed.jsonl"),
        "/Users/zhangyanhua/.grok/sessions/%2FUsers%2Fzhangyanhua%2FAI%2FTradingAgents-CN/019fd235/updates.jsonl",
        "grok-4.5",
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source, Source::Grok);
    assert_eq!(records[0].project, "/Users/zhangyanhua/AI/TradingAgents-CN");
    assert_eq!(records[0].session_id, "019fd235");
    assert_eq!(records[0].model, "grok-4.6-build");
    assert_eq!(records[0].input_tokens, 447430);
    assert_eq!(records[0].output_tokens, 4742);
    assert_eq!(records[0].cache_read_tokens, 410112);
    assert_eq!(records[0].cache_creation_tokens, 0);
    assert_eq!(records[0].reasoning_tokens, 3567);
    assert_eq!(records[0].total_tokens, 452172);
    assert_ne!(records[0].total_tokens, 15681);
    assert!((records[0].native_cost.unwrap() - 0.308144).abs() < 1e-9);
    assert_eq!(records[1].input_tokens, 100);
    assert_eq!(records[1].output_tokens, 10);
    assert_eq!(records[1].cache_read_tokens, 5);
    assert_eq!(records[1].reasoning_tokens, 3);
    assert_eq!(records[1].total_tokens, 110);
    assert!((records[1].native_cost.unwrap() - 0.1).abs() < 1e-9);
}

#[test]
fn qwen_adapter_returns_empty_when_no_tokens() {
    let records = qwen::parse_qwen_session(&fixture("qwen-logs.json"), "logs.json");
    assert!(records.is_empty());
}

#[test]
fn factory_adapter_maps_session_token_usage() {
    let records = factory::parse_factory_settings(
        &fixture("factory.settings.json"),
        "/Users/zhangyanhua/.factory/sessions/-Users-zhangyanhua-AI-cli/9ab2ca7b-bd30-495b-9434-07892ee0e5e6.settings.json",
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source, Source::Factory);
    assert_eq!(records[0].provider, "anthropic");
    assert_eq!(records[0].model, "");
    assert_eq!(records[0].project, "/Users/zhangyanhua/AI/cli");
    assert_eq!(
        records[0].session_id,
        "9ab2ca7b-bd30-495b-9434-07892ee0e5e6"
    );
    assert_eq!(records[0].input_tokens, 3);
    assert_eq!(records[0].output_tokens, 1022);
    assert_eq!(records[0].cache_creation_tokens, 8125);
    assert_eq!(records[0].cache_read_tokens, 11084);
    assert_eq!(records[0].reasoning_tokens, 0);
    assert_eq!(records[0].total_tokens, 20234);
}

#[test]
fn cursor_agent_adapter_maps_result_usage_per_turn() {
    let records = cursor_agent::parse_cursor_agent_jsonl(
        &fixture_lines(&fixture("cursor-agent-stream.jsonl")),
        "/Users/dev/.cursor-agent-usage/3ce011d4-33d1-41d0-a16c-f6dc206c47f1.jsonl",
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source, Source::CursorAgent);
    assert_eq!(records[0].model, "Cursor Grok 4.6 High Fast");
    assert_eq!(records[0].provider, "");
    assert_eq!(records[0].project, "/Users/dev/project");
    assert_eq!(
        records[0].session_id,
        "3ce011d4-33d1-41d0-a16c-f6dc206c47f1"
    );
    assert_eq!(records[0].occurred_at, "2026-08-17T05:31:13.226190+00:00");
    assert_eq!(records[0].input_tokens, 18851);
    assert_eq!(records[0].output_tokens, 35);
    assert_eq!(records[0].cache_read_tokens, 0);
    assert_eq!(records[0].cache_creation_tokens, 0);
    assert_eq!(records[0].reasoning_tokens, 0);
    assert_eq!(records[0].total_tokens, 18886);
    assert!(records[0].native_cost.is_none());
    // 第二轮：cacheWriteTokens 映射到 cache_creation，total 为各口径之和。
    assert_eq!(records[1].cache_creation_tokens, 400);
    assert_eq!(records[1].total_tokens, 1000);
}

#[test]
fn copilot_adapter_only_uses_the_last_shutdown_snapshot_per_session() {
    let records = copilot::parse_copilot_jsonl(
        &fixture_lines(&fixture("copilot-events.jsonl")),
        "/Users/dev/.copilot/session-state/c0ffee11-2222-4333-8444-555566667777/events.jsonl",
    );
    // 文件里有两次 session.shutdown（会话续接两次）；只应采信最后一次的累计用量，
    // 否则会把第一次 shutdown 的 gpt-5.4 用量重复计入。
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source, Source::Copilot);
    assert_eq!(records[0].model, "claude-sonnet-4.5");
    assert_eq!(records[0].provider, "");
    assert_eq!(records[0].project, "/Users/dev/ai-usage-stats");
    assert_eq!(
        records[0].session_id,
        "c0ffee11-2222-4333-8444-555566667777"
    );
    assert_eq!(records[0].occurred_at, "2026-08-10T15:12:30.500Z");
    assert_eq!(records[0].input_tokens, 21583);
    assert_eq!(records[0].output_tokens, 1064);
    assert_eq!(records[0].cache_read_tokens, 21187);
    assert_eq!(records[0].cache_creation_tokens, 0);
    assert_eq!(records[0].total_tokens, 21583 + 1064 + 21187);

    assert_eq!(records[1].model, "gpt-5.4");
    assert_eq!(records[1].input_tokens, 244120);
    assert_eq!(records[1].output_tokens, 2383);
    assert_eq!(records[1].cache_read_tokens, 202112);
}

#[test]
fn copilot_adapter_falls_back_to_parent_dir_name_when_session_id_is_missing() {
    let content = r#"{"type":"session.shutdown","timestamp":"2026-08-11T00:00:00.000Z","data":{"modelMetrics":{"gpt-5.4":{"usage":{"inputTokens":10,"outputTokens":5,"cacheReadTokens":0,"cacheWriteTokens":0}}}}}"#;
    let records = copilot::parse_copilot_jsonl(
        &fixture_lines(content),
        "/Users/dev/.copilot/session-state/no-start-event/events.jsonl",
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].session_id, "no-start-event");
    assert_eq!(records[0].project, "");
}

#[test]
fn source_maps_to_user_facing_application_names() {
    assert_eq!(Source::Claude.application_name(), "Claude Code");
    assert_eq!(Source::Codex.application_name(), "Codex");
    assert_eq!(Source::Factory.application_name(), "Droid");
    assert_eq!(Source::Opencode.application_name(), "OpenCode");
    assert_eq!(Source::Dsh.application_name(), "DeepSeek Harness");
    assert_eq!(Source::CursorAgent.application_name(), "Cursor Agent");
    assert_eq!(Source::Omp.application_name(), "OMP");
    assert_eq!(Source::Copilot.application_name(), "GitHub Copilot CLI");
}

#[test]
fn application_breakdown_ranks_user_facing_apps() {
    let records = seed_records();
    let rows = aggregate::by_name(
        &records,
        &Filter::default(),
        &PriceTable::default(),
        |record| record.source.application_name().to_string(),
    );

    assert_eq!(rows[0].name, "Claude Code");
    assert_eq!(rows[0].total_tokens, 300);
    assert_eq!(rows[1].name, "Codex");
    assert_eq!(rows[1].total_tokens, 100);
    assert_eq!(rows[2].name, "Pi");
    assert_eq!(rows[2].total_tokens, 50);
}

#[test]
fn application_analytics_builds_trend_matrix_and_efficiency_metrics() {
    let mut codex_day_one = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "codex-session",
        100,
    );
    codex_day_one.input_tokens = 80;
    codex_day_one.cache_read_tokens = 20;
    codex_day_one.reasoning_tokens = 10;

    let mut codex_day_two = rec(
        "2026-08-02T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "codex-session",
        50,
    );
    codex_day_two.input_tokens = 40;
    codex_day_two.cache_read_tokens = 10;
    codex_day_two.reasoning_tokens = 5;

    let mut claude_project_a = rec(
        "2026-08-01T11:00:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/proj/a",
        "claude-a",
        200,
    );
    claude_project_a.input_tokens = 100;
    claude_project_a.cache_read_tokens = 100;
    claude_project_a.reasoning_tokens = 20;

    let mut claude_project_b = rec(
        "2026-08-02T11:00:00Z",
        Source::Claude,
        "claude-sonnet-5",
        "anthropic",
        "/proj/b",
        "claude-b",
        100,
    );
    claude_project_b.input_tokens = 0;

    let records = vec![
        codex_day_one,
        codex_day_two,
        claude_project_a,
        claude_project_b,
    ];
    let analytics = aggregate::application_analytics(&records, &Filter::default(), "day");

    assert_eq!(analytics.summary.total_tokens, 450);
    assert_eq!(analytics.summary.session_count, 3);
    assert_eq!(analytics.summary.average_session_tokens, Some(150.0));
    assert!((analytics.summary.cache_hit_rate.unwrap() - 130.0 / 350.0).abs() < 1e-9);
    assert!((analytics.summary.reasoning_share.unwrap() - 35.0 / 450.0).abs() < 1e-9);

    assert_eq!(analytics.by_application.len(), 2);
    assert_eq!(analytics.by_application[0].application, "Claude Code");
    assert_eq!(analytics.by_application[0].metrics.total_tokens, 300);
    assert_eq!(analytics.by_application[0].metrics.session_count, 2);
    assert_eq!(
        analytics.by_application[0].metrics.average_session_tokens,
        Some(150.0)
    );
    assert_eq!(
        analytics.by_application[0].metrics.cache_hit_rate,
        Some(0.5)
    );
    assert!(
        (analytics.by_application[0].metrics.reasoning_share.unwrap() - 1.0 / 15.0).abs() < 1e-9
    );
    assert_eq!(analytics.by_application[1].source, "codex");
    assert_eq!(
        analytics.by_application[1].metrics.cache_hit_rate,
        Some(0.2)
    );

    assert_eq!(analytics.trend.len(), 2);
    assert_eq!(analytics.trend[0].bucket, "2026-08-01");
    assert_eq!(analytics.trend[0].total_tokens, 300);
    assert_eq!(analytics.trend[0].values["codex"], 100);
    assert_eq!(analytics.trend[0].values["claude"], 200);
    assert_eq!(analytics.trend[1].total_tokens, 150);

    assert_eq!(analytics.projects.len(), 2);
    assert_eq!(analytics.projects[0].project, "/proj/a");
    assert_eq!(analytics.projects[0].total_tokens, 350);
    assert_eq!(analytics.projects[0].values["codex"], 150);
    assert_eq!(analytics.projects[0].values["claude"], 200);
    assert_eq!(analytics.projects[1].project, "/proj/b");

    let filtered = aggregate::application_analytics(
        &records,
        &Filter {
            projects: vec!["/proj/b".into()],
            ..Filter::default()
        },
        "month",
    );
    assert_eq!(filtered.summary.total_tokens, 100);
    assert_eq!(filtered.by_application.len(), 1);
    assert_eq!(filtered.by_application[0].application, "Claude Code");
    assert_eq!(filtered.trend[0].bucket, "2026-08");
    assert_eq!(filtered.projects.len(), 1);
}

#[test]
fn application_efficiency_returns_none_when_ratio_denominators_are_zero() {
    let records = vec![rec(
        "2026-08-01T10:00:00Z",
        Source::Factory,
        "",
        "anthropic",
        "",
        "droid-session",
        0,
    )];
    let analytics = aggregate::application_analytics(&records, &Filter::default(), "day");

    assert_eq!(analytics.summary.cache_hit_rate, None);
    assert_eq!(analytics.summary.reasoning_share, None);
    assert_eq!(analytics.summary.average_session_tokens, Some(0.0));
    assert_eq!(analytics.projects[0].project, "（未标注）");
}

#[test]
fn application_efficiency_does_not_treat_missing_cache_as_zero_percent() {
    let mut record = rec(
        "2026-08-01T10:00:00Z",
        Source::Codex,
        "gpt-5.1-codex",
        "official",
        "/proj/a",
        "no-cache",
        100,
    );
    record.input_tokens = 100;
    record.cache_read_tokens = 0;
    record.cache_creation_tokens = 0;
    let analytics = aggregate::application_analytics(&[record], &Filter::default(), "day");
    assert_eq!(analytics.summary.cache_hit_rate, None);
    assert_eq!(analytics.by_application[0].metrics.cache_hit_rate, None);
}

#[test]
fn factory_adapter_root_settings_have_empty_project() {
    let records = factory::parse_factory_settings(
        &fixture("factory.settings.json"),
        "/Users/zhangyanhua/.factory/sessions/9ab2ca7b-bd30-495b-9434-07892ee0e5e6.settings.json",
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].project, "");
    assert_eq!(
        records[0].session_id,
        "9ab2ca7b-bd30-495b-9434-07892ee0e5e6"
    );
}

#[test]
fn cursor_code_volume_stays_outside_usage_records() {
    let commits = parse_cursor_commits(&[CursorCommitRow {
        commit_hash: "abc".into(),
        branch: "main".into(),
        scored_at_ms: 1_771_411_050_440,
        lines_added: 156,
        composer_lines_added: 32,
        human_lines_added: 0,
        ai_percentage: Some(100.0),
        ..Default::default()
    }]);
    let summary = summarize_code_volume(&commits);
    assert_eq!(summary.commit_count, 1);
    assert_eq!(summary.lines_added, 156);
    assert_eq!(summary.composer_lines_added, 32);
    assert!((summary.ai_percentage.unwrap() - 20.51282051282051).abs() < 1e-9);
    assert_ne!(summary.ai_percentage.unwrap(), 100.0);

    let empty = summarize_code_volume(&[]);
    assert_eq!(empty.commit_count, 0);
    assert_eq!(empty.lines_added, 0);
    assert_eq!(empty.ai_percentage, None);

    let fallback = summarize_code_volume(&parse_cursor_commits(&[
        CursorCommitRow {
            commit_hash: "a".into(),
            branch: "main".into(),
            scored_at_ms: 1,
            ai_percentage: Some(40.0),
            ..Default::default()
        },
        CursorCommitRow {
            commit_hash: "b".into(),
            branch: "main".into(),
            scored_at_ms: 2,
            ai_percentage: Some(60.0),
            ..Default::default()
        },
    ]));
    assert_eq!(fallback.lines_added, 0);
    assert!((fallback.ai_percentage.unwrap() - 50.0).abs() < 1e-9);

    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &seed_records()).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 450);
    assert_eq!(stored.len(), 3);
}

#[test]
fn with_cost_roi_derives_cost_per_thousand_ai_lines() {
    let summary = summarize_code_volume(&parse_cursor_commits(&[CursorCommitRow {
        commit_hash: "abc".into(),
        branch: "main".into(),
        scored_at_ms: 1,
        lines_added: 4000,
        composer_lines_added: 2000,
        human_lines_added: 2000,
        ..Default::default()
    }]));

    let priced = with_cost_roi(summary.clone(), Some(30.0), false);
    assert_eq!(priced.total_cost, Some(30.0));
    assert!(!priced.cost_unpriced);
    // 2000 行 AI 代码花了 $30，即每千行 $15。
    assert!((priced.cost_per_thousand_ai_lines.unwrap() - 15.0).abs() < 1e-9);

    // 未配置任何单价时 cost 为 None，ROI 也应为 None，而不是被当成 0 处理。
    let unpriced = with_cost_roi(summary.clone(), None, true);
    assert_eq!(unpriced.cost_per_thousand_ai_lines, None);
    assert!(unpriced.cost_unpriced);

    // 没有任何 AI 生成行时分母为 0，即使有费用也不应该算出 ROI。
    let no_lines = summarize_code_volume(&[]);
    let no_lines_priced = with_cost_roi(no_lines, Some(10.0), false);
    assert_eq!(no_lines_priced.cost_per_thousand_ai_lines, None);
}

#[test]
fn load_code_volume_reads_sqlite_without_writing_usage() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    assert_eq!(ingest::load_code_volume(home).unwrap().commit_count, 0);
    assert_eq!(ingest::load_code_volume(home).unwrap().ai_percentage, None);

    let db_path = home.join(".cursor/ai-tracking/ai-code-tracking.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let src = rusqlite::Connection::open(&db_path).unwrap();
    src.execute_batch(
        r#"
        CREATE TABLE scored_commits (
            commitHash TEXT,
            branchName TEXT,
            scoredAt INTEGER,
            commitMessage TEXT,
            linesAdded INTEGER,
            linesDeleted INTEGER,
            composerLinesAdded INTEGER,
            composerLinesDeleted INTEGER,
            humanLinesAdded INTEGER,
            humanLinesDeleted INTEGER,
            tabLinesAdded INTEGER,
            tabLinesDeleted INTEGER,
            v2AiPercentage TEXT
        );
        INSERT INTO scored_commits VALUES
            ('abc', 'main', 1771411050440, 'feat', 156, 20, 32, 4, 0, 0, 10, 1, '100'),
            ('skip', 'main', 1771411050441, '', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL);
        "#,
    )
    .unwrap();
    drop(src);

    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all(&conn, home).unwrap();
    assert_eq!(report.records_written, 0);
    assert!(store::load_all(&conn).unwrap().is_empty());

    let volume = ingest::load_code_volume(home).unwrap();
    assert_eq!(volume.commit_count, 1);
    assert_eq!(volume.lines_added, 156);
    assert_eq!(volume.lines_deleted, 20);
    assert_eq!(volume.net_lines, 136);
    assert_eq!(volume.composer_lines_added, 32);
    assert_eq!(volume.tab_lines_added, 10);
    assert_eq!(volume.commits.len(), 1);
    assert_eq!(volume.by_branch.len(), 1);
    assert_eq!(volume.by_branch[0].name, "main");
    assert!((volume.ai_percentage.unwrap() - 20.51282051282051).abs() < 1e-9);
}
