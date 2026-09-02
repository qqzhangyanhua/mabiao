//! 本机缓存与用户配置的备份/恢复。不含 Cursor 钥匙串 token。

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use rusqlite::{backup::Backup, Connection};
use serde::{Deserialize, Serialize};

use crate::official_quota::custom::store::CONFIG_NAME as CUSTOM_QUOTA_NAME;
use crate::scan_paths::CONFIG_NAME as SCAN_PATHS_NAME;

pub const MANIFEST_NAME: &str = "manifest.json";
pub const DB_NAME: &str = "usage.sqlite";
pub const PRICES_NAME: &str = "prices.json";
pub const SNAPSHOT_NAME: &str = "litellm_prices.json";
pub const BUDGET_NAME: &str = "budget.json";
pub const BUDGET_NOTIFY_NAME: &str = "budget_notify_state.json";
pub const OFFICIAL_QUOTA_NAME: &str = "official_quota.json";
pub const OFFICIAL_QUOTA_NOTIFY_NAME: &str = "official_quota_notify_state.json";

const STAGING_DIR: &str = ".restore-staging";
const BAK_SUFFIX: &str = ".restore-bak";

#[derive(Debug, Clone)]
pub struct AppDataPaths {
    pub db_path: PathBuf,
    pub prices_path: PathBuf,
    pub snapshot_path: PathBuf,
    pub budget_path: PathBuf,
    pub budget_notify_path: PathBuf,
    pub official_quota_path: PathBuf,
    pub official_quota_notify_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub created_at: String,
    pub files: Vec<String>,
    pub note: String,
}

fn default_note() -> String {
    "不含 Cursor 钥匙串中的 WorkosCursorSessionToken，也不含对话事件正文和自定义提供商密钥；恢复会覆盖当前缓存、单价/预算/扫描路径配置与自定义提供商配置（不含密钥）。"
        .to_string()
}

fn copy_if_exists(
    src: &Path,
    dest: &Path,
    files: &mut Vec<String>,
    name: &str,
) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }
    fs::copy(src, dest).map_err(|e| format!("复制 {name} 失败：{e}"))?;
    files.push(name.to_string());
    Ok(())
}

pub fn backup_sqlite(conn: &Connection, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if dest.exists() {
        fs::remove_file(dest).map_err(|e| e.to_string())?;
    }
    let mut target = Connection::open(dest).map_err(|e| e.to_string())?;
    {
        let backup = Backup::new(conn, &mut target).map_err(|e| e.to_string())?;
        backup
            .run_to_completion(100, std::time::Duration::from_millis(0), None)
            .map_err(|e| e.to_string())?;
    }
    strip_conversation_event_index(&target)
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        rusqlite::params![name],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

fn conversation_sessions_has_generation(conn: &Connection) -> Result<bool, String> {
    let mut statement = conn
        .prepare("PRAGMA table_info(conversation_sessions)")
        .map_err(|e| e.to_string())?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(names.iter().any(|name| name == "event_index_generation"))
}

/// 事件索引是派生缓存，且含完整对话正文。备份只留目录元数据，恢复后走回退路径再渐进补建。
fn strip_conversation_event_index(conn: &Connection) -> Result<(), String> {
    if table_exists(conn, "conversation_events")? {
        conn.execute("DROP TABLE conversation_events", [])
            .map_err(|e| e.to_string())?;
    }
    if table_exists(conn, "conversation_sessions")? && conversation_sessions_has_generation(conn)? {
        conn.execute(
            "UPDATE conversation_sessions SET event_index_generation = NULL",
            [],
        )
        .map_err(|e| e.to_string())?;
    }
    conn.execute("VACUUM", []).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn backup_to(
    conn: &Connection,
    dest_dir: &Path,
    paths: &AppDataPaths,
) -> Result<BackupManifest, String> {
    fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    let mut files = Vec::new();

    backup_sqlite(conn, &dest_dir.join(DB_NAME))?;
    files.push(DB_NAME.to_string());

    copy_if_exists(
        &paths.prices_path,
        &dest_dir.join(PRICES_NAME),
        &mut files,
        PRICES_NAME,
    )?;
    copy_if_exists(
        &paths.snapshot_path,
        &dest_dir.join(SNAPSHOT_NAME),
        &mut files,
        SNAPSHOT_NAME,
    )?;
    copy_if_exists(
        &paths.budget_path,
        &dest_dir.join(BUDGET_NAME),
        &mut files,
        BUDGET_NAME,
    )?;
    copy_if_exists(
        &paths.budget_notify_path,
        &dest_dir.join(BUDGET_NOTIFY_NAME),
        &mut files,
        BUDGET_NOTIFY_NAME,
    )?;
    copy_if_exists(
        &paths.official_quota_path,
        &dest_dir.join(OFFICIAL_QUOTA_NAME),
        &mut files,
        OFFICIAL_QUOTA_NAME,
    )?;
    copy_if_exists(
        &paths.official_quota_notify_path,
        &dest_dir.join(OFFICIAL_QUOTA_NOTIFY_NAME),
        &mut files,
        OFFICIAL_QUOTA_NOTIFY_NAME,
    )?;
    // 配置可以进备份，凭证不行。路径跟官方额度配置同目录，不单独挂到
    // AppDataPaths 上——那会让恢复函数手里握着凭证路径，容易误覆盖。
    copy_if_exists(
        &sibling(&paths.official_quota_path, CUSTOM_QUOTA_NAME),
        &dest_dir.join(CUSTOM_QUOTA_NAME),
        &mut files,
        CUSTOM_QUOTA_NAME,
    )?;
    copy_if_exists(
        &sibling(&paths.budget_path, SCAN_PATHS_NAME),
        &dest_dir.join(SCAN_PATHS_NAME),
        &mut files,
        SCAN_PATHS_NAME,
    )?;

    let manifest = BackupManifest {
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        files,
        note: default_note(),
    };
    fs::write(
        dest_dir.join(MANIFEST_NAME),
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(manifest)
}

pub fn load_manifest(src_dir: &Path) -> Result<BackupManifest, String> {
    let text = fs::read_to_string(src_dir.join(MANIFEST_NAME))
        .map_err(|_| "不是有效的备份目录：缺少 manifest.json".to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("备份清单无效：{e}"))
}

/// 只读校验，不改目标文件，也不要求释放 sqlite 连接。
///
/// 不要求 `conversation_events` 存在：既有备份（索引落地之前）和本次起排除事件表的备份都能恢复。
pub fn validate_restore(src_dir: &Path) -> Result<BackupManifest, String> {
    let manifest = load_manifest(src_dir)?;
    if !src_dir.join(DB_NAME).exists() {
        return Err("备份目录缺少 usage.sqlite".to_string());
    }
    Ok(manifest)
}

/// 调用方必须先释放目标 sqlite 连接，否则 WAL 模式下无法安全覆盖。
/// 先把全部文件拷到 staging，再逐个 rename 进位；失败时从 `.restore-bak` 回滚。
pub fn restore_from(src_dir: &Path, paths: &AppDataPaths) -> Result<BackupManifest, String> {
    let manifest = validate_restore(src_dir)?;
    let dest_root = paths
        .db_path
        .parent()
        .ok_or_else(|| "数据库路径没有父目录".to_string())?
        .to_path_buf();
    let staging = dest_root.join(STAGING_DIR);
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| format!("创建恢复暂存目录失败：{e}"))?;

    let planned = match stage_restore(src_dir, paths, &staging) {
        Ok(planned) => planned,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };

    match apply_replacements(&planned, &paths.db_path) {
        Ok(()) => {
            for (dest, _) in &planned {
                let _ = fs::remove_file(bak_path(dest));
            }
            let _ = fs::remove_dir_all(&staging);
            Ok(manifest)
        }
        Err(error) => {
            rollback_replacements(&planned);
            let _ = fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}

fn stage_restore(
    src_dir: &Path,
    paths: &AppDataPaths,
    staging: &Path,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut planned = Vec::new();
    stage_required(src_dir, DB_NAME, &paths.db_path, staging, &mut planned)?;
    stage_optional(
        src_dir,
        PRICES_NAME,
        &paths.prices_path,
        staging,
        &mut planned,
    )?;
    stage_optional(
        src_dir,
        SNAPSHOT_NAME,
        &paths.snapshot_path,
        staging,
        &mut planned,
    )?;
    stage_optional(
        src_dir,
        BUDGET_NAME,
        &paths.budget_path,
        staging,
        &mut planned,
    )?;
    stage_optional(
        src_dir,
        BUDGET_NOTIFY_NAME,
        &paths.budget_notify_path,
        staging,
        &mut planned,
    )?;
    stage_optional(
        src_dir,
        OFFICIAL_QUOTA_NAME,
        &paths.official_quota_path,
        staging,
        &mut planned,
    )?;
    stage_optional(
        src_dir,
        OFFICIAL_QUOTA_NOTIFY_NAME,
        &paths.official_quota_notify_path,
        staging,
        &mut planned,
    )?;
    stage_optional(
        src_dir,
        CUSTOM_QUOTA_NAME,
        &sibling(&paths.official_quota_path, CUSTOM_QUOTA_NAME),
        staging,
        &mut planned,
    )?;
    stage_optional(
        src_dir,
        SCAN_PATHS_NAME,
        &sibling(&paths.budget_path, SCAN_PATHS_NAME),
        staging,
        &mut planned,
    )?;
    Ok(planned)
}

fn sibling(path: &Path, name: &str) -> PathBuf {
    path.with_file_name(name)
}

fn stage_required(
    src_dir: &Path,
    name: &str,
    dest: &Path,
    staging: &Path,
    planned: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), String> {
    let staged = staging.join(name);
    fs::copy(src_dir.join(name), &staged).map_err(|e| format!("暂存 {name} 失败：{e}"))?;
    planned.push((dest.to_path_buf(), staged));
    Ok(())
}

fn stage_optional(
    src_dir: &Path,
    name: &str,
    dest: &Path,
    staging: &Path,
    planned: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), String> {
    if !src_dir.join(name).exists() {
        return Ok(());
    }
    stage_required(src_dir, name, dest, staging, planned)
}

fn apply_replacements(planned: &[(PathBuf, PathBuf)], db_path: &Path) -> Result<(), String> {
    for (dest, staged) in planned {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bak = bak_path(dest);
        if dest.exists() {
            if dest.is_dir() {
                return Err(format!("写入 {} 失败：目标是目录", dest.display()));
            }
            replace_file(dest, &bak)
                .map_err(|e| format!("备份当前 {} 失败：{e}", dest.display()))?;
        }
        replace_file(staged, dest).map_err(|e| format!("写入 {} 失败：{e}", dest.display()))?;
    }
    remove_sidecar(db_path, "-wal");
    remove_sidecar(db_path, "-shm");
    Ok(())
}

fn rollback_replacements(planned: &[(PathBuf, PathBuf)]) {
    for (dest, _) in planned.iter().rev() {
        let bak = bak_path(dest);
        if bak.exists() {
            let _ = fs::remove_file(dest);
            let _ = replace_file(&bak, dest);
            let _ = fs::remove_file(&bak);
        }
    }
}

fn bak_path(dest: &Path) -> PathBuf {
    let mut name = dest
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    name.push_str(BAK_SUFFIX);
    dest.with_file_name(name)
}

fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(from, to).map_err(|e| e.to_string())?;
            let _ = fs::remove_file(from);
            Ok(())
        }
    }
}

fn remove_sidecar(db_path: &Path, suffix: &str) {
    let sidecar = PathBuf::from(format!("{}{suffix}", db_path.to_string_lossy()));
    let _ = fs::remove_file(sidecar);
}
