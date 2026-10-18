//! VS Code 扩展任务目录（Roo Code / Kilo Code 共用）。
//!
//! 每个任务一个文件夹，主文件是 `ui_messages.json`。`say == "api_req_started"`
//! 的 `text` 是嵌套 JSON，含 `tokensIn` / `tokensOut` / `cacheReads` /
//! `cacheWrites` / `cost` / `apiProtocol`。
//!
//! `tokensIn` 是含 cache 的 `totalInputTokens`（Roo Code PR #8954、Kilo
//! `src/shared/cost.ts` / `openai-native.ts` / Gemini `uncachedInputTokens =
//! inputTokens - cacheReadTokens`）：入账时拆出 cache，避免和
//! `cacheReads`/`cacheWrites` 重复计数。模型名从同目录
//! `api_conversation_history.json` 的 `<model>` 环境标签按请求顺序配对。
//! Kilo 另有 `usageMissing`：为 true 时该轮没有用量，跳过。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::{finish, has_billable_tokens, i64_field, text_field};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

pub(crate) const ROO_EXTENSION_ID: &str = "rooveterinaryinc.roo-cline";
pub(crate) const KILO_EXTENSION_ID: &str = "kilocode.kilo-code";
pub(crate) const UI_MESSAGES: &str = "ui_messages.json";
pub(crate) const API_HISTORY: &str = "api_conversation_history.json";
pub(crate) const HISTORY_ITEM: &str = "history_item.json";

const EDITOR_APPS: &[&str] = &[
    "Code",
    "Code - Insiders",
    "Code - OSS",
    "VSCodium",
    "VSCodium - Insiders",
    "Cursor",
    "Windsurf",
    "Trae",
    "Trae CN",
];

const SERVER_ROOTS: &[&str] = &[
    ".vscode-server/data",
    ".vscode-insiders-server/data",
    ".cursor-server/data",
];

pub(crate) fn scan_dirs(
    overrides: &PathOverrides,
    home: &Path,
    env: &str,
    extension_id: &str,
) -> Vec<PathBuf> {
    overrides.get(env).cloned().unwrap_or_else(|| {
        let leaf = format!("User/globalStorage/{extension_id}/tasks");
        let mut dirs = Vec::new();
        for app in EDITOR_APPS {
            for root in [
                home.join(".config").join(app),
                home.join("Library/Application Support").join(app),
                home.join("AppData/Roaming").join(app),
            ] {
                dirs.push(root.join(&leaf));
            }
        }
        for root in SERVER_ROOTS {
            dirs.push(home.join(root).join(&leaf));
        }
        dirs
    })
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    crate::adapters::discover_suffix(roots, UI_MESSAGES)
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    let Some(parent) = path.parent() else {
        return String::new();
    };
    [API_HISTORY, HISTORY_ITEM]
        .into_iter()
        .filter_map(|name| {
            let sidecar = parent.join(name);
            sidecar
                .exists()
                .then(|| ingest::metadata_fingerprint(&sidecar))
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub(crate) fn parse(path: &Path, source: Source) -> Result<Vec<UsageRecord>, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let content = std::str::from_utf8(&bytes)
        .map_err(|error| error.to_string())?
        .trim_start_matches('\u{feff}');
    serde_json::from_str::<serde::de::IgnoredAny>(content).map_err(|error| error.to_string())?;
    Ok(parse_ui_messages(content, path, source))
}

pub(crate) fn parse_ui_messages(content: &str, path: &Path, source: Source) -> Vec<UsageRecord> {
    let value: Value = match serde_json::from_str(content.trim_start_matches('\u{feff}')) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let messages = match value.as_array() {
        Some(messages) => messages,
        None => return Vec::new(),
    };
    let parent = path.parent();
    let history = parent.and_then(|dir| read_json(&dir.join(API_HISTORY)));
    let history_item = parent.and_then(|dir| read_json(&dir.join(HISTORY_ITEM)));
    let models = history
        .as_ref()
        .map(models_from_history)
        .unwrap_or_default();
    let project = project_from_sidecars(history_item.as_ref(), history.as_ref());
    let session_id = parent
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    let source_file = path.to_string_lossy().into_owned();

    let mut model_index = 0usize;
    let mut last_model = String::new();
    let mut records = Vec::new();
    for message in messages {
        let Some(info) = api_req_info(message) else {
            continue;
        };
        if info.get("usageMissing").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let cache_read = i64_field(&info, &["cacheReads", "cache_reads"]).max(0);
        let cache_write = i64_field(&info, &["cacheWrites", "cache_writes"]).max(0);
        let raw_input = i64_field(&info, &["tokensIn", "tokens_in"]).max(0);
        let output = i64_field(&info, &["tokensOut", "tokens_out"]).max(0);
        // tokensIn 是含 cache 的总量；拆出后再入账。
        let input = (raw_input - cache_read - cache_write).max(0);
        let model = next_model(&models, &mut model_index, &mut last_model);
        if model.is_empty() {
            continue;
        }
        let record = finish(UsageRecord {
            occurred_at: occurred_at_from_message(message),
            source,
            model,
            provider: provider_from_info(&info),
            project: project.clone(),
            session_id: session_id.clone(),
            source_file: source_file.clone(),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_write,
            cache_creation_1h_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            native_cost: native_cost_from_info(&info),
        });
        if has_billable_tokens(&record) {
            records.push(record);
        }
    }
    records
}

fn api_req_info(message: &Value) -> Option<Value> {
    let say = text_field(message, &["say"]);
    let kind = text_field(message, &["type", "entry_type"]);
    if say != "api_req_started" && kind != "api_req_started" {
        return None;
    }
    let text = match message.get("text") {
        Some(Value::String(text)) => text.as_str(),
        _ => return None,
    };
    serde_json::from_str(text).ok()
}

fn next_model(models: &[String], index: &mut usize, last: &mut String) -> String {
    if *index < models.len() {
        last.clone_from(&models[*index]);
        *index += 1;
    }
    last.clone()
}

pub(crate) fn models_from_history(history: &Value) -> Vec<String> {
    let mut models = Vec::new();
    let Some(messages) = history.as_array() else {
        return models;
    };
    for message in messages {
        if text_field(message, &["role"]) != "user" {
            continue;
        }
        let blob = content_blob(message.get("content").unwrap_or(&Value::Null));
        for model in extract_tags(&blob, "model") {
            if !model.is_empty() {
                models.push(model);
            }
        }
    }
    models
}

pub(crate) fn project_from_sidecars(
    history_item: Option<&Value>,
    history: Option<&Value>,
) -> String {
    if let Some(item) = history_item {
        let workspace = text_field(item, &["workspace", "cwd"]);
        if !workspace.is_empty() {
            return workspace;
        }
    }
    let Some(history) = history else {
        return String::new();
    };
    let Some(messages) = history.as_array() else {
        return String::new();
    };
    for message in messages {
        let blob = content_blob(message.get("content").unwrap_or(&Value::Null));
        if let Some(project) = workspace_from_environment(&blob) {
            return project;
        }
    }
    String::new()
}

pub(crate) fn workspace_from_environment(blob: &str) -> Option<String> {
    let marker = "# Current Workspace Directory (";
    let start = blob.find(marker)? + marker.len();
    let end = blob[start..].find(')')?;
    let path = blob[start..start + end].trim();
    (!path.is_empty()).then(|| path.to_string())
}

pub(crate) fn extract_tags(blob: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut values = Vec::new();
    let mut rest = blob;
    while let Some(start) = rest.find(&open) {
        rest = &rest[start + open.len()..];
        let Some(end) = rest.find(&close) else {
            break;
        };
        let value = rest[..end].trim();
        if !value.is_empty() {
            values.push(value.to_string());
        }
        rest = &rest[end + close.len()..];
    }
    values
}

pub(crate) fn content_blob(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.clone()),
                Value::Object(_) => item.get("text").and_then(Value::as_str).map(str::to_string),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(_) => content
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

fn provider_from_info(info: &Value) -> String {
    let protocol = text_field(info, &["apiProtocol", "api_protocol"]);
    let raw = if protocol.is_empty() {
        text_field(info, &["inferenceProvider", "inference_provider"])
    } else {
        protocol
    };
    if raw.is_empty() {
        return String::new();
    }
    raw.split(['/', ':']).next().unwrap_or(&raw).to_string()
}

fn native_cost_from_info(info: &Value) -> Option<f64> {
    let amount = info.get("cost").and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_i64().map(|n| n as f64))
            .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
    })?;
    (amount > 0.0).then_some(amount)
}

fn occurred_at_from_message(message: &Value) -> String {
    let ts = i64_field(message, &["ts"]);
    if ts <= 0 {
        return String::new();
    }
    let millis = if ts > 1_000_000_000_000 {
        ts
    } else {
        ts * 1000
    };
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}
