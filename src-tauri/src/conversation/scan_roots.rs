//! 对话记录的扫描根解析。
//!
//! Cursor Agent 对话在 `~/.cursor/projects`，与 token 包装目录不是同一条路径。
//! 打开对话详情不经过这里的公开面：需要根目录的调用方直接找本模块。

use std::path::{Path, PathBuf};

use crate::domain::Source;
use crate::ingest;

/// Cursor Agent 对话在 `~/.cursor/projects`，与 token 包装目录不是同一条路径。
pub(crate) fn catalog_roots(
    overrides: &crate::ingest::PathOverrides,
    home: &Path,
    source: Source,
) -> Vec<PathBuf> {
    if source == Source::CursorAgent {
        vec![home.join(".cursor/projects")]
    } else {
        ingest::source_scan_dirs_with(overrides, home, source)
    }
}

pub(crate) fn conversation_source_roots(home: &Path, source: Source) -> Vec<PathBuf> {
    catalog_roots(&ingest::path_overrides(), home, source)
}
