use std::path::{Path, PathBuf};

use crate::adapters::{parse_streaming_jsonl, LineFactory};
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// Qoder 国内站 CLI（`qoderclicn`）会话同样是 Claude Code 形态 jsonl：
/// `$QODERCN_CONFIG_DIR`，否则 `~/.qoder-cn`，再扫 `projects/`。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(
        overrides,
        home,
        "QODERCN_CONFIG_DIR",
        ".qoder-cn",
        "projects",
    )
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_streaming_jsonl(path, parse_qoder_cn_jsonl)
}

pub fn parse_qoder_cn_jsonl(lines: &LineFactory<'_>, source_file: &str) -> Vec<UsageRecord> {
    crate::adapters::claude::parse_claude_shaped_jsonl(lines, source_file, Source::QoderCn)
}
