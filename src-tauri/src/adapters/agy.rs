//! Antigravity（`agy`）Usage Source 骨架。
//!
//! 本票只接通检测、发现与路径覆盖；解析返回空（#238 才写消耗记录映射）。
//! 与官方额度 provider `antigravity` 分属两个维度，不合并。
//!
//! 两个默认根（同一产品、同一套库结构、同一个账号额度池）：
//! CLI `~/.gemini/antigravity-cli` 与 IDE `~/.gemini/antigravity-ide`，
//! 各自再进 `conversations/`。`AGY_DATA_DIR` 整体覆盖两个根（Claude Code 先例）。

use std::path::{Path, PathBuf};

use crate::domain::UsageRecord;
use crate::ingest::{self, PathOverrides};

pub(crate) const PATH_ENV: &str = "AGY_DATA_DIR";
const DEFAULT_ROOTS: [&str; 2] = [".gemini/antigravity-cli", ".gemini/antigravity-ide"];
const CONVERSATIONS: &str = "conversations";

pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    let roots = overrides.get(PATH_ENV).cloned().unwrap_or_else(|| {
        DEFAULT_ROOTS
            .iter()
            .map(|relative| home.join(relative))
            .collect()
    });
    roots
        .into_iter()
        .map(|root| root.join(CONVERSATIONS))
        .collect()
}

/// 只白名单 `.db`。同目录旧版加密 `.pb` 必须显式排除：尝试解析失败会计入
/// `files_failed`，进而按既有对账规则拖停整个来源。
pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        paths.extend(ingest::walk_files(root, "db")?);
    }
    Ok(paths)
}

/// 「已检测到」= 任一扫描根（已拼好的 `conversations/`）下至少有一个 `.db`。
/// 目录存在或只有加密 `.pb` 都不算检测到。
pub(crate) fn detected(dirs: &[PathBuf]) -> bool {
    dirs.iter().any(|root| {
        ingest::walk_files(root, "db")
            .map(|files| !files.is_empty())
            .unwrap_or(false)
    })
}

pub(crate) fn parse(_path: &Path, _scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    Ok(Vec::new())
}
