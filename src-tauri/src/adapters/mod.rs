pub mod claude;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod cursor_account;
pub mod cursor_agent;
pub mod cursor_session;
pub mod dsh;
pub mod factory;
pub mod gemini;
pub mod grok;
pub mod kimi;
pub mod opencode;
pub mod pi;
pub mod project;
pub mod qwen;

use crate::domain::UsageRecord;

/// 惰性逐行产出 `Value`，同一时刻只有一行的解析结果活着。
///
/// 会话 jsonl 单文件可以到几十 MB，`Value` 的堆表示又是原文的数倍；
/// 一次性 collect 成 `Vec` 会让整轮摄取的常驻内存和最大文件成正比。
/// 需要多趟扫描的适配器请重复调用本函数，重复解析比把整份文件留在内存里便宜。
pub fn parse_jsonl_values(content: &str) -> impl Iterator<Item = serde_json::Value> + '_ {
    content.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        serde_json::from_str(line).ok()
    })
}

pub fn i64_field(value: &serde_json::Value, keys: &[&str]) -> i64 {
    for key in keys {
        if let Some(n) = value.get(key).and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_u64().map(|n| n as i64))
                .or_else(|| v.as_f64().map(|n| n.round() as i64))
        }) {
            return n;
        }
    }
    0
}

pub fn text_field(value: &serde_json::Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(s) = value.get(*key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    String::new()
}

pub fn finish(record: UsageRecord) -> UsageRecord {
    record.with_total()
}

pub fn has_billable_tokens(record: &UsageRecord) -> bool {
    record.input_tokens > 0
        || record.output_tokens > 0
        || record.cache_read_tokens > 0
        || record.cache_creation_tokens > 0
        || record.reasoning_tokens > 0
}
