//! 对话记录的扫描根解析。
//!
//! 统一委托给 [`crate::scan_paths::ScanEnv`]：Cursor Agent 对话在 `~/.cursor/projects`
//! （跨主家目录与 WSL 家目录），与 token 包装目录隔离；OpenCode 信任根额外放行数据目录
//! 以兼容无库时的旧 JSON。

use std::path::{Path, PathBuf};

use crate::domain::Source;
use crate::scan_paths::ScanEnv;

/// 供目录刷新发现文件使用的扫描根。
pub(crate) fn catalog_roots_with_env(env: &ScanEnv, source: Source) -> Vec<PathBuf> {
    env.conversation_catalog_roots(source)
}

#[cfg(test)]
pub(crate) fn catalog_roots(
    overrides: &crate::ingest::PathOverrides,
    home: &Path,
    source: Source,
) -> Vec<PathBuf> {
    catalog_roots_with_env(&ScanEnv::from_overrides(home, overrides), source)
}

pub(crate) fn conversation_source_roots(home: &Path, source: Source) -> Vec<PathBuf> {
    ScanEnv::capture_for_home(home).conversation_trust_roots(source)
}
