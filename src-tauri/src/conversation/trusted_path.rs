//! 路径白名单与 canonical 校验，以及会话文件修订计算。
//!
//! 读盘范围是否越界只在这里判定。四个修订函数与路径校验同处，因为
//! `checked_detail_file_revision` 本身要先过 canonical 校验。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::domain::{ConversationSessionRow, Source};

use super::conversation_adapter;
use super::read::conversation_source_roots;

pub(crate) fn modified_nanos(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
        .unwrap_or(0)
}

pub(crate) fn metadata_revision(metadata: &fs::Metadata) -> String {
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{modified_ns}:{}", metadata.len())
}

pub(crate) fn ensure_trusted_path(path: &Path, roots: &[PathBuf]) -> Result<(), String> {
    let canonical_path =
        fs::canonicalize(path).map_err(|error| format!("无法验证原始文件路径：{error}"))?;
    ensure_canonical_path_in_roots(&canonical_path, roots)
}

pub(crate) fn ensure_canonical_path_in_roots(
    canonical_path: &Path,
    roots: &[PathBuf],
) -> Result<(), String> {
    for root in roots {
        let Ok(canonical_root) = fs::canonicalize(root) else {
            continue;
        };
        if canonical_path.starts_with(canonical_root) {
            return Ok(());
        }
    }
    Err("原始文件不在该来源允许的扫描目录内".to_string())
}

pub(crate) fn session_source_paths(
    session: &ConversationSessionRow,
) -> Result<Vec<PathBuf>, String> {
    let representative = PathBuf::from(&session.source_file);
    let paths = if session.source_files.is_empty() {
        vec![representative.clone()]
    } else {
        session.source_files.iter().map(PathBuf::from).collect()
    };
    if !paths.iter().any(|path| path == &representative) {
        return Err("会话索引的代表文件与来源清单不一致".to_string());
    }
    Ok(paths)
}

pub(crate) fn trusted_paths_for_session(
    home: &Path,
    source: Source,
    session: &ConversationSessionRow,
) -> Result<Vec<PathBuf>, String> {
    let roots = conversation_source_roots(home, source);
    let representative = PathBuf::from(&session.source_file);
    if !representative.exists() {
        return Err("原文件已删除，详情不可读取".to_string());
    }
    ensure_trusted_path(&representative, &roots)?;
    let paths = session_source_paths(session)?;
    for path in &paths {
        if !path.exists() {
            return Err("原文件已删除，详情不可读取".to_string());
        }
        ensure_trusted_path(path, &roots)?;
    }
    Ok(paths)
}

pub(crate) fn files_revision(source: Source, paths: &[PathBuf]) -> Result<String, String> {
    let adapter = conversation_adapter(source)?;
    let revisions = paths
        .iter()
        .map(|path| {
            (adapter.revision)(path).map(|revision| (path.to_string_lossy().to_string(), revision))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let [(_, revision)] = revisions.as_slice() {
        return Ok(revision.clone());
    }
    serde_json::to_string(&revisions).map_err(|error| error.to_string())
}

pub(crate) fn detail_files_revision(
    source: Source,
    paths: &[PathBuf],
    roots: &[PathBuf],
) -> Result<Option<String>, String> {
    let mut revisions = Vec::with_capacity(paths.len());
    for path in paths {
        let Some(revision) = detail_file_revision(source, path, roots)? else {
            return Ok(None);
        };
        revisions.push((path.to_string_lossy().to_string(), revision));
    }
    if let [(_, revision)] = revisions.as_slice() {
        return Ok(Some(revision.clone()));
    }
    serde_json::to_string(&revisions)
        .map(Some)
        .map_err(|error| error.to_string())
}

pub(crate) fn detail_file_revision(
    source: Source,
    path: &Path,
    roots: &[PathBuf],
) -> Result<Option<String>, String> {
    let revision = conversation_adapter(source)?.revision;
    checked_detail_file_revision(
        roots,
        || fs::canonicalize(path),
        |canonical_path| revision(canonical_path).map_err(std::io::Error::other),
    )
}

pub(crate) fn checked_detail_file_revision(
    roots: &[PathBuf],
    canonicalize_file: impl FnOnce() -> std::io::Result<PathBuf>,
    read_revision: impl FnOnce(&Path) -> std::io::Result<String>,
) -> Result<Option<String>, String> {
    let canonical_path = match canonicalize_file() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法验证原始文件路径：{error}")),
    };
    ensure_canonical_path_in_roots(&canonical_path, roots)?;
    match read_revision(&canonical_path) {
        Ok(revision) => Ok(Some(revision)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("读取原始文件元数据失败：{error}")),
    }
}
