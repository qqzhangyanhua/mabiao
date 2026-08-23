use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};

use crate::domain::WriteUserFileResult;

const BACKUP_KEEP: usize = 5;
const INSTRUCTION_BACKUP_DIR: &str = "instruction-backups";

pub fn write(
    home: &Path,
    data_dir: &Path,
    path: &Path,
    content: &str,
    expected_mtime: Option<&str>,
) -> Result<WriteUserFileResult, String> {
    if !is_allowed(home, path) {
        return Err("该路径不在可写名单中".into());
    }
    write_protected(
        home,
        data_dir,
        path,
        content.as_bytes(),
        expected_mtime,
        INSTRUCTION_BACKUP_DIR,
    )
}

pub fn observe_mtime(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = fs::metadata(path).map_err(|error| format!("读取目标文件状态失败：{error}"))?;
    Ok(mtime_rfc3339(&metadata))
}

pub fn write_export(
    path: &Path,
    content: &[u8],
    expected_mtime: Option<&str>,
) -> Result<WriteUserFileResult, String> {
    write_export_with(path, expected_mtime, |writer| {
        writer
            .write_all(content)
            .map_err(|error| format!("写入临时文件失败：{error}"))
    })
}

pub fn write_export_with(
    path: &Path,
    expected_mtime: Option<&str>,
    write_contents: impl FnOnce(&mut dyn Write) -> Result<(), String>,
) -> Result<WriteUserFileResult, String> {
    if !is_allowed_export(path) {
        return Err("导出路径不在可写名单中".into());
    }
    if expected_mtime.is_some() {
        return Err("导出目标已存在，请选择新文件名".into());
    }
    let tmp = temporary_path(path)?;
    let write_result = (|| {
        let mut file =
            fs::File::create(&tmp).map_err(|error| format!("写入临时文件失败：{error}"))?;
        write_contents(&mut file)?;
        file.flush()
            .map_err(|error| format!("写入临时文件失败：{error}"))
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    let result = fs::hard_link(&tmp, path);
    let _ = fs::remove_file(&tmp);
    match result {
        Ok(()) => {
            let meta = fs::metadata(path).map_err(|e| format!("写入后读取失败：{e}"))?;
            Ok(WriteUserFileResult {
                modified_at: mtime_rfc3339(&meta),
                byte_size: meta.len(),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err("导出目标已存在，请选择新文件名".to_string())
        }
        Err(error) => Err(format!("创建导出文件失败：{error}")),
    }
}

fn write_protected(
    home: &Path,
    data_dir: &Path,
    path: &Path,
    content: &[u8],
    expected_mtime: Option<&str>,
    backup_dir: &str,
) -> Result<WriteUserFileResult, String> {
    let exists = path.is_file();
    let current_mtime = if exists { observe_mtime(path)? } else { None };
    if current_mtime.as_deref() != expected_mtime {
        return Err("该文件在外部被修改过".into());
    }
    if exists {
        backup_original(data_dir, home, path, backup_dir)?;
    }
    atomic_write(path, content)?;
    let meta = fs::metadata(path).map_err(|e| format!("写入后读取失败：{e}"))?;
    Ok(WriteUserFileResult {
        modified_at: mtime_rfc3339(&meta),
        byte_size: meta.len(),
    })
}

pub fn is_allowed(home: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(home) else {
        return false;
    };
    let parts: Vec<_> = rel.iter().filter_map(|p| p.to_str()).collect();
    match parts.as_slice() {
        [".claude", "CLAUDE.md"] => true,
        [".claude", "rules", name] if is_plain_name(name) => true,
        [".codex", "AGENTS.md"] => true,
        [".codex", "AGENTS.override.md"] => true,
        [".gemini", "GEMINI.md"] => true,
        _ => false,
    }
}

fn is_allowed_export(path: &Path) -> bool {
    path.is_absolute()
        && matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("md" | "jsonl")
        )
}

fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

fn backup_original(
    data_dir: &Path,
    home: &Path,
    path: &Path,
    backup_dir: &str,
) -> Result<(), String> {
    let original = fs::read(path).map_err(|e| format!("备份前读取失败：{e}"))?;
    let dir = backup_dir_for(data_dir, home, path, backup_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败：{e}"))?;
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
    let dest = dir.join(format!("{stamp}.bak"));
    fs::write(&dest, original).map_err(|e| format!("写入备份失败：{e}"))?;
    prune_backups(&dir)?;
    Ok(())
}

fn backup_dir_for(data_dir: &Path, home: &Path, path: &Path, backup_dir: &str) -> PathBuf {
    let rel = path
        .strip_prefix(home)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(['/', '\\', ':'], "__");
    data_dir.join(backup_dir).join(rel)
}

fn prune_backups(dir: &Path) -> Result<(), String> {
    let mut files = fs::read_dir(dir)
        .map_err(|e| format!("读取备份目录失败：{e}"))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    files.sort();
    if files.len() <= BACKUP_KEEP {
        return Ok(());
    }
    for stale in files.iter().take(files.len() - BACKUP_KEEP) {
        let _ = fs::remove_file(stale);
    }
    Ok(())
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    let tmp = temporary_path(path)?;
    let write_result = fs::write(&tmp, content).map_err(|e| format!("写入临时文件失败：{e}"));
    if write_result.is_err() {
        let _ = fs::remove_file(&tmp);
        return write_result;
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            Err(format!("替换目标文件失败：{error}"))
        }
    }
}

fn temporary_path(path: &Path) -> Result<PathBuf, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    Ok(path.with_file_name(format!(
        ".{}.tmp-{nonce}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
    )))
}

fn mtime_rfc3339(meta: &fs::Metadata) -> Option<String> {
    let modified = meta.modified().ok()?;
    Some(DateTime::<Utc>::from(modified).to_rfc3339())
}
