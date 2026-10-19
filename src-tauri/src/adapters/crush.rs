use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Deserialize;

use crate::adapters::sqlite::{sql_int, sql_real, sql_text};
use crate::adapters::{finish, has_billable_tokens};

pub(crate) use crate::adapters::sqlite::table_columns;
use crate::domain::{Source, UsageRecord};
use crate::ingest::{self, PathOverrides};

/// Crush 把每个项目的会话写进 `{data_dir}/crush.db`。
///
/// 项目清单在全局 `projects.json`（charmbracelet/crush `internal/projects`）。
/// `$CRUSH_GLOBAL_DATA` 覆盖该全局数据目录；未覆盖时扫
/// `~/.local/share/crush` 与 Windows `%LOCALAPPDATA%/crush`。
/// 不遍历 `$HOME` 下的 `.crush` 目录。
pub(crate) fn scan_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    let registries = registry_dirs(overrides, home);
    let mut dirs = registries.clone();
    for root in &registries {
        let Ok(projects) = load_projects(root) else {
            continue;
        };
        for project in projects {
            let data_dir = project.data_dir.trim();
            if data_dir.is_empty() {
                continue;
            }
            let data_dir = PathBuf::from(data_dir);
            if !dirs.contains(&data_dir) {
                dirs.push(data_dir);
            }
        }
    }
    dirs
}

fn registry_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    overrides
        .get("CRUSH_GLOBAL_DATA")
        .cloned()
        .unwrap_or_else(|| {
            vec![
                home.join(".local/share/crush"),
                home.join("AppData/Local/crush"),
            ]
        })
}

#[derive(Deserialize)]
struct ProjectList {
    #[serde(default)]
    projects: Vec<TrackedProject>,
}

#[derive(Deserialize)]
struct TrackedProject {
    #[serde(default)]
    path: String,
    #[serde(default)]
    data_dir: String,
}

pub(crate) fn discover(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        // 覆盖值若直接指向带 crush.db 的 data_dir，没有 projects.json 也要收。
        push_crush_db(&mut paths, &root.join("crush.db"));
        if root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "crush.db")
        {
            push_crush_db(&mut paths, root);
        }
        for project in load_projects(root)? {
            let data_dir = project.data_dir.trim();
            if data_dir.is_empty() {
                continue;
            }
            push_crush_db(&mut paths, &PathBuf::from(data_dir).join("crush.db"));
        }
    }
    Ok(paths)
}

fn push_crush_db(paths: &mut Vec<PathBuf>, db: &Path) {
    if db.is_file() && !paths.contains(&db.to_path_buf()) {
        paths.push(db.to_path_buf());
    }
}

pub(crate) fn sidecar_fingerprint(path: &Path, _dirs: &[PathBuf]) -> String {
    ingest::wal_shm_fingerprint(path)
}

pub fn parse(path: &Path, scan_dir: &Path) -> Result<Vec<UsageRecord>, String> {
    let db = ingest::open_readonly_uri(path)?;
    let columns = table_columns(&db, "sessions")?;
    if !columns.contains("id") {
        return Ok(Vec::new());
    }
    let project = project_for_db(path, scan_dir);
    let sql = format!(
        "
        SELECT
            {id},
            {parent},
            {prompt},
            {completion},
            {cost},
            {created_at},
            {updated_at}
        FROM sessions
        ",
        id = sql_text(&columns, "id"),
        parent = sql_text(&columns, "parent_session_id"),
        prompt = sql_int(&columns, "prompt_tokens"),
        completion = sql_int(&columns, "completion_tokens"),
        cost = sql_real(&columns, "cost"),
        created_at = sql_int(&columns, "created_at"),
        updated_at = sql_int(&columns, "updated_at"),
    );
    let mut stmt = db.prepare(&sql).map_err(|error| error.to_string())?;
    let source_file = path.to_string_lossy().into_owned();
    let rows = stmt
        .query_map([], |row| {
            Ok(ParsedSession {
                session_id: row.get(0)?,
                parent_session_id: row.get(1)?,
                prompt_tokens: row.get(2)?,
                completion_tokens: row.get(3)?,
                cost: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let message_columns = table_columns(&db, "messages").unwrap_or_default();
    let mut records = Vec::new();
    for row in rows {
        let parsed = row.map_err(|error| error.to_string())?;
        if let Some(record) = parsed.into_record(&db, &message_columns, &source_file, &project) {
            records.push(record);
        }
    }
    Ok(records)
}

struct ParsedSession {
    session_id: String,
    parent_session_id: String,
    prompt_tokens: i64,
    completion_tokens: i64,
    cost: f64,
    created_at: i64,
    updated_at: i64,
}

impl ParsedSession {
    fn into_record(
        self,
        db: &Connection,
        message_columns: &BTreeSet<String>,
        source_file: &str,
        project: &str,
    ) -> Option<UsageRecord> {
        if !is_root_session(&self.parent_session_id) {
            return None;
        }
        let (model, provider) = session_model(db, message_columns, &self.session_id);
        if model.is_empty() {
            return None;
        }
        let occurred_at = unix_seconds_to_rfc3339(if self.updated_at > 0 {
            self.updated_at
        } else {
            self.created_at
        });
        let native_cost = (self.cost.is_finite() && self.cost > 0.0).then_some(self.cost);
        let record = finish(UsageRecord {
            occurred_at,
            source: Source::Crush,
            model,
            provider,
            project: project.to_string(),
            session_id: self.session_id,
            source_file: source_file.to_string(),
            input_tokens: self.prompt_tokens.max(0),
            output_tokens: self.completion_tokens.max(0),
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            cache_creation_1h_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            native_cost,
        });
        if !has_billable_tokens(&record) {
            None
        } else {
            Some(record)
        }
    }
}

pub(crate) fn is_root_session(parent_session_id: &str) -> bool {
    parent_session_id.trim().is_empty()
}

pub(crate) fn unix_seconds_to_rfc3339(started_at: i64) -> String {
    if started_at <= 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp(started_at, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

pub(crate) fn project_for_db(db_path: &Path, scan_dir: &Path) -> String {
    let data_dir = db_path.parent();
    if let Some(data_dir) = data_dir {
        if let Ok(projects) = load_projects(scan_dir) {
            for project in projects {
                if Path::new(project.data_dir.trim()) == data_dir {
                    let path = project.path.trim();
                    if !path.is_empty() {
                        return path.to_string();
                    }
                }
            }
        }
        if data_dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == ".crush")
        {
            if let Some(cwd) = data_dir.parent() {
                return cwd.to_string_lossy().into_owned();
            }
        }
        return data_dir.to_string_lossy().into_owned();
    }
    String::new()
}

fn load_projects(root: &Path) -> Result<Vec<TrackedProject>, String> {
    let path = root.join("projects.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    let list: ProjectList = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    Ok(list.projects)
}

fn session_model(
    db: &Connection,
    columns: &BTreeSet<String>,
    session_id: &str,
) -> (String, String) {
    if !columns.contains("session_id") {
        return (String::new(), String::new());
    }
    let model = sql_text(columns, "model");
    let provider = sql_text(columns, "provider");
    let role = sql_text(columns, "role");
    let order = if columns.contains("created_at") {
        "created_at DESC, id DESC"
    } else {
        "id DESC"
    };
    let sql = format!(
        "SELECT {model}, {provider}, {role}
         FROM messages
         WHERE session_id = ?1
         ORDER BY {order}"
    );
    let Ok(mut stmt) = db.prepare(&sql) else {
        return (String::new(), String::new());
    };
    let Ok(rows) = stmt.query_map([session_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) else {
        return (String::new(), String::new());
    };
    let mut fallback = None;
    for row in rows {
        let Ok((model, provider, role)) = row else {
            continue;
        };
        if model.trim().is_empty() {
            continue;
        }
        if role.trim().eq_ignore_ascii_case("assistant") {
            return (model, provider);
        }
        if fallback.is_none() {
            fallback = Some((model, provider));
        }
    }
    fallback.unwrap_or_default()
}
