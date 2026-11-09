use std::path::{Path, PathBuf};

use crate::adapters::extension_tasks::{self, CLINE_EXTENSION_ID};
use crate::domain::{Source, UsageRecord};
use crate::ingest::PathOverrides;

const PATH_ENV: &str = "CLINE_IDE_TASKS_DIR";

/// Cline VS Code 扩展任务目录。
///
/// `$CLINE_IDE_TASKS_DIR` 整体替换，指向 `…/globalStorage/saoudrizwan.claude-dev/tasks`。
/// 未覆盖时扫 VS Code / Insiders / VSCodium / Cursor / Windsurf / Trae 以及
/// vscode-server 的同一相对路径，并追加 `~/.cline/data/tasks`（与 CLI 的
/// `sessions/*.messages.json` 不是同一套文件）。
///
/// `tokensIn` 已不含 cache（扩展把 `cacheReads` / `cacheWrites` 另记）。
/// Cline 4.x 写到 `~/.cline/data/sessions` 的用量仍走 `Source::Cline`。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    if overrides.contains_key(PATH_ENV) {
        return extension_tasks::scan_dirs(overrides, home, PATH_ENV, CLINE_EXTENSION_ID);
    }
    let mut dirs = extension_tasks::scan_dirs(overrides, home, PATH_ENV, CLINE_EXTENSION_ID);
    let home_tasks = home.join(".cline/data/tasks");
    if !dirs.contains(&home_tasks) {
        dirs.push(home_tasks);
    }
    dirs
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    extension_tasks::discover(roots)
}

pub(crate) fn sidecar_fingerprint(path: &Path, dirs: &[PathBuf]) -> String {
    extension_tasks::sidecar_fingerprint(path, dirs)
}

pub(crate) fn parse(path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    extension_tasks::parse_with_input_policy(path, Source::ClineIde, false)
}
