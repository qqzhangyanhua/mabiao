use crate::test_support::*;

#[test]
fn overview_from_codex_fixture_uses_last_token_usage_totals() {
    let records = codex::parse_codex_jsonl(
        &fixture_lines(&fixture("codex.jsonl")),
        "/Users/zhangyanhua/.codex/sessions/rollout.jsonl",
    );
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 19113);
    assert_eq!(dto.input_tokens, 18413);
    assert_eq!(dto.output_tokens, 700);
    assert_eq!(dto.cache_read_tokens, 2048);
    assert_eq!(dto.cache_creation_tokens, 0);
    assert_eq!(dto.reasoning_tokens, 64);
    assert_eq!(dto.session_count, 1);
    assert_ne!(dto.total_tokens, 9496 + 19113);
}

#[test]
fn overview_from_claude_fixture_sums_per_record_token_dimensions() {
    let records = claude::parse_claude_jsonl(
        &fixture_lines(&fixture("claude.jsonl")),
        "/Users/zhangyanhua/.claude/projects/-Users-zhangyanhua-AI-TradingAgents-CN/04868551-34c3-4588-b984-6ae9a5d95f8a.jsonl",
    );
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 112886);
    assert_eq!(dto.input_tokens, 120);
    assert_eq!(dto.output_tokens, 102);
    assert_eq!(dto.cache_read_tokens, 56332);
    assert_eq!(dto.cache_creation_tokens, 56332);
    assert_eq!(dto.reasoning_tokens, 0);
    assert_eq!(dto.session_count, 1);
    assert!((dto.cost.unwrap() - 0.0204).abs() < 1e-9);
    assert!(!dto.unpriced);
}

#[test]
fn overview_from_pi_fixture_uses_native_cost() {
    let records = pi::parse_pi_jsonl(
        &fixture_lines(&fixture("pi.jsonl")),
        "/Users/zhangyanhua/.pi/agent/sessions/--Users-zhangyanhua-workCode-ruoyi-ui-vue3--/s.jsonl",
    );
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 25539);
    assert_eq!(dto.input_tokens, 13175);
    assert_eq!(dto.output_tokens, 76);
    assert_eq!(dto.cache_read_tokens, 12288);
    assert_eq!(dto.cache_creation_tokens, 0);
    assert_eq!(dto.reasoning_tokens, 25);
    assert_eq!(dto.session_count, 1);
    assert!((dto.cost.unwrap() - 0.074299).abs() < 1e-9);
    assert!(!dto.unpriced);
}

#[test]
fn overview_from_opencode_fixture_uses_native_cost() {
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
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 21140);
    assert_eq!(dto.input_tokens, 20882);
    assert_eq!(dto.output_tokens, 138);
    assert_eq!(dto.cache_read_tokens, 100);
    assert_eq!(dto.cache_creation_tokens, 20);
    assert_eq!(dto.reasoning_tokens, 0);
    assert_eq!(dto.session_count, 1);
    assert_eq!(dto.cost, Some(0.42));
    assert!(!dto.unpriced);
}

#[test]
fn overview_from_kimi_fixture_uses_last_status_update_totals() {
    let records = kimi::parse_kimi_wire(
        &fixture("kimi-wire.jsonl"),
        "/Users/zhangyanhua/.kimi/sessions/hash/bd1ab6fc-768d-4cff-b4c4-221a583c3af8/wire.jsonl",
        "/Users/zhangyanhua/workCode/app-storage",
    );
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 14887);
    assert_eq!(dto.input_tokens, 3330);
    assert_eq!(dto.output_tokens, 539);
    assert_eq!(dto.cache_read_tokens, 11008);
    assert_eq!(dto.cache_creation_tokens, 10);
    assert_eq!(dto.reasoning_tokens, 0);
    assert_eq!(dto.session_count, 1);
    assert_ne!(dto.total_tokens, 2547 + 142 + 3000 + 200 + 330 + 339);
}

#[test]
fn overview_from_dsh_fixture_uses_final_assistant_totals() {
    let records = dsh::parse_dsh_jsonl(
        &fixture("dsh.jsonl"),
        "/Users/zhangyanhua/.dsh/sessions/--Users-zhangyanhua-AI-pi--/session.jsonl.zstd",
    );
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 30829);
    assert_eq!(dto.input_tokens, 15275);
    assert_eq!(dto.output_tokens, 872);
    assert_eq!(dto.cache_read_tokens, 14080);
    assert_eq!(dto.cache_creation_tokens, 0);
    assert_eq!(dto.reasoning_tokens, 602);
    assert_eq!(dto.session_count, 1);
    assert_ne!(dto.total_tokens, 30829 + 4);
}

#[test]
fn overview_from_gemini_fixture_sums_per_record_token_dimensions() {
    let records = gemini::parse_gemini_session(
        &fixture("gemini-session.json"),
        "/Users/zhangyanhua/.gemini/tmp/ruoyi-ui-vue3/chats/session-2026-03-07.json",
    );
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 14301);
    assert_eq!(dto.input_tokens, 13354);
    assert_eq!(dto.output_tokens, 662);
    assert_eq!(dto.cache_read_tokens, 0);
    assert_eq!(dto.cache_creation_tokens, 0);
    assert_eq!(dto.reasoning_tokens, 285);
    assert_eq!(dto.session_count, 1);
}

#[test]
fn overview_from_grok_fixture_uses_last_total_per_prompt() {
    let records = grok::parse_grok_updates(
        &fixture("grok-updates.jsonl"),
        "/Users/zhangyanhua/.grok/sessions/%2FUsers%2Fzhangyanhua%2FAI%2FTradingAgents-CN/019fd235/updates.jsonl",
        "grok-4.5",
    );
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 98208);
    assert_eq!(dto.input_tokens, 0);
    assert_eq!(dto.output_tokens, 0);
    assert_eq!(dto.cache_read_tokens, 0);
    assert_eq!(dto.cache_creation_tokens, 0);
    assert_eq!(dto.reasoning_tokens, 0);
    assert_eq!(dto.session_count, 1);
    assert_ne!(dto.total_tokens, 15681 + 26857 + 71351);
}

#[test]
fn overview_from_grok_turn_completed_uses_usage_not_context_total() {
    let records = grok::parse_grok_updates(
        &fixture("grok-turn-completed.jsonl"),
        "/Users/zhangyanhua/.grok/sessions/%2FUsers%2Fzhangyanhua%2FAI%2FTradingAgents-CN/019fd235/updates.jsonl",
        "grok-4.5",
    );
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 452282);
    assert_eq!(dto.input_tokens, 447530);
    assert_eq!(dto.output_tokens, 4752);
    assert_eq!(dto.cache_read_tokens, 410117);
    assert_eq!(dto.reasoning_tokens, 3570);
    assert_eq!(dto.session_count, 1);
    assert!((dto.cost.unwrap() - 0.408144).abs() < 1e-9);
    assert!(!dto.unpriced);
}

#[test]
fn overview_from_qwen_fixture_contributes_no_tokens() {
    let records = qwen::parse_qwen_session(&fixture("qwen-logs.json"), "logs.json");
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 0);
    assert_eq!(dto.input_tokens, 0);
    assert_eq!(dto.output_tokens, 0);
    assert_eq!(dto.cache_read_tokens, 0);
    assert_eq!(dto.cache_creation_tokens, 0);
    assert_eq!(dto.reasoning_tokens, 0);
    assert_eq!(dto.session_count, 0);
}

#[test]
fn overview_from_copilot_fixture_uses_last_shutdown_snapshot() {
    let records = copilot::parse_copilot_jsonl(
        &fixture_lines(&fixture("copilot-events.jsonl")),
        "/Users/dev/.copilot/session-state/c0ffee11-2222-4333-8444-555566667777/events.jsonl",
    );
    assert_eq!(records.len(), 2);
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());

    assert_eq!(dto.input_tokens, 21583 + 244120);
    assert_eq!(dto.output_tokens, 1064 + 2383);
    assert_eq!(dto.cache_read_tokens, 21187 + 202112);
    assert_eq!(dto.cache_creation_tokens, 0);
    assert_eq!(dto.reasoning_tokens, 0);
    assert_eq!(dto.session_count, 1);
    assert_eq!(
        dto.total_tokens,
        records
            .iter()
            .map(|record| record.total_tokens)
            .sum::<i64>()
    );
    assert_eq!(
        dto.total_tokens,
        (21583 + 1064 + 21187) + records[1].total_tokens
    );
}

#[test]
fn overview_from_factory_fixture_uses_session_token_usage() {
    let records = factory::parse_factory_settings(
        &fixture("factory.settings.json"),
        "/Users/zhangyanhua/.factory/sessions/-Users-zhangyanhua-AI-cli/9ab2ca7b-bd30-495b-9434-07892ee0e5e6.settings.json",
    );
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &records).unwrap();
    let stored = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&stored, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 20234);
    assert_eq!(dto.input_tokens, 3);
    assert_eq!(dto.output_tokens, 1022);
    assert_eq!(dto.cache_read_tokens, 11084);
    assert_eq!(dto.cache_creation_tokens, 8125);
    assert_eq!(dto.reasoning_tokens, 0);
    assert_eq!(dto.session_count, 1);
}

#[test]
fn overview_sums_seeded_sqlite_records() {
    let conn = store::open_memory().unwrap();
    store::insert_records(&conn, &seed_records()).unwrap();
    let records = store::load_all(&conn).unwrap();
    let dto = aggregate::overview(&records, &Filter::default(), &PriceTable::default());
    assert_eq!(dto.total_tokens, 450);
    assert_eq!(dto.input_tokens, 450);
    assert_eq!(dto.session_count, 3);
}
