//! 数据源扫描路径配置、声明式规格与 [`ScanEnv`] 解析模块。
//!
//! 语义与环境变量相同：值为「根目录」，再按声明式规则拼接叶子路径与动态发现目录。
//! 优先级：设置页 > 环境变量 > 默认路径（Windows 未覆盖时包含单次缓存的 WSL 家目录）。

mod env;
mod spec;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::Source;
use crate::ingest::PathOverrides;

pub use env::ScanEnv;
pub use spec::{
    source_path_spec, source_path_specs, DefaultRoot, DefaultRoots, DisplayRule, DynamicExpansion,
    SourcePathSpec, SOURCE_PATH_SPECS,
};

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
    /// 声明式规格中拼在根目录后的首项叶子。空表示根目录本身就是扫描目标。
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
            overrides.insert(source_path_spec(source).path_env, paths);
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
    let env = ScanEnv::capture_with_config(home, config_path);
    panel_with_env(&env)
}

pub fn panel_with_env(env: &ScanEnv) -> ScanPathPanelDto {
    ScanPathPanelDto {
        rows: Source::ALL
            .iter()
            .copied()
            .map(|source| row(env, source))
            .collect(),
    }
}

fn row(env: &ScanEnv, source: Source) -> ScanPathRowDto {
    let spec = source_path_spec(source);
    let override_roots = path_strings(&env.ui_roots(source));
    let env_roots = path_strings(&env.env_display_roots(source));
    let default_roots = path_strings(&env.default_roots(source));
    let effective_scan_dirs = path_strings(&env.usage_scan_dirs(source));
    let active = env.active_layer(source);

    ScanPathRowDto {
        source: source.as_str().to_string(),
        application: source.application_name().to_string(),
        env_var: spec.path_env.to_string(),
        override_roots,
        env_roots,
        default_roots,
        effective_scan_dirs,
        join_leaf: spec.join_leaf().to_string(),
        active: active.to_string(),
        note: row_note(source),
    }
}

#[cfg(test)]
pub(crate) fn join_leaf(source: Source) -> &'static str {
    source_path_spec(source).join_leaf()
}

fn row_note(source: Source) -> String {
    match source {
        Source::Codex => "同级 archived_sessions 一并扫描。".to_string(),
        Source::Trae => "扫 cli/sessions 与同级 cli/archived_sessions。".to_string(),
        Source::CursorAgent => "只覆盖 token 包装目录，会话仍扫 ~/.cursor。".to_string(),
        Source::Claude => {
            "CLAUDE_CONFIG_DIR 只替换 Claude Code 根；已存在的 Desktop / Cowork 嵌套 projects 仍会追加。".to_string()
        }
        Source::Opencode => {
            "未设 OPENCODE_DATA_DIR 时用 XDG_DATA_HOME/opencode。默认扫 opencode.db 与 opencode-*.db；OPENCODE_DB 替换库文件名；没有库才读 storage/ 下的旧 JSON。".to_string()
        }
        Source::Goose => {
            "扫 sessions.db。GOOSE_PATH_ROOT 指向 goose 根，拼 data/sessions；未覆盖时扫 XDG / Application Support 与 Block 旧路径。".to_string()
        }
        Source::Roo => {
            "扫 VS Code / Cursor / Windsurf 等 globalStorage/rooveterinaryinc.roo-cline/tasks。ROO_TASKS_DIR 直接指向 tasks 目录。".to_string()
        }
        Source::Kilo => {
            "扫 VS Code / Cursor / Windsurf 等 globalStorage/kilocode.kilo-code/tasks。KILO_TASKS_DIR 直接指向 tasks 目录。".to_string()
        }
        Source::Amp => {
            "扫 threads/*.json。AMP_DATA_DIR 指向 Amp 数据根，拼 threads/；未覆盖时扫 ~/.local/share/amp/threads，以及 macOS Application Support 与 Windows AppData。credits 不是美元，不写入 native_cost。".to_string()
        }
        Source::Zed => {
            "扫 threads.db。ZED_DATA_DIR 指向 Zed 数据根，拼 threads/；未覆盖时扫 XDG、macOS Application Support 与 Windows LocalAppData。".to_string()
        }
        Source::Cherry => {
            "扫 cherrystudio.sqlite。CHERRY_STUDIO_DATA_DIR 覆盖 Electron userData 根；未覆盖时同时扫 CherryStudio 与 Cherry Studio。".to_string()
        }
        Source::OpenClaw => {
            "扫 agents/*/sessions 下 jsonl（含 .reset / .deleted 归档，跳过 checkpoint / bak / trajectory）。OPENCLAW_STATE_DIR 覆盖状态根并拼 agents；未覆盖时另扫 ~/.clawdbot。".to_string()
        }
        Source::CodeBuddyIde => {
            "扫 CodeBuddyExtension/Data 下 history/*/messages/*.json 的累计 statsSnapshot。CODEBUDDY_IDE_DIR 覆盖 Data 根。".to_string()
        }
        Source::Crush => {
            "读 projects.json 再打开各项目 crush.db。CRUSH_GLOBAL_DATA 覆盖全局数据目录；未覆盖时扫 ~/.local/share/crush 与 %LOCALAPPDATA%/crush。不遍历家目录下的 .crush。".to_string()
        }
        Source::Aider => {
            "只读 --analytics-log 的 JSONL。默认认 ~/.aider/analytics.jsonl；AIDER_ANALYTICS_LOG 可指向该文件或目录。不读、不写 .aider.conf.yml。".to_string()
        }
        Source::Warp => {
            "扫 warp.sqlite 的 agent_conversations。WARP_HOME 指向含该库的目录或库文件本身。默认扫 Linux ~/.local/state/warp-terminal、macOS Group Container Stable/Preview、Windows %LOCALAPPDATA%/warp/Warp/data。Token 只有按模型总量，没有 input/output 拆分。".to_string()
        }
        Source::Windsurf => {
            "扫 Windsurf / Devin Desktop 的 User/acp-events/*.ndjson。WINDSURF_ACP_HOME 指向该目录。只读 usage_update 的 cognition.ai token 字段；无模型名跳过。不读加密 Cascade .pb，不读 state.vscdb 额度%。".to_string()
        }
        _ => String::new(),
    }
}

fn path_strings(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// 默认扫描目录是否存在。只读探测，不读设置页覆盖、不写任何文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePresenceRoot {
    pub path: String,
    pub exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePresenceRow {
    pub source: String,
    pub application: String,
    pub roots: Vec<SourcePresenceRoot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePresenceDto {
    /// 本机 sqlite 里是否已有任何消耗记录（含归档）。有数据时总览不展示上手空态。
    pub has_usage_records: bool,
    pub rows: Vec<SourcePresenceRow>,
}

/// 用默认配置走声明式路径规格（含活跃 WSL 家目录），再问目录在不在。
pub fn detect_default_presence(home: &Path, has_usage_records: bool) -> SourcePresenceDto {
    let env = ScanEnv::capture_defaults_for_home(home);
    detect_default_presence_with_env(&env, has_usage_records)
}

pub fn detect_default_presence_with_env(
    env: &ScanEnv,
    has_usage_records: bool,
) -> SourcePresenceDto {
    let defaults = env.defaults_only();
    SourcePresenceDto {
        has_usage_records,
        rows: Source::ALL
            .iter()
            .copied()
            .map(|source| {
                let dirs = defaults.display_dirs(source);
                SourcePresenceRow {
                    source: source.as_str().to_string(),
                    application: source.application_name().to_string(),
                    roots: dirs
                        .into_iter()
                        .map(|path| SourcePresenceRoot {
                            exists: path.exists(),
                            path: path.to_string_lossy().into_owned(),
                        })
                        .collect(),
                }
            })
            .collect(),
    }
}
