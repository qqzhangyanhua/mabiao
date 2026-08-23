use serde_json::Value;

use crate::adapters::{
    finish, has_billable_tokens, i64_field, parse_jsonl_value_lines, text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};

/// 解析 GitHub Copilot CLI 落盘的 `~/.copilot/session-state/<session-id>/events.jsonl`。
///
/// token 只在 `session.shutdown` 事件的 `data.modelMetrics` 里按模型给出「本会话累计值」；
/// 若同一文件里出现多次 `session.shutdown`（会话被多次续接退出），只取时间最晚的一次，
/// 避免把同一份累计用量重复计入——与 Codex 适配器「取最后一次快照，不逐条累加」的策略一致。
/// 详见 docs/probe/copilot.md。
pub fn parse_copilot_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    let mut session_id = String::new();
    let mut project = String::new();
    let mut last_shutdown: Option<Value> = None;
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
            Some("session.shutdown") => last_shutdown = Some(value),
            _ => {}
        }
    }
    if session_id.is_empty() {
        // events.jsonl 本身总是同名，会话 ID 只能从父目录名兜底。
        session_id = std::path::Path::new(source_file)
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
    }

    let Some(shutdown) = last_shutdown else {
        return Vec::new();
    };
    let timestamp = text_field(&shutdown, &["timestamp"]);
    let data = shutdown.get("data").unwrap_or(&Value::Null);
    let Some(metrics) = data.get("modelMetrics").and_then(|value| value.as_object()) else {
        return Vec::new();
    };

    let mut records = Vec::new();
    for (model, metric) in metrics {
        let usage = match metric.get("usage") {
            Some(usage) if !usage.is_null() => usage,
            _ => continue,
        };
        let record = finish(UsageRecord {
            occurred_at: timestamp.clone(),
            source: Source::Copilot,
            model: model.clone(),
            provider: String::new(),
            project: project.clone(),
            session_id: session_id.clone(),
            source_file: source_file.to_string(),
            input_tokens: i64_field(usage, &["inputTokens"]),
            output_tokens: i64_field(usage, &["outputTokens"]),
            cache_read_tokens: i64_field(usage, &["cacheReadTokens"]),
            cache_creation_tokens: i64_field(usage, &["cacheWriteTokens"]),
            reasoning_tokens: 0,
            total_tokens: 0,
            native_cost: None,
        });
        if !has_billable_tokens(&record) {
            continue;
        }
        records.push(record);
    }
    records.sort_by(|a, b| a.model.cmp(&b.model));
    records
}
