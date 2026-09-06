use serde::de::DeserializeOwned;

use crate::domain::{EngineCommand, WorkNotesEntry};

use super::engines::EngineRunner;
use super::input::take_chars;

pub enum ParseOutcome<T> {
    Parsed(T),
    Plain(String),
    Failed(String),
}

pub fn run<T: DeserializeOwned>(
    runner: &dyn EngineRunner,
    command: &EngineCommand,
) -> ParseOutcome<T> {
    run_with(runner, command, parse_structured)
}

pub fn run_with<T>(
    runner: &dyn EngineRunner,
    command: &EngineCommand,
    parse: impl Fn(&str) -> Option<T>,
) -> ParseOutcome<T> {
    let first = match runner.run(command) {
        Ok(stdout) => stdout,
        Err(error) => return ParseOutcome::Failed(error),
    };
    if let Some(value) = parse(&first) {
        return ParseOutcome::Parsed(value);
    }
    let second = match runner.run(command) {
        Ok(stdout) => stdout,
        Err(error) => return ParseOutcome::Failed(error),
    };
    if let Some(value) = parse(&second) {
        return ParseOutcome::Parsed(value);
    }
    ParseOutcome::Plain(second)
}

pub fn parse_structured<T: DeserializeOwned>(raw: &str) -> Option<T> {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Some(value);
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
