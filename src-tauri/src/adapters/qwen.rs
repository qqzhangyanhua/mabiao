use std::path::{Path, PathBuf};

use crate::adapters::parse_whole_json;
use crate::domain::UsageRecord;
use crate::ingest::{self, PathOverrides};

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    ingest::resolve_dirs(overrides, home, "QWEN_DATA_DIR", ".qwen", "tmp")
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        for path in ingest::walk_files(root, "json")? {
            if path.file_name().and_then(|name| name.to_str()) == Some("logs.json") {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    parse_whole_json(path, parse_qwen_session)
}

pub fn parse_qwen_session(_content: &str, _source_file: &str) -> Vec<UsageRecord> {
    Vec::new()
}
