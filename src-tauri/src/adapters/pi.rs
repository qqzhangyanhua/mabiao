use std::path::{Path, PathBuf};

use crate::adapters::project::decode_dashed_dir;
use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, parse_streaming_jsonl,
    text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "PI_AGENT_DIR", ".pi/agent/sessions", "")
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_pi_jsonl)
}

pub fn parse_pi_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    let mut session_id = String::new();
    let mut project = String::new();
    let mut provider = String::new();
    let mut records = Vec::new();

    for value in parse_jsonl_value_lines(lines()) {
        match value.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "session" => {
                session_id = text_field(&value, &["id"]);
                project = text_field(&value, &["cwd"]);
            }
            "model_change" => {
                let next = text_field(&value, &["provider"]);
                if !next.is_empty() {
                    provider = next;
                }
            }
            "message" => {
                let message = value.get("message").cloned().unwrap_or_default();
                if message.get("role").and_then(|v| v.as_str()) != Some("assistant") {
                    continue;
                }
                let usage = message.get("usage").cloned().unwrap_or_default();
                if usage.is_null() {
                    continue;
                }
                let native_cost = usage
                    .get("cost")
                    .and_then(|c| c.get("total"))
                    .and_then(|v| v.as_f64());
                let message_provider = text_field(&message, &["provider"]);
                let record = finish(UsageRecord {
                    occurred_at: text_field(&value, &["timestamp"]),
                    source: Source::Pi,
                    model: text_field(&message, &["model", "modelId"]),
                    provider: if message_provider.is_empty() {
                        provider.clone()
                    } else {
                        message_provider
                    },
                    project: if project.is_empty() {
                        project_from_path(source_file)
                    } else {
                        project.clone()
                    },
                    session_id: session_id.clone(),
                    source_file: source_file.to_string(),
                    input_tokens: i64_field(&usage, &["input"]),
                    output_tokens: i64_field(&usage, &["output"]),
                    cache_read_tokens: i64_field(&usage, &["cacheRead"]),
                    cache_creation_tokens: i64_field(&usage, &["cacheWrite"]),
                    reasoning_tokens: i64_field(&usage, &["reasoning"]),
                    total_tokens: i64_field(&usage, &["totalTokens"]),
                    native_cost,
                });
                // 与其它 adapter 保持一致：usage 对象存在但四个分项全 0 的消息不计入会话/费用统计。
                if !has_billable_tokens(&record) {
                    continue;
                }
                records.push(record);
            }
            _ => {}
        }
    }

    records
}

fn project_from_path(source_file: &str) -> String {
    std::path::Path::new(source_file)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(decode_dashed_dir)
        .unwrap_or_default()
}
