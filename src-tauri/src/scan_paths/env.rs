//! 不可变的单次路径解析环境快照 [`ScanEnv`] 及各维度投影。
//!
//! 将主家目录、单次缓存的 WSL 额外家目录、设置页覆盖、环境变量覆盖与 OpenCode 副环境变量
//! 折叠为一份不可变快照，统一供应：
//! - Usage 消耗记录扫描目录与诊断展示目录
//! - 设置页扫描路径面板（`default_roots`、`effective_scan_dirs`、`active`）
//! - 对话记录目录发现根与路径白名单信任根
//! - Cursor 会话 transcript 扫描根与代码追踪库路径
//! - 官方额度客户端默认配置根目录与 Claude statusline hook 设置路径

use std::path::{Path, PathBuf};

use crate::domain::Source;
use crate::ingest::PathOverrides;

use super::spec::{
    claude_desktop_project_dirs, expand_crush_projects, expand_hermes_profiles, push_unique,
    resolve_opencode_dbs, source_path_spec, DisplayRule, DynamicExpansion,
};
use super::{load, ScanPathConfig};

/// 不可变的单次路径解析环境快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanEnv {
    home: PathBuf,
    wsl_homes: Vec<PathBuf>,
    ui_config: ScanPathConfig,
    env_overrides: PathOverrides,
    xdg_data_home: Option<PathBuf>,
    opencode_db: Option<PathBuf>,
}

impl ScanEnv {
    /// 构造纯内存快照：无 WSL 额外家目录、无设置页覆盖、无环境变量覆盖。
    /// 适用于并行单元测试与隔离环境。
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            wsl_homes: Vec::new(),
            ui_config: ScanPathConfig::default(),
            env_overrides: PathOverrides::new(),
            xdg_data_home: None,
            opencode_db: None,
        }
    }

    /// 捕获当前进程环境的快照（单次发现 WSL 家目录）。
    /// 测试模式下不读宿主真实 `scan_paths.json`，只读环境变量与注入的 WSL 家目录。
    pub fn capture_for_home(home: &Path) -> Self {
        #[cfg(test)]
        {
            Self::capture_parts(
                home,
                ScanPathConfig::default(),
                crate::wsl::extra_scan_homes,
            )
        }
        #[cfg(not(test))]
        {
            Self::capture_with_config(home, &super::config_path())
        }
    }

    /// 捕获包含指定 `scan_paths.json` 配置文件与当前进程环境变量的快照（单次发现 WSL 家目录）。
    pub fn capture_with_config(home: &Path, config_path: &Path) -> Self {
        Self::capture_parts(home, load(config_path), crate::wsl::extra_scan_homes)
    }

    /// 捕获快照但由调用方注入 WSL 发现函数（用于单次发现测试与隔离环境）。
    pub fn capture_with_wsl(
        home: &Path,
        config_path: &Path,
        discover_wsl: impl FnOnce() -> Vec<PathBuf>,
    ) -> Self {
        Self::capture_parts(home, load(config_path), discover_wsl)
    }

    /// 只捕获主家目录与单次 WSL 家目录，刻意忽略所有设置页与环境变量覆盖（用于总览默认存在性探测）。
    pub fn capture_defaults_for_home(home: &Path) -> Self {
        Self::new(home).with_wsl_homes(crate::wsl::extra_scan_homes())
    }

    /// 兼容旧 `PathOverrides` 覆盖表（测试与既有过渡入口用）：单次捕获当前 WSL 注入列表，
    /// 并把 `OPENCODE_DB` 拆到专有字段，保证与 `ScanEnv` 语义完全一致。
    #[cfg(test)]
    pub(crate) fn from_overrides(home: &Path, overrides: &PathOverrides) -> Self {
        let mut env_overrides = overrides.clone();
        let opencode_db = env_overrides
            .remove("OPENCODE_DB")
            .and_then(|paths| paths.into_iter().next())
            .and_then(normalize_opencode_db);
        let mut env = Self::new(home)
            .with_wsl_homes(crate::wsl::extra_scan_homes())
            .with_env_overrides(env_overrides);
        env.opencode_db = opencode_db;
        env
    }

    fn capture_parts(
        home: &Path,
        ui_config: ScanPathConfig,
        discover_wsl: impl FnOnce() -> Vec<PathBuf>,
    ) -> Self {
        let mut env = Self::new(home)
            .with_wsl_homes(discover_wsl())
            .with_ui_config(ui_config)
            .with_env_overrides(read_process_env_overrides());
        if let Some(xdg) = trimmed_env("XDG_DATA_HOME") {
            env.xdg_data_home = Some(PathBuf::from(xdg));
        }
        if let Some(db) = trimmed_env("OPENCODE_DB")
            .map(PathBuf::from)
            .and_then(normalize_opencode_db)
        {
            env.opencode_db = Some(db);
        }
        env
    }

    pub fn with_wsl_homes(mut self, homes: impl IntoIterator<Item = PathBuf>) -> Self {
        self.wsl_homes.clear();
        for extra in homes {
            if extra != self.home && !self.wsl_homes.contains(&extra) {
                self.wsl_homes.push(extra);
            }
        }
        self
    }

    pub fn with_ui_config(mut self, config: ScanPathConfig) -> Self {
        self.ui_config = config;
        self
    }

    pub fn with_env_overrides(mut self, overrides: PathOverrides) -> Self {
        self.env_overrides = overrides;
        self
    }

    pub fn with_env_override(mut self, var: &'static str, roots: Vec<PathBuf>) -> Self {
        if roots.is_empty() {
            self.env_overrides.remove(var);
        } else {
            self.env_overrides.insert(var, roots);
        }
        self
    }

    pub fn with_xdg_data_home(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        if path.as_os_str().is_empty() {
            self.xdg_data_home = None;
        } else {
            self.xdg_data_home = Some(path);
        }
        self
    }

    pub fn with_opencode_db(mut self, db: impl Into<PathBuf>) -> Self {
        self.opencode_db = normalize_opencode_db(db.into());
        self
    }

    /// 保留主家目录与已缓存的 WSL 家目录，清除所有 UI 与环境变量覆盖。
    pub fn defaults_only(&self) -> Self {
        Self {
            home: self.home.clone(),
            wsl_homes: self.wsl_homes.clone(),
            ui_config: ScanPathConfig::default(),
            env_overrides: PathOverrides::new(),
            xdg_data_home: None,
            opencode_db: None,
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn wsl_homes(&self) -> &[PathBuf] {
        &self.wsl_homes
    }

    /// 主家目录加上已缓存的 WSL 额外家目录（去重，主家目录在首）。
    pub fn scan_homes(&self) -> Vec<PathBuf> {
        let mut homes = Vec::with_capacity(1 + self.wsl_homes.len());
        homes.push(self.home.clone());
        for extra in &self.wsl_homes {
            push_unique(&mut homes, extra.clone());
        }
        homes
    }

    /// 设置页该来源配置的 UI 覆盖根目录（已 trim 去空）。
    pub(crate) fn ui_roots(&self, source: Source) -> Vec<PathBuf> {
        let Some(roots) = self.ui_config.overrides.get(source.as_str()) else {
            return Vec::new();
        };
        let mut paths = Vec::new();
        for raw in roots {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                push_unique(&mut paths, PathBuf::from(trimmed));
            }
        }
        paths
    }

    /// 该来源的数据根环境变量覆盖（含 OpenCode 的 `XDG_DATA_HOME/opencode` 回退）。
    pub(crate) fn env_data_roots(&self, source: Source) -> Vec<PathBuf> {
        let spec = source_path_spec(source);
        if let Some(paths) = self.env_overrides.get(spec.path_env) {
            let mut out = Vec::new();
            for path in paths {
                if !path.as_os_str().is_empty() {
                    push_unique(&mut out, path.clone());
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
        if source == Source::Opencode {
            if let Some(xdg) = &self.xdg_data_home {
                return vec![xdg.join("opencode")];
            }
        }
        Vec::new()
    }

    /// 供设置页展示的环境变量覆盖项（OpenCode 仅设 `OPENCODE_DB` 时展示库名/路径）。
    pub(crate) fn env_display_roots(&self, source: Source) -> Vec<PathBuf> {
        let roots = self.env_data_roots(source);
        if !roots.is_empty() {
            return roots;
        }
        if source == Source::Opencode {
            if let Some(db) = &self.opencode_db {
                return vec![db.clone()];
            }
        }
        Vec::new()
    }

    /// 设置页当前生效层：`"ui"` / `"env"` / `"default"`。
    pub(crate) fn active_layer(&self, source: Source) -> &'static str {
        if !self.ui_roots(source).is_empty() {
            "ui"
        } else if !self.env_data_roots(source).is_empty()
            || (source == Source::Opencode && self.opencode_db.is_some())
        {
            "env"
        } else {
            "default"
        }
    }

    /// 干净的默认根目录列表（跨主家目录 + 活跃 WSL 家目录，不含叶子、归档配对或动态磁盘展开）。
    pub fn default_roots(&self, source: Source) -> Vec<PathBuf> {
        let spec = source_path_spec(source);
        let mut roots = Vec::new();
        for scan_home in self.scan_homes() {
            for root in spec.default_roots_for_home(&scan_home) {
                push_unique(&mut roots, root);
            }
        }
        roots
    }

    /// 生效的基础根目录列表（未拼接叶子、未做磁盘动态扩展）。
    ///
    /// 优先级：设置页 UI 覆盖 > 环境变量（含 `XDG_DATA_HOME/opencode`） > 默认根目录（含 WSL）。
    /// OpenCode 若仅设置了 `OPENCODE_DB`，仍压制 WSL 回退，仅使用主家目录默认根。
    pub fn effective_roots(&self, source: Source) -> Vec<PathBuf> {
        let ui = self.ui_roots(source);
        if !ui.is_empty() {
            return ui;
        }
        let env = self.env_data_roots(source);
        if !env.is_empty() {
            return env;
        }
        let spec = source_path_spec(source);
        if source == Source::Opencode && self.opencode_db.is_some() {
            return spec.default_roots_for_home(&self.home);
        }
        self.default_roots(source)
    }

    /// Usage 消耗记录实际要扫描的目录/库文件列表。
    pub fn usage_scan_dirs(&self, source: Source) -> Vec<PathBuf> {
        let spec = source_path_spec(source);
        let overridden_roots = {
            let ui = self.ui_roots(source);
            if !ui.is_empty() {
                Some(ui)
            } else {
                let env = self.env_data_roots(source);
                (!env.is_empty()).then_some(env)
            }
        };

        match spec.dynamic {
            DynamicExpansion::OpencodeDb => {
                let base_roots = match overridden_roots {
                    Some(roots) => roots,
                    None if self.opencode_db.is_some() => spec.default_roots_for_home(&self.home),
                    None => self.default_roots(source),
                };
                resolve_opencode_dbs(&base_roots, self.opencode_db.as_deref())
            }
            DynamicExpansion::ClaudeDesktopCowork => {
                let mut dirs = Vec::new();
                if let Some(roots) = overridden_roots {
                    for dir in spec.apply_leaves_to_roots(&roots) {
                        push_unique(&mut dirs, dir);
                    }
                    // CLAUDE_CONFIG_DIR 只替换 Code 根，不关闭主家目录的 Desktop / Cowork 发现。
                    for extra in claude_desktop_project_dirs(&self.home) {
                        push_unique(&mut dirs, extra);
                    }
                } else {
                    for scan_home in self.scan_homes() {
                        for dir in spec.default_static_scan_dirs_for_home(&scan_home) {
                            push_unique(&mut dirs, dir);
                        }
                        for extra in claude_desktop_project_dirs(&scan_home) {
                            push_unique(&mut dirs, extra);
                        }
                    }
                }
                dirs
            }
            DynamicExpansion::HermesProfiles => {
                let base_roots = overridden_roots.unwrap_or_else(|| self.default_roots(source));
                expand_hermes_profiles(base_roots)
            }
            DynamicExpansion::CrushProjectsJson => {
                let base_roots = overridden_roots.unwrap_or_else(|| self.default_roots(source));
                expand_crush_projects(base_roots)
            }
            DynamicExpansion::None => {
                if let Some(roots) = overridden_roots {
                    spec.apply_leaves_to_roots(&roots)
                } else {
                    let mut dirs = Vec::new();
                    for scan_home in self.scan_homes() {
                        for dir in spec.default_static_scan_dirs_for_home(&scan_home) {
                            push_unique(&mut dirs, dir);
                        }
                    }
                    dirs
                }
            }
        }
    }

    /// 来源诊断与默认存在性检查展示的目录列表。
    pub fn display_dirs(&self, source: Source) -> Vec<PathBuf> {
        let spec = source_path_spec(source);
        match spec.display {
            DisplayRule::SameAsUsage => self.usage_scan_dirs(source),
            DisplayRule::CursorAgentSharedPlusExistingUsage => {
                let mut dirs = Vec::new();
                for scan_home in self.scan_homes() {
                    push_unique(&mut dirs, scan_home.join(".cursor/chats"));
                    push_unique(&mut dirs, scan_home.join(".cursor/projects"));
                }
                for dir in self.usage_scan_dirs(source) {
                    if dir.exists() {
                        push_unique(&mut dirs, dir);
                    }
                }
                dirs
            }
        }
    }

    /// 对话记录目录刷新使用的扫描根。
    /// CursorAgent 始终扫描主家目录与 WSL 家目录的 `.cursor/projects`，不受 `CURSOR_AGENT_USAGE_DIR` 影响。
    pub fn conversation_catalog_roots(&self, source: Source) -> Vec<PathBuf> {
        if source == Source::CursorAgent {
            self.cursor_session_roots()
        } else {
            self.usage_scan_dirs(source)
        }
    }

    /// 对话记录详情 / 补建 / 导出使用的路径白名单信任根。
    /// OpenCode 额外放行库文件的非根父目录，以支持无库时读取 `storage/` 下的旧 JSON。
    pub fn conversation_trust_roots(&self, source: Source) -> Vec<PathBuf> {
        let roots = self.conversation_catalog_roots(source);
        if source == Source::Opencode {
            opencode_trust_roots(&roots)
        } else {
            roots
        }
    }

    /// Cursor 会话行为统计的 transcript 扫描根（跨主家目录 + 活跃 WSL 家目录）。
    pub fn cursor_session_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for scan_home in self.scan_homes() {
            push_unique(&mut roots, scan_home.join(".cursor/projects"));
        }
        roots
    }

    /// Cursor IDE 写在本机主家目录的 `ai-code-tracking.db` 路径。
    pub fn cursor_tracking_db_path(&self) -> PathBuf {
        self.home.join(".cursor/ai-tracking/ai-code-tracking.db")
    }

    /// 官方额度客户端默认配置根目录候选（按 UI > env > 默认含 WSL 顺序）。
    /// Claude 额外在末尾附带默认 `[home/.claude, home/.config/claude]` 回退，
    /// 避免仅把 `CLAUDE_CONFIG_DIR` 指到会话目录时找不到默认登录凭证。
    pub fn client_config_roots(&self, source: Source) -> Vec<PathBuf> {
        let mut roots = self.effective_roots(source);
        if source == Source::Claude {
            for fallback in [self.home.join(".claude"), self.home.join(".config/claude")] {
                push_unique(&mut roots, fallback);
            }
        }
        roots
    }

    /// 某来源的首要配置根目录（UI 首项 > env 首项 > 默认首项）。
    pub fn primary_client_config_root(&self, source: Source) -> PathBuf {
        self.effective_roots(source)
            .into_iter()
            .next()
            .unwrap_or_else(|| self.home.clone())
    }

    /// 选出第一个包含指定凭证文件的配置根目录；都不存在时回落到首选配置根目录。
    pub fn client_credential_root(&self, source: Source, filename: &str) -> PathBuf {
        self.client_config_roots(source)
            .into_iter()
            .find(|root| root.join(filename).is_file())
            .unwrap_or_else(|| self.primary_client_config_root(source))
    }

    /// 选出第一个存在的客户端凭证文件路径；都不存在时回落到首选配置根目录下的目标文件。
    pub fn client_credential_path(&self, source: Source, filename: &str) -> PathBuf {
        self.client_credential_root(source, filename).join(filename)
    }

    /// Claude statusline hook 默认检查的 `settings.json` 路径（首选配置根）。
    pub fn claude_hook_settings_path(&self) -> PathBuf {
        self.primary_client_config_root(Source::Claude)
            .join("settings.json")
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

/// 从当前进程环境变量收集全部 Source 的主路径覆盖（逗号分隔、去空白）。
pub(crate) fn read_process_env_overrides() -> PathOverrides {
    Source::ALL
        .iter()
        .copied()
        .filter_map(|source| {
            let var = source_path_spec(source).path_env;
            read_comma_env(var).map(|paths| (var, paths))
        })
        .collect()
}

fn read_comma_env(var: &str) -> Option<Vec<PathBuf>> {
    let raw = std::env::var(var).ok()?;
    let paths: Vec<PathBuf> = raw
        .split(',')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(PathBuf::from)
        .collect();
    (!paths.is_empty()).then_some(paths)
}

fn trimmed_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_opencode_db(path: PathBuf) -> Option<PathBuf> {
    if let Some(text) = path.to_str() {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed == ":memory:" {
            return None;
        }
        return Some(PathBuf::from(trimmed));
    }
    (!path.as_os_str().is_empty()).then_some(path)
}
