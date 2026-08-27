use std::path::{Path, PathBuf};

use crate::adapters::{
    finish, i64_field, parse_jsonl_value_lines, parse_streaming_jsonl, text_field, LineFactory,
};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// token 包装目录，不是 CLI 原生会话库。会话与 IDE 共用 ~/.cursor。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(
        overrides,
        home,
        "CURSOR_AGENT_USAGE_DIR",
        ".cursor-agent-usage",
        "",
    )
}

/// 设置页优先展示与 IDE 共用的原生目录，包装目录只在真实存在时追加。
pub(crate) fn display_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![home.join(".cursor/chats"), home.join(".cursor/projects")];
    for dir in scan_dirs(overrides, home) {
        if dir.exists() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_cursor_agent_jsonl)
}

/// 解析 cursor-agent 无头 stream-json 的落盘 jsonl（由 scripts/cursor-agent-usage.py 采集）。
///
/// token 只出现在 `type=result` 事件的 `usage` 子对象里；model/cwd 来自开头的 `type=system` 事件。
/// 每条 result 归一为一条 Usage Record。详见 docs/probe/cursor-agent.md。
///
/// 下面两轮扫描都通过 `lines()` 重新拿一份新的行迭代器：这样调用方可以用磁盘流式读取
/// 而不必先把整份文件内容读进内存再扫两遍。
pub fn parse_cursor_agent_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    let mut model = String::new();
    let mut project = String::new();
    for value in parse_jsonl_value_lines(lines()) {
        if value.get("type").and_then(|v| v.as_str()) == Some("system") {
            let candidate_model = text_field(&value, &["model"]);
            if !candidate_model.is_empty() {
                model = candidate_model;
            }
            let candidate_cwd = text_field(&value, &["cwd"]);
            if !candidate_cwd.is_empty() {
                project = candidate_cwd;
            }
        }
    }

    let file_session = std::path::Path::new(source_file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let mut records = Vec::new();
    for value in parse_jsonl_value_lines(lines()) {
        if value.get("type").and_then(|v| v.as_str()) != Some("result") {
            continue;
        }
        let usage = match value.get("usage") {
            Some(usage) if !usage.is_null() => usage,
            _ => continue,
        };
        let session_id = {
            let session_id = text_field(&value, &["session_id"]);
            if session_id.is_empty() {
                file_session.clone()
            } else {
                session_id
            }
        };
        let record_model = {
            let record_model = text_field(&value, &["model"]);
            if record_model.is_empty() {
                model.clone()
            } else {
                record_model
            }
        };
        records.push(finish(UsageRecord {
            occurred_at: text_field(&value, &["captured_at"]),
            source: Source::CursorAgent,
            model: record_model,
            provider: String::new(),
            project: project.clone(),
            session_id,
            source_file: source_file.to_string(),
            input_tokens: i64_field(usage, &["inputTokens"]),
            output_tokens: i64_field(usage, &["outputTokens"]),
            cache_read_tokens: i64_field(usage, &["cacheReadTokens"]),
            cache_creation_tokens: i64_field(usage, &["cacheWriteTokens"]),
            reasoning_tokens: 0,
            total_tokens: 0,
            native_cost: None,
        }));
    }
    records
}
