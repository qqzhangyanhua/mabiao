use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, parse_streaming_jsonl,
    text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "CODEX_HOME", ".codex", "sessions")
}

pub fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_codex_jsonl)
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct CodexUsage {
    input: i64,
    cached: i64,
    output: i64,
    reasoning: i64,
    total: i64,
}

impl CodexUsage {
    fn fingerprint(self) -> (i64, i64, i64, i64, i64) {
        (
            self.input,
            self.cached,
            self.output,
            self.reasoning,
            self.total,
        )
    }

    fn is_zero(self) -> bool {
        self.input == 0 && self.cached == 0 && self.output == 0 && self.reasoning == 0
    }

    fn clamped(self) -> Self {
        Self {
            cached: self.cached.min(self.input),
            ..self
        }
    }

    fn saturating_sub(self, prev: Self) -> Self {
        Self {
            input: self.input.saturating_sub(prev.input),
            cached: self.cached.saturating_sub(prev.cached),
            output: self.output.saturating_sub(prev.output),
            reasoning: self.reasoning.saturating_sub(prev.reasoning),
            total: self.total.saturating_sub(prev.total),
        }
    }

    fn high_water(self, other: Self) -> Self {
        Self {
            input: self.input.max(other.input),
            cached: self.cached.max(other.cached),
            output: self.output.max(other.output),
            reasoning: self.reasoning.max(other.reasoning),
            total: self.total.max(other.total),
        }
    }
}

pub fn parse_codex_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    let mut session_id = String::new();
    let mut project = String::new();
    let mut provider = String::new();
    let mut model = String::new();
    let mut last_usage: Option<(i64, i64, i64, i64, i64)> = None;
    let mut total_high_water: Option<CodexUsage> = None;
    let mut records = Vec::new();

    for value in parse_jsonl_value_lines(lines()) {
        let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let payload = value.get("payload").cloned().unwrap_or(Value::Null);
        let timestamp = text_field(&value, &["timestamp"]);

        match kind {
            "session_meta" => {
                session_id = text_field(&payload, &["id", "session_id"]);
                project = text_field(&payload, &["cwd"]);
                provider = text_field(&payload, &["model_provider"]);
            }
            "turn_context" => {
                let next_model = text_field(&payload, &["model"]);
                if !next_model.is_empty() {
                    model = next_model;
                }
                let cwd = text_field(&payload, &["cwd"]);
                if project.is_empty() && !cwd.is_empty() {
                    project = cwd;
                }
            }
            "event_msg" => {
                if payload.get("type").and_then(|v| v.as_str()) != Some("token_count") {
                    continue;
                }
                let info = payload.get("info").cloned().unwrap_or(Value::Null);
                if info.is_null() {
                    continue;
                }
                let info_model = text_field(&info, &["model", "model_name"]);
                if !info_model.is_empty() {
                    model = info_model;
                }
                let last = parse_codex_usage(info.get("last_token_usage"));
                let total = parse_codex_usage(info.get("total_token_usage"));
                if last.is_none() && total.is_none() {
                    continue;
                }
                let usage = if let Some(last) = last {
                    last
                } else if let Some(total) = total {
                    match total_high_water {
                        Some(prev) => total.saturating_sub(prev),
                        None => total,
                    }
                } else {
                    continue;
                };
                if let Some(total) = total {
                    total_high_water = Some(match total_high_water {
                        Some(prev) => prev.high_water(total),
                        None => total,
                    });
                }
                let usage = usage.clamped();
                if usage.is_zero() {
                    continue;
                }
                let fingerprint = usage.fingerprint();
                if last_usage == Some(fingerprint) {
                    continue;
                }
                last_usage = Some(fingerprint);
                let record = finish(UsageRecord {
                    occurred_at: timestamp,
                    source: Source::Codex,
                    model: model.clone(),
                    provider: provider.clone(),
                    project: project.clone(),
                    session_id: session_id.clone(),
                    source_file: source_file.to_string(),
                    input_tokens: usage.input,
                    output_tokens: usage.output,
                    cache_read_tokens: usage.cached,
                    cache_creation_tokens: 0,
                    reasoning_tokens: usage.reasoning,
                    total_tokens: usage.total,
                    native_cost: None,
                });
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

fn parse_codex_usage(value: Option<&Value>) -> Option<CodexUsage> {
    let value = value?;
    let object = value.as_object()?;
    if ![
        "input_tokens",
        "cached_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
        "reasoning_output_tokens",
        "total_tokens",
    ]
    .iter()
    .any(|key| object.contains_key(*key))
    {
        return None;
    }
    Some(CodexUsage {
        input: i64_field(value, &["input_tokens"]),
        cached: i64_field(value, &["cached_input_tokens", "cache_read_input_tokens"]),
        output: i64_field(value, &["output_tokens"]),
        reasoning: i64_field(value, &["reasoning_output_tokens"]),
        total: i64_field(value, &["total_tokens"]),
    })
}
