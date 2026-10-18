use std::path::{Path, PathBuf};

use crate::adapters::extension_tasks::{self, KILO_EXTENSION_ID};
use crate::domain::{Source, UsageRecord};
use crate::ingest::PathOverrides;

/// Kilo Code VS Code 扩展任务目录。
///
/// `$KILO_TASKS_DIR` 整体替换，指向 `…/globalStorage/kilocode.kilo-code/tasks`。
/// 未覆盖时扫 VS Code / Insiders / VSCodium / Cursor / Windsurf / Trae 以及
/// vscode-server 的同一相对路径。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    extension_tasks::scan_dirs(overrides, home, "KILO_TASKS_DIR", KILO_EXTENSION_ID)
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    extension_tasks::discover(roots)
}

pub(crate) fn sidecar_fingerprint(path: &Path, dirs: &[PathBuf]) -> String {
    extension_tasks::sidecar_fingerprint(path, dirs)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    extension_tasks::parse(path, Source::Kilo)
}
