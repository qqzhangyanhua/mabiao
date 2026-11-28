use std::path::{Path, PathBuf};

use crate::adapters::extension_tasks;
use crate::domain::{Source, UsageRecord};

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    extension_tasks::discover(roots)
}

pub(crate) fn sidecar_fingerprint(path: &Path, dirs: &[PathBuf]) -> String {
    extension_tasks::sidecar_fingerprint(path, dirs)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    extension_tasks::parse(path, Source::Roo)
}
