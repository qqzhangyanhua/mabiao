use std::path::{Path, PathBuf};

use crate::adapters::{parse_streaming_jsonl, LineFactory};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// Qoder CLI（`qodercli`）把会话写成 Claude Code 形态的 jsonl：
/// `$QODER_CONFIG_DIR`，否则 `~/.qoder`，再扫 `projects/`。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "QODER_CONFIG_DIR", ".qoder", "projects")
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_qoder_jsonl)
}

pub fn parse_qoder_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    crate::adapters::claude::parse_claude_shaped_jsonl(lines, source_file, Source::Qoder)
}
