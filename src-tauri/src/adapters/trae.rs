use std::path::{Path, PathBuf};

use crate::adapters::codex;
use crate::domain::{Source, UsageRecord};
use crate::ingest::PathOverrides;

/// Trae CLI 2.x（`traex`）是 Codex 的闭源 fork，会话写在 `~/.trae/cli`。
/// `$TRAE_HOME` 覆盖 `~/.trae`；扫 `cli/sessions` 与 `cli/archived_sessions`。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    let roots = overrides
        .get("TRAE_HOME")
        .cloned()
        .unwrap_or_else(|| vec![home.join(".trae")]);
    roots
        .into_iter()
        .flat_map(|root| {
            let cli = root.join("cli");
            [cli.join("sessions"), cli.join("archived_sessions")]
        })
        .collect()
}

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
