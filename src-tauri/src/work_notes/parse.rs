use std::sync::atomic::AtomicBool;

use serde::de::DeserializeOwned;

use crate::domain::{EngineCommand, WorkNotesEntry};

use super::engines::{EngineError, EngineRunner};
use super::input::take_chars;
use super::usage::{self, EngineUsage};

pub enum ParseOutcome<T> {
    Parsed(T),
    Plain(String),
    Failed(String),
    Cancelled,
}

pub struct Parsed<T> {
    pub outcome: ParseOutcome<T>,
    pub usage: EngineUsage,
}

pub fn run<T: DeserializeOwned>(
    runner: &dyn EngineRunner,
    make: impl FnMut() -> Result<EngineCommand, String>,
    cancel: &AtomicBool,
) -> Parsed<T> {
    run_with(runner, make, cancel, parse_structured)
}

pub fn run_with<T>(
    runner: &dyn EngineRunner,
    mut make: impl FnMut() -> Result<EngineCommand, String>,
    cancel: &AtomicBool,
    parse: impl Fn(&str) -> Option<T>,
) -> Parsed<T> {
    let mut usage = EngineUsage::default();
    let first_command = match make() {
        Ok(command) => command,
        Err(error) => {
            return Parsed {
                outcome: ParseOutcome::Failed(error),
                usage,
            };
        }
    };
    let first = match runner.run(&first_command, cancel) {
        Ok(stdout) => stdout,
        Err(EngineError::Cancelled) => {
            return Parsed {
                outcome: ParseOutcome::Cancelled,
                usage,
            };
        }
        Err(EngineError::Failed(error)) => {
            return Parsed {
                outcome: ParseOutcome::Failed(error),
                usage,
            };
        }
    };
    let split = usage::split_output(&first);
    usage.add(&split.usage);
    if let Some(value) = parse(&split.payload) {
        return Parsed {
            outcome: ParseOutcome::Parsed(value),
            usage,
        };
    }
    let second_command = match make() {
        Ok(command) => command,
        Err(error) => {
            return Parsed {
                outcome: ParseOutcome::Failed(error),
                usage,
            };
        }
    };
    let second = match runner.run(&second_command, cancel) {
        Ok(stdout) => stdout,
        Err(EngineError::Cancelled) => {
            return Parsed {
                outcome: ParseOutcome::Cancelled,
                usage,
            };
        }
        Err(EngineError::Failed(error)) => {
            return Parsed {
                outcome: ParseOutcome::Failed(error),
                usage,
            };
        }
    };
    let split = usage::split_output(&second);
    usage.add(&split.usage);
    if let Some(value) = parse(&split.payload) {
        return Parsed {
            outcome: ParseOutcome::Parsed(value),
            usage,
        };
    }
    Parsed {
        outcome: ParseOutcome::Plain(split.payload),
        usage,
    }
}

pub fn parse_structured<T: DeserializeOwned>(raw: &str) -> Option<T> {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Some(value);
    }
    // grok 有时把 JSON 对象序列化为字符串后再放入 text 字段，split_output 拿到的是
    // 内层字符串（已 JSON 解码），直接 parse 即可；但如果拿到的仍是外层编码字符串
    // （形如 `"{ \"headline\": ... }"`），则需多解一层。
    if let Ok(serde_json::Value::String(inner)) = serde_json::from_str::<serde_json::Value>(trimmed)
    {
        if let Ok(value) = serde_json::from_str(&inner) {
            return Some(value);
        }
    }
    let block = extract_json_fence(trimmed)?;
    serde_json::from_str(block).ok()
}

pub fn degrade_reduce(raw: &str) -> (String, Vec<WorkNotesEntry>, String) {
    (
        String::new(),
        vec![WorkNotesEntry {
            title: "未能解析".to_string(),
            detail: take_chars(raw.trim(), 1000),
            project: String::new(),
        }],
        String::new(),
    )
}

fn extract_json_fence(raw: &str) -> Option<&str> {
    let lower = raw.to_ascii_lowercase();
    let start = if let Some(index) = lower.find("```json") {
        index + 7
    } else {
        lower.find("```").map(|index| index + 3)?
    };
    let rest = raw.get(start..)?;
    let rest = rest.trim_start_matches(['\r', '\n', ' ', '\t']);
    let close = rest.find("```")?;
    Some(rest.get(..close)?.trim())
}
