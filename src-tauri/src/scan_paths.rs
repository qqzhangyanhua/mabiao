//! 设置页扫描根目录覆盖。
//!
//! 语义与环境变量相同：值为「根目录」，适配器再按原规则拼接叶子路径。
//! 优先级：设置页 > 环境变量 > 默认路径。从 Dock 启动读不到 shell 环境变量时，
//! 靠这里的绝对路径避免漏扫。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adapters::usage_adapter;
use crate::domain::Source;
use crate::ingest::{self, PathOverrides};

pub const CONFIG_NAME: &str = "scan_paths.json";

pub fn config_path() -> PathBuf {
    crate::paths::app_data_dir().join(CONFIG_NAME)
}

/// source slug → 绝对根目录。缺 key 或空数组表示该来源不覆盖。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanPathConfig {
    #[serde(flatten)]
    pub overrides: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanPathPanelDto {
    pub rows: Vec<ScanPathRowDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanPathRowDto {
    pub source: String,
    pub application: String,
    pub env_var: String,
    pub override_roots: Vec<String>,
    pub env_roots: Vec<String>,
    pub default_roots: Vec<String>,
    pub effective_scan_dirs: Vec<String>,
    /// 适配器拼在根目录后的叶子。空表示根目录本身就是扫描目标。
    pub join_leaf: String,
    /// `ui` / `env` / `default`
    pub active: String,
    pub note: String,
}

pub fn load(path: &Path) -> ScanPathConfig {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, config: &ScanPathConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub fn load_overrides(path: &Path) -> PathOverrides {
    config_to_overrides(&load(path))
}

pub fn config_to_overrides(config: &ScanPathConfig) -> PathOverrides {
    let mut overrides = PathOverrides::new();
    for source in Source::ALL {
        let Some(roots) = config.overrides.get(source.as_str()) else {
            continue;
        };
        let paths: Vec<PathBuf> = roots
            .iter()
            .map(|root| root.trim())
            .filter(|root| !root.is_empty())
            .map(PathBuf::from)
            .collect();
        if !paths.is_empty() {
            overrides.insert(usage_adapter(source).path_env, paths);
        }
    }
    overrides
}

/// 规范化用户提交的覆盖表：展开 `~`、要求绝对路径、去空白与重复。
/// 未出现的来源视为清除覆盖。未知来源直接报错。
pub fn normalize(
    raw: BTreeMap<String, Vec<String>>,
    home: &Path,
) -> Result<ScanPathConfig, String> {
    let mut overrides = BTreeMap::new();
    for (source, roots) in raw {
        let Some(parsed) = Source::parse(&source) else {
            return Err(format!("未知来源：{source}"));
        };
        let mut paths = Vec::new();
        for root in roots {
            let trimmed = root.trim();
            if trimmed.is_empty() {
                continue;
            }
            let path = expand_home(trimmed, home);
            if !path.is_absolute() {
                return Err(format!(
                    "{} 的扫描路径必须是绝对路径：{trimmed}",
                    parsed.application_name()
                ));
            }
            let display = path.to_string_lossy().into_owned();
            if !paths.contains(&display) {
                paths.push(display);
            }
        }
        if !paths.is_empty() {
            overrides.insert(source, paths);
        }
    }
    Ok(ScanPathConfig { overrides })
}

pub fn expand_home(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    if let Some(rest) = raw.strip_prefix("~\\") {
        return home.join(rest);
    }
    PathBuf::from(raw)
}

pub fn panel(config_path: &Path, home: &Path) -> ScanPathPanelDto {
    let config = load(config_path);
    let env = ingest::env_overrides();
    let merged = ingest::merge_path_overrides(env.clone(), config_to_overrides(&config));
    let empty = PathOverrides::new();
    ScanPathPanelDto {
        rows: Source::ALL
            .iter()
            .copied()
            .map(|source| row(source, &config, &env, &merged, &empty, home))
            .collect(),
    }
}

fn row(
    source: Source,
    config: &ScanPathConfig,
    env: &PathOverrides,
    merged: &PathOverrides,
    empty: &PathOverrides,
    home: &Path,
) -> ScanPathRowDto {
    let adapter = usage_adapter(source);
    let override_roots = config
        .overrides
        .get(source.as_str())
        .cloned()
        .unwrap_or_default();
    let env_roots = env
        .get(adapter.path_env)
        .map(|paths| path_strings(paths))
        .unwrap_or_default();
    let join_leaf = join_leaf(source);
    let default_scan = (adapter.scan_dirs)(empty, home);
    let default_roots = default_scan
        .iter()
        .map(|dir| strip_leaf(dir, &join_leaf))
        .collect();
    let effective_scan_dirs = path_strings(&(adapter.scan_dirs)(merged, home));
    let active = if !override_roots.is_empty() {
        "ui"
    } else if !env_roots.is_empty() {
        "env"
    } else {
        "default"
    };
    ScanPathRowDto {
        source: source.as_str().to_string(),
        application: source.application_name().to_string(),
        env_var: adapter.path_env.to_string(),
        override_roots,
        env_roots,
        default_roots,
        effective_scan_dirs,
        join_leaf,
        active: active.to_string(),
        note: row_note(source),
    }
}

pub(crate) fn join_leaf(source: Source) -> String {
    let sentinel = PathBuf::from("/__mabiao_scan_root__");
    let env = usage_adapter(source).path_env;
    let overrides = PathOverrides::from([(env, vec![sentinel.clone()])]);
    let dirs = (usage_adapter(source).scan_dirs)(&overrides, Path::new("/home"));
    let Some(dir) = dirs.first() else {
        return String::new();
    };
    let Ok(rel) = dir.strip_prefix(&sentinel) else {
        return String::new();
    };
    rel.iter()
        .filter_map(|component| component.to_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn strip_leaf(scan: &Path, leaf: &str) -> String {
    if leaf.is_empty() {
        return scan.to_string_lossy().into_owned();
    }
    let mut current = scan.to_path_buf();
    for component in leaf.split('/').rev() {
        if current.file_name().and_then(|name| name.to_str()) == Some(component) {
            if let Some(parent) = current.parent() {
                current = parent.to_path_buf();
            }
        }
    }
    current.to_string_lossy().into_owned()
}

fn row_note(source: Source) -> String {
    match source {
        Source::CursorAgent => "只覆盖 token 包装目录，会话仍扫 ~/.cursor。".to_string(),
        _ => String::new(),
    }
}

fn path_strings(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}
