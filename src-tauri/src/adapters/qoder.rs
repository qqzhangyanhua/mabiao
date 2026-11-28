use std::path::Path;

use crate::adapters::{parse_streaming_jsonl, LineFactory};
use crate::domain::{Source, UsageRecord};

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_qoder_jsonl)
}

pub fn parse_qoder_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    crate::adapters::claude::parse_claude_shaped_jsonl(lines, source_file, Source::Qoder)
}
