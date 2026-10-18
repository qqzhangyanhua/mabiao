use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, parse_streaming_jsonl,
    text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "COPILOT_HOME", ".copilot", "session-state")
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_copilot_jsonl)
}

#[derive(Clone, Copy, Default)]
struct CopilotUsage {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
}

impl CopilotUsage {
    fn from_value(usage: &Value) -> Self {
        Self {
            input: i64_field(usage, &["inputTokens"]).max(0),
            output: i64_field(usage, &["outputTokens"]).max(0),
            cache_read: i64_field(usage, &["cacheReadTokens"]).max(0),
            cache_write: i64_field(usage, &["cacheWriteTokens"]).max(0),
        }
    }

    fn saturating_sub(self, prev: Self) -> Self {
        Self {
            input: self.input.saturating_sub(prev.input),
            output: self.output.saturating_sub(prev.output),
            cache_read: self.cache_read.saturating_sub(prev.cache_read),
            cache_write: self.cache_write.saturating_sub(prev.cache_write),
        }
    }

    /// session-state 的 `inputTokens` 已含 cache read/write（与 OTEL `gen_ai.usage.input_tokens` 相同）。
    /// 拆成互斥四档，避免 `with_total` 把 cache 再加一遍。
    fn spent(self) -> Self {
        let cached = self.cache_read.min(self.input);
        let write = self.cache_write.min(self.input.saturating_sub(cached));
        Self {
            input: self.input - cached - write,
            output: self.output,
            cache_read: cached,
            cache_write: write,
        }
    }

    fn is_zero(self) -> bool {
        self.input == 0 && self.output == 0 && self.cache_read == 0 && self.cache_write == 0
    }
}

/// 解析 GitHub Copilot CLI 落盘的 `~/.copilot/session-state/<session-id>/events.jsonl`。
///
/// token 只在 `session.shutdown` 的 `data.modelMetrics` 里按模型给出「本会话累计值」。
/// 多次 shutdown（续接退出）按 (session, model) 做相邻快照差分，日/窗归属跟退出时刻走，
/// 全时段合计仍等于最后一次快照。`inputTokens` 含 cache，入账前拆成裸 input。
/// 详见 docs/probe/copilot.md。
pub fn parse_copilot_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    let mut session_id = String::new();
    let mut project = String::new();
    let mut shutdowns: Vec<Value> = Vec::new();
    for value in parse_jsonl_value_lines(lines()) {
        match value.get("type").and_then(|v| v.as_str()) {
            Some("session.start") => {
                let data = value.get("data").unwrap_or(&Value::Null);
                let candidate_session = text_field(data, &["sessionId"]);
                if !candidate_session.is_empty() {
                    session_id = candidate_session;
                }
                let candidate_cwd = data
                    .get("context")
                    .and_then(|context| context.get("cwd"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                if !candidate_cwd.is_empty() {
                    project = candidate_cwd.to_string();
                }
            }
            Some("session.shutdown") => shutdowns.push(value),
            _ => {}
        }
    }
    if session_id.is_empty() {
        session_id = std::path::Path::new(source_file)
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
    }

    let mut by_model: BTreeMap<String, Vec<(String, CopilotUsage)>> = BTreeMap::new();
    for shutdown in shutdowns {
        let timestamp = text_field(&shutdown, &["timestamp"]);
        let data = shutdown.get("data").unwrap_or(&Value::Null);
        let Some(metrics) = data.get("modelMetrics").and_then(|value| value.as_object()) else {
            continue;
        };
        for (model, metric) in metrics {
            let usage = match metric.get("usage") {
                Some(usage) if !usage.is_null() => usage,
                _ => continue,
            };
            by_model
                .entry(model.clone())
                .or_default()
                .push((timestamp.clone(), CopilotUsage::from_value(usage)));
        }
    }

    let mut records = Vec::new();
    for (model, mut snapshots) in by_model {
        snapshots.sort_by(|a, b| a.0.cmp(&b.0));
        let mut previous = CopilotUsage::default();
        for (timestamp, cumulative) in snapshots {
            let interval = cumulative.saturating_sub(previous).spent();
            previous = cumulative;
            if interval.is_zero() {
                continue;
            }
            let record = finish(UsageRecord {
                occurred_at: timestamp,
                source: Source::Copilot,
                model: model.clone(),
                provider: String::new(),
                project: project.clone(),
                session_id: session_id.clone(),
                source_file: source_file.to_string(),
                input_tokens: interval.input,
                output_tokens: interval.output,
                cache_read_tokens: interval.cache_read,
                cache_creation_tokens: interval.cache_write,
                cache_creation_1h_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 0,
                native_cost: None,
            });
            if !has_billable_tokens(&record) {
                continue;
            }
            records.push(record);
        }
    }
    records.sort_by(|a, b| {
        a.occurred_at
            .cmp(&b.occurred_at)
            .then_with(|| a.model.cmp(&b.model))
    });
    records
}
