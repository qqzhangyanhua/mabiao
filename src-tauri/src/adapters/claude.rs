use std::collections::HashMap;

use crate::adapters::project::decode_dashed_dir;
use crate::adapters::{finish, has_billable_tokens, i64_field, parse_jsonl_values, text_field};
use crate::domain::{Source, UsageRecord};

struct ClaudeTurn {
    record: UsageRecord,
    stop_reason: Option<String>,
}

pub fn parse_claude_jsonl(content: &str, source_file: &str) -> Vec<UsageRecord> {
    let mut project = String::new();
    let mut session_id = String::new();
    for value in parse_jsonl_values(content) {
        if project.is_empty() {
            project = text_field(&value, &["cwd"]);
        }
        if session_id.is_empty() {
            session_id = text_field(&value, &["sessionId", "session_id"]);
        }
        if !project.is_empty() && !session_id.is_empty() {
            break;
        }
    }
    if project.is_empty() {
        project = project_from_path(source_file);
    }
    if session_id.is_empty() {
        session_id = crate::adapters::project::session_id_from_source_file(source_file);
    }

    let mut by_id: HashMap<String, ClaudeTurn> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut anonymous = Vec::new();

    for value in parse_jsonl_values(content) {
        if value.get("type").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let message = value.get("message").cloned().unwrap_or_default();
        let usage = message.get("usage").cloned().unwrap_or_default();
        if usage.is_null() {
            continue;
        }
        let agent_id = text_field(&value, &["agentId", "agent_id"]);
        let record_session_id = if agent_id.is_empty() {
            session_id.clone()
        } else {
            agent_id
        };
        let record = finish(UsageRecord {
            occurred_at: text_field(&value, &["timestamp"]),
            source: Source::Claude,
            model: text_field(&message, &["model"]),
            provider: String::new(),
            project: project.clone(),
            session_id: record_session_id,
            source_file: source_file.to_string(),
            input_tokens: i64_field(&usage, &["input_tokens"]),
            output_tokens: i64_field(&usage, &["output_tokens"]),
            cache_read_tokens: i64_field(&usage, &["cache_read_input_tokens"]),
            cache_creation_tokens: i64_field(&usage, &["cache_creation_input_tokens"]),
            reasoning_tokens: 0,
            total_tokens: 0,
            native_cost: native_cost_from_event(&value),
        });
        if !has_billable_tokens(&record) {
            continue;
        }
        let stop_reason = message
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let message_id = text_field(&message, &["id"]);
        if message_id.is_empty() {
            anonymous.push(record);
            continue;
        }
        let turn = ClaudeTurn {
            record,
            stop_reason,
        };
        let should_replace = match by_id.get(&message_id) {
            None => true,
            Some(existing) => should_replace_claude(existing, &turn),
        };
        if should_replace {
            if !by_id.contains_key(&message_id) {
                order.push(message_id.clone());
            }
            by_id.insert(message_id, turn);
        }
    }

    let mut records: Vec<UsageRecord> = order
        .into_iter()
        .filter_map(|id| by_id.remove(&id).map(|turn| turn.record))
        .collect();
    records.extend(anonymous);
    records
}

/// 与 cc-switch 一致：同一 `message.id` 优先保留有 stop_reason 的，否则取 output 更大的。
fn should_replace_claude(existing: &ClaudeTurn, next: &ClaudeTurn) -> bool {
    match (existing.stop_reason.is_some(), next.stop_reason.is_some()) {
        (false, true) => true,
        (true, false) => false,
        _ => next.record.output_tokens > existing.record.output_tokens,
    }
}

fn native_cost_from_event(value: &serde_json::Value) -> Option<f64> {
    for key in ["costUSD", "costUsd", "cost_usd"] {
        if let Some(amount) = value.get(key).and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_i64().map(|n| n as f64))
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        }) {
            if amount > 0.0 {
                return Some(amount);
            }
        }
    }
    None
}

fn project_from_path(source_file: &str) -> String {
    std::path::Path::new(source_file)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(decode_dashed_dir)
        .unwrap_or_default()
}
