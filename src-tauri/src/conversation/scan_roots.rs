//! 对话记录的扫描根解析。
//!
//! Cursor Agent 对话在 `~/.cursor/projects`，与 token 包装目录不是同一条路径。
//! 目录刷新用 [`catalog_roots`]（发现文件）。打开详情用 [`conversation_source_roots`]，
//! OpenCode 在库文件之外再放行数据目录，否则没有 `opencode.db` 时 `storage/` 下的旧 JSON
//! 过不了路径白名单。

use std::path::{Path, PathBuf};

use crate::domain::Source;
use crate::ingest;

/// Cursor Agent 对话在 `~/.cursor/projects`，与 token 包装目录不是同一条路径。
///
/// 这里返回的是发现用的根。OpenCode 仍是 `opencode.db` 文件路径：库在就只读库，
/// 库不在才由发现逻辑去父目录的 `storage/`。不要把数据目录塞进来，否则发现会把
/// 数据目录当成「缺失的库文件」，改去扫它的上一级。
pub(crate) fn catalog_roots(
    overrides: &crate::ingest::PathOverrides,
    home: &Path,
    source: Source,
) -> Vec<PathBuf> {
    if source == Source::CursorAgent {
        crate::wsl::scan_homes(home)
            .into_iter()
            .map(|scan_home| scan_home.join(".cursor/projects"))
            .collect()
    } else {
        ingest::source_scan_dirs_with(overrides, home, source)
    }
}

pub(crate) fn conversation_source_roots(home: &Path, source: Source) -> Vec<PathBuf> {
    let roots = catalog_roots(&ingest::path_overrides(), home, source);
    if source == Source::Opencode {
        opencode_trust_roots(&roots)
    } else {
        roots
    }
}

/// 扫描结果是库文件。详情要放行两处：库文件本身（绝对路径的 `OPENCODE_DB` 可以在数据目录外），
/// 以及库文件的父目录（没有库时，1.2 以前的会话 JSON 在这个数据目录的 `storage/` 下）。
/// 父目录若已是文件系统根则不放行，避免把整块盘放进白名单。
fn opencode_trust_roots(scan_files: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::with_capacity(scan_files.len().saturating_mul(2));
    for path in scan_files {
        if let Some(parent) = path.parent().filter(|parent| parent.parent().is_some()) {
            roots.push(parent.to_path_buf());
        }
        roots.push(path.clone());
    }
    roots.sort();
    roots.dedup();
    roots
}
