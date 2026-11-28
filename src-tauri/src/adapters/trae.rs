use std::path::{Path, PathBuf};

use crate::adapters::codex;
use crate::domain::{Source, UsageRecord};

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    codex::discover(roots)
}

pub fn parse(path: &Path, scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let mut records = codex::parse(path, scan_dir)?;
    for record in &mut records {
        record.source = Source::Trae;
    }
    Ok(records)
}
