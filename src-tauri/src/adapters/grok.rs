use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::project::decode_url_dir;
use crate::adapters::{finish, i64_field, parse_jsonl_values, text_field};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "GROK_HOME", ".grok", "sessions")
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        for path in ingest::walk_files(root, "jsonl")? {
            if path.file_name().and_then(|name| name.to_str()) == Some("updates.jsonl") {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    ingest::content_fingerprint(&summary_path(path))
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let model = current_model(path)?;
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let content = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
    ingest::validate_jsonl(content)?;
    Ok(parse_grok_updates(content, &path.to_string_lossy(), &model))
}

fn summary_path(path: &Path) -> PathBuf {
    path.parent()
        .map(|parent| parent.join("summary.json"))
        .unwrap_or_default()
}

/// 摘要缺失时回退空模型名；解析失败必须返回 Err，由摄取记来源级失败并跳过当前文件。
fn current_model(path: &Path) -> Result<String, String> {
    let summary_path = summary_path(path);
    if !summary_path.exists() {
        return Ok(String::new());
    }
    let text =
        fs::read_to_string(&summary_path).map_err(|error| format!("Grok 模型摘要无效：{error}"))?;
    let summary = serde_json::from_str::<Value>(&text)
        .map_err(|error| format!("Grok 模型摘要无效：{error}"))?;
    Ok(summary
        .get("current_model_id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string())
}

/// `costUsdTicks`：1 tick = 1e-10 USD（与 Grok CLI / cc-switch 一致）。
const USD_TICKS_PER_DOLLAR: f64 = 10_000_000_000.0;

pub fn parse_grok_updates(
    content: &str,
    source_file: &str,
    fallback_model: &str,
) -> Vec<UsageRecord> {
    let session_id = path_session_id(source_file);
    let project = path_project(source_file);
    let turns = parse_turn_completed(content, source_file, &session_id, &project, fallback_model);
    if !turns.is_empty() {
        return turns;
    }
    parse_context_totals(content, source_file, &session_id, &project, fallback_model)
}

/// 优先口径：`turn_completed` 的 `usage`（与 cc-switch 相同）。
/// 这是该 user prompt 一轮的独立用量，不是上下文占用，也不按相邻事件差分。
fn parse_turn_completed(
    content: &str,
    source_file: &str,
    session_id: &str,
    project: &str,
    fallback_model: &str,
) -> Vec<UsageRecord> {
    let mut last_by_key: HashMap<(String, String), UsageRecord> = HashMap::new();
    let mut order: Vec<(String, String)> = Vec::new();

    for value in parse_jsonl_values(content) {
        let params = value.get("params").cloned().unwrap_or_default();
        let update = params.get("update").cloned().unwrap_or_default();
        if !is_turn_completed(&value, &update) {
            continue;
        }
        let usage = match update.get("usage") {
            Some(usage) if usage.is_object() => usage,
            _ => continue,
        };

        let prompt_id = text_field(&update, &["prompt_id"]);
        let occurred = event_occurred_at(&value, &params);
        let mut per_model = model_usages(usage);
        if per_model.is_empty() {
            per_model.push((fallback_model.to_string(), usage.clone()));
        }
        per_model.sort_by(|a, b| a.0.cmp(&b.0));

        for (mut model, counters) in per_model {
            if model.is_empty() || model == "unknown" {
                model = fallback_model.to_string();
            }
            let input = i64_field(&counters, &["inputTokens"]);
            let output = i64_field(&counters, &["outputTokens"]);
            let cache_read = i64_field(&counters, &["cachedReadTokens"]);
            let cache_creation = i64_field(&counters, &["cacheCreationTokens"]);
            let reasoning = i64_field(&counters, &["reasoningTokens"]);
            let reported_total = i64_field(&counters, &["totalTokens"]);
            if input == 0 && output == 0 && cache_read == 0 && reported_total == 0 {
                continue;
            }
            let key = (prompt_id.clone(), model.clone());
            if !last_by_key.contains_key(&key) {
                order.push(key.clone());
            }
            last_by_key.insert(
                key,
                finish(UsageRecord {
                    occurred_at: occurred.clone(),
                    source: Source::Grok,
                    model,
                    provider: String::new(),
                    project: project.to_string(),
                    session_id: session_id.to_string(),
                    source_file: source_file.to_string(),
                    input_tokens: input,
                    output_tokens: output,
                    cache_read_tokens: cache_read,
                    cache_creation_tokens: cache_creation,
                    reasoning_tokens: reasoning,
                    total_tokens: if reported_total > 0 {
                        reported_total
                    } else {
                        input + output
                    },
                    native_cost: ticks_to_usd(&counters),
                }),
            );
        }
    }

    order
        .into_iter()
        .filter_map(|key| last_by_key.remove(&key))
        .collect()
}

/// 旧日志兜底：只有 `_meta.totalTokens`（上下文占用），无分项。
fn parse_context_totals(
    content: &str,
    source_file: &str,
    session_id: &str,
    project: &str,
    fallback_model: &str,
) -> Vec<UsageRecord> {
    let mut last_by_prompt: HashMap<String, UsageRecord> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut model = fallback_model.to_string();

    for value in parse_jsonl_values(content) {
        let params = value.get("params").cloned().unwrap_or_default();
        let update = params.get("update").cloned().unwrap_or_default();
        let update_meta = update.get("_meta").cloned().unwrap_or_default();
        let next_model = text_field(&update_meta, &["modelId"]);
        if !next_model.is_empty() {
            model = next_model;
        }
        let meta = params.get("_meta").cloned().unwrap_or_default();
        let prompt_id = text_field(&meta, &["promptId"]);
        let total = meta
            .get("totalTokens")
            .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|n| n as i64)));
        if prompt_id.is_empty() || total.is_none() {
            continue;
        }
        let occurred = meta
            .get("agentTimestampMs")
            .and_then(|v| v.as_i64())
            .map(millis_to_rfc3339)
            .unwrap_or_default();
        if !last_by_prompt.contains_key(&prompt_id) {
            order.push(prompt_id.clone());
        }
        last_by_prompt.insert(
            prompt_id,
            finish(UsageRecord {
                occurred_at: occurred,
                source: Source::Grok,
                model: model.clone(),
                provider: String::new(),
                project: project.to_string(),
                session_id: session_id.to_string(),
                source_file: source_file.to_string(),
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: total.unwrap_or(0),
                native_cost: None,
            }),
        );
    }

    order
        .into_iter()
        .filter_map(|id| last_by_prompt.remove(&id))
        .collect()
}

fn is_turn_completed(record: &Value, update: &Value) -> bool {
    let kind = update.get("sessionUpdate").and_then(|v| v.as_str());
    if kind == Some("turn_completed") {
        return true;
    }
    // 与 cc-switch 一致：缺 sessionUpdate 时，只收官方 `_x.ai/session/update` 上的 usage。
    kind.is_none() && record.get("method").and_then(|v| v.as_str()) == Some("_x.ai/session/update")
}

fn model_usages(usage: &Value) -> Vec<(String, Value)> {
    usage
        .get("modelUsage")
        .and_then(|v| v.as_object())
        .map(|map| {
            map.iter()
                .filter(|(_, counters)| counters.is_object())
                .map(|(model, counters)| (model.clone(), counters.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn ticks_to_usd(usage: &Value) -> Option<f64> {
    let ticks = i64_field(usage, &["costUsdTicks"]);
    (ticks > 0).then_some(ticks as f64 / USD_TICKS_PER_DOLLAR)
}

fn event_occurred_at(record: &Value, params: &Value) -> String {
    if let Some(ts) = record.get("timestamp") {
        if let Some(n) = ts.as_i64().or_else(|| ts.as_u64().map(|n| n as i64)) {
            let ms = if n > 100_000_000_000 {
                n
            } else {
                n.saturating_mul(1000)
            };
            return millis_to_rfc3339(ms);
        }
        if let Some(text) = ts.as_str() {
            if !text.is_empty() {
                return text.to_string();
            }
        }
    }
    params
        .get("_meta")
        .and_then(|meta| meta.get("agentTimestampMs"))
        .and_then(|v| v.as_i64())
        .map(millis_to_rfc3339)
        .unwrap_or_default()
}

fn path_session_id(source_file: &str) -> String {
    std::path::Path::new(source_file)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
}

fn path_project(source_file: &str) -> String {
    std::path::Path::new(source_file)
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(decode_url_dir)
        .unwrap_or_default()
}

fn millis_to_rfc3339(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}
