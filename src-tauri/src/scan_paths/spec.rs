//! 35 个 Usage Source 的声明式路径规格（ADR 0005）。
//!
//! 取代原本分散在各适配器的不透明 `scan_dirs` 闭包、设置页哨兵路径探测与倒推剥离逻辑。
//! 每个 Source 的默认根、覆盖子路径、配对归档目录、磁盘动态扩展与展示规则都集中声明于此。

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::Source;

/// 单条相对 `home` 的默认根条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultRoot {
    /// 展示在设置页 `default_roots` 里的默认根目录（相对 home）。
    pub root: &'static str,
    /// 未覆盖时的实际扫描路径（相对 home）。`None` 表示用 `spec.leaves` 从 `root` 派生。
    pub default_scan: Option<&'static str>,
}

impl DefaultRoot {
    const fn simple(root: &'static str) -> Self {
        Self {
            root,
            default_scan: None,
        }
    }

    const fn with_scan(root: &'static str, default_scan: &'static str) -> Self {
        Self {
            root,
            default_scan: Some(default_scan),
        }
    }
}

/// 某 Source 的默认根目录集合形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultRoots {
    Static(&'static [DefaultRoot]),
    /// VS Code 家族 + 远程 server 的 `User/globalStorage/<extension_id>/tasks`。
    VsCodeExtensionTasks {
        extension_id: &'static str,
    },
}

/// 需要在磁盘上动态发现或展开的路径规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicExpansion {
    None,
    /// Claude Desktop / Cowork 嵌套 `local-agent-mode-sessions/*/*/local_*/.claude/projects`。
    /// 始终追加在主 Code 根之后，即使 `CLAUDE_CONFIG_DIR` 覆盖了主根也不关闭。
    ClaudeDesktopCowork,
    /// OpenCode 发现 `opencode.db` / `opencode-*.db` 或解析 `OPENCODE_DB`（含缺省绝对路径回退）。
    OpencodeDb,
    /// Hermes 将指向 `profiles/<name>` 的根归一到父根，并动态展开非隐藏子 profile 目录。
    HermesProfiles,
    /// Crush 保留注册表目录并追加 `<registry>/projects.json` 里的项目 `data_dir`。
    CrushProjectsJson,
}

/// 来源诊断与存在性检查使用的展示目录规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayRule {
    /// 与 Usage 扫描目录一致。
    SameAsUsage,
    /// CursorAgent：始终展示跨全部 scan_homes 的 `~/.cursor/{chats,projects}`，再追加已存在的包装目录。
    CursorAgentSharedPlusExistingUsage,
}

/// 单个 Usage Source 的完整声明式路径规格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePathSpec {
    pub source: Source,
    /// 整体覆盖默认扫描根的主环境变量名（每个 Source 唯一）。
    pub path_env: &'static str,
    pub defaults: DefaultRoots,
    /// 拼在根目录后的子路径列表。空表示根目录本身就是扫描目标。
    /// 首项即设置页展示的 `join_leaf`；多项（Codex / Trae）表示同一根同时展开配对目录。
    pub leaves: &'static [&'static str],
    pub dynamic: DynamicExpansion,
    pub display: DisplayRule,
}

impl SourcePathSpec {
    /// 设置页展示给用户的「根目录后拼接的叶子」。空字符串表示根目录即扫描目标。
    pub const fn join_leaf(&self) -> &'static str {
        match self.leaves {
            [first, ..] => first,
            [] => "",
        }
    }

    /// 针对单个 `home` 列出干净的默认根目录（不含叶子、不含归档配对、不含磁盘动态展开）。
    pub fn default_roots_for_home(&self, home: &Path) -> Vec<PathBuf> {
        match self.defaults {
            DefaultRoots::Static(entries) => {
                entries.iter().map(|entry| home.join(entry.root)).collect()
            }
            DefaultRoots::VsCodeExtensionTasks { extension_id } => {
                vscode_extension_task_dirs(home, extension_id)
            }
        }
    }

    /// 针对单个 `home` 列出未覆盖时的静态扫描目录（未做 `dynamic` 磁盘扩展）。
    pub(crate) fn default_static_scan_dirs_for_home(&self, home: &Path) -> Vec<PathBuf> {
        match self.defaults {
            DefaultRoots::Static(entries) => {
                let mut dirs = Vec::new();
                for entry in entries {
                    if let Some(scan) = entry.default_scan {
                        push_unique(&mut dirs, home.join(scan));
                    } else {
                        let root = home.join(entry.root);
                        append_leaves(&mut dirs, &root, self.leaves);
                    }
                }
                dirs
            }
            DefaultRoots::VsCodeExtensionTasks { extension_id } => {
                vscode_extension_task_dirs(home, extension_id)
            }
        }
    }

    /// 对覆盖根目录拼接声明的 `leaves`。
    pub(crate) fn apply_leaves_to_roots(&self, roots: &[PathBuf]) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        for root in roots {
            append_leaves(&mut dirs, root, self.leaves);
        }
        dirs
    }
}

fn append_leaves(out: &mut Vec<PathBuf>, root: &Path, leaves: &[&str]) {
    if leaves.is_empty() {
        push_unique(out, root.to_path_buf());
    } else {
        for leaf in leaves {
            push_unique(out, root.join(leaf));
        }
    }
}

pub(crate) fn push_unique(out: &mut Vec<PathBuf>, path: PathBuf) {
    if !out.contains(&path) {
        out.push(path);
    }
}

const EDITOR_APPS: &[&str] = &[
    "Code",
    "Code - Insiders",
    "Code - OSS",
    "VSCodium",
    "VSCodium - Insiders",
    "Cursor",
    "Windsurf",
    "Trae",
    "Trae CN",
];

const SERVER_ROOTS: &[&str] = &[
    ".vscode-server/data",
    ".vscode-insiders-server/data",
    ".cursor-server/data",
];

fn vscode_extension_task_dirs(home: &Path, extension_id: &str) -> Vec<PathBuf> {
    let leaf = format!("User/globalStorage/{extension_id}/tasks");
    let mut dirs = Vec::new();
    for app in EDITOR_APPS {
        for root in [
            home.join(".config").join(app),
            home.join("Library/Application Support").join(app),
            home.join("AppData/Roaming").join(app),
        ] {
            dirs.push(root.join(&leaf));
        }
    }
    for root in SERVER_ROOTS {
        dirs.push(home.join(root).join(&leaf));
    }
    dirs
}

/// 全部 35 个 Usage Source 的声明式路径规格表。
pub const SOURCE_PATH_SPECS: &[SourcePathSpec] = &[
    SourcePathSpec {
        source: Source::Codex,
        path_env: "CODEX_HOME",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".codex")]),
        leaves: &["sessions", "archived_sessions"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Claude,
        path_env: "CLAUDE_CONFIG_DIR",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".claude"),
            DefaultRoot::simple(".config/claude"),
        ]),
        leaves: &["projects"],
        dynamic: DynamicExpansion::ClaudeDesktopCowork,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Pi,
        path_env: "PI_AGENT_DIR",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".pi/agent/sessions")]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Omp,
        path_env: "OMP_AGENT_DIR",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".omp/agent/sessions")]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Kimi,
        path_env: "KIMI_DATA_DIR",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".kimi"),
            DefaultRoot::simple(".kimi-code"),
        ]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::CursorAgent,
        path_env: "CURSOR_AGENT_USAGE_DIR",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".cursor-agent-usage")]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::CursorAgentSharedPlusExistingUsage,
    },
    SourcePathSpec {
        source: Source::Copilot,
        path_env: "COPILOT_HOME",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".copilot")]),
        leaves: &["session-state"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Dsh,
        path_env: "DSH_HOME",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".dsh")]),
        leaves: &["sessions"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Gemini,
        path_env: "GEMINI_DATA_DIR",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".gemini/tmp")]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Qwen,
        path_env: "QWEN_DATA_DIR",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".qwen")]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Factory,
        path_env: "FACTORY_SESSIONS_DIR",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".factory/sessions")]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Grok,
        path_env: "GROK_HOME",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".grok")]),
        leaves: &["sessions"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Opencode,
        path_env: "OPENCODE_DATA_DIR",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".local/share/opencode")]),
        leaves: &["opencode.db"],
        dynamic: DynamicExpansion::OpencodeDb,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Hermes,
        path_env: "HERMES_HOME",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".hermes")]),
        leaves: &[],
        dynamic: DynamicExpansion::HermesProfiles,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Agy,
        path_env: "AGY_DATA_DIR",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".gemini/antigravity-cli"),
            DefaultRoot::simple(".gemini/antigravity-ide"),
        ]),
        leaves: &["conversations"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Qoder,
        path_env: "QODER_CONFIG_DIR",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".qoder")]),
        leaves: &["projects"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::QoderCn,
        path_env: "QODERCN_CONFIG_DIR",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".qoder-cn")]),
        leaves: &["projects"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Cline,
        path_env: "CLINE_SESSION_DATA_DIR",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".cline/data/sessions")]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::WorkBuddy,
        path_env: "WORKBUDDY_CONFIG_DIR",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".workbuddy"),
            DefaultRoot::simple(".workbuddy-ai"),
        ]),
        leaves: &["projects"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Zcode,
        path_env: "ZCODE_HOME",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".zcode")]),
        leaves: &["cli/db/db.sqlite"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Alma,
        path_env: "ALMA_HOME",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".config/alma"),
            DefaultRoot::simple("Library/Application Support/alma"),
            DefaultRoot::simple("AppData/Roaming/alma"),
        ]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::CodeBuddy,
        path_env: "CODEBUDDY_DIR",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".codebuddy")]),
        leaves: &["projects"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::CodeBuddyIde,
        path_env: "CODEBUDDY_IDE_DIR",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".config/CodeBuddyExtension/Data"),
            DefaultRoot::simple("Library/Application Support/CodeBuddyExtension/Data"),
            DefaultRoot::simple("AppData/Roaming/CodeBuddyExtension/Data"),
            DefaultRoot::simple("AppData/Local/CodeBuddyExtension/Data"),
        ]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Trae,
        path_env: "TRAE_HOME",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".trae")]),
        leaves: &["cli/sessions", "cli/archived_sessions"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Goose,
        path_env: "GOOSE_PATH_ROOT",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::with_scan(".local/share/goose", ".local/share/goose/sessions"),
            DefaultRoot::with_scan(
                "Library/Application Support/goose",
                "Library/Application Support/goose/sessions",
            ),
            DefaultRoot::with_scan(
                "Library/Application Support/Block/goose",
                "Library/Application Support/Block/goose/sessions",
            ),
            DefaultRoot::with_scan(
                ".local/share/Block/goose",
                ".local/share/Block/goose/sessions",
            ),
            DefaultRoot::with_scan(
                "AppData/Roaming/Block/goose",
                "AppData/Roaming/Block/goose/data/sessions",
            ),
        ]),
        leaves: &["data/sessions"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Roo,
        path_env: "ROO_TASKS_DIR",
        defaults: DefaultRoots::VsCodeExtensionTasks {
            extension_id: "rooveterinaryinc.roo-cline",
        },
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Kilo,
        path_env: "KILO_TASKS_DIR",
        defaults: DefaultRoots::VsCodeExtensionTasks {
            extension_id: "kilocode.kilo-code",
        },
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Amp,
        path_env: "AMP_DATA_DIR",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".local/share/amp"),
            DefaultRoot::simple("Library/Application Support/amp"),
            DefaultRoot::simple("AppData/Roaming/amp"),
        ]),
        leaves: &["threads"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Zed,
        path_env: "ZED_DATA_DIR",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".local/share/zed"),
            DefaultRoot::simple("Library/Application Support/Zed"),
            DefaultRoot::simple("AppData/Local/Zed"),
        ]),
        leaves: &["threads"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Cherry,
        path_env: "CHERRY_STUDIO_DATA_DIR",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".config/CherryStudio"),
            DefaultRoot::simple("Library/Application Support/CherryStudio"),
            DefaultRoot::simple("AppData/Roaming/CherryStudio"),
            DefaultRoot::simple(".config/Cherry Studio"),
            DefaultRoot::simple("Library/Application Support/Cherry Studio"),
            DefaultRoot::simple("AppData/Roaming/Cherry Studio"),
        ]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::OpenClaw,
        path_env: "OPENCLAW_STATE_DIR",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".openclaw"),
            DefaultRoot::simple(".clawdbot"),
        ]),
        leaves: &["agents"],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Crush,
        path_env: "CRUSH_GLOBAL_DATA",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".local/share/crush"),
            DefaultRoot::simple("AppData/Local/crush"),
        ]),
        leaves: &[],
        dynamic: DynamicExpansion::CrushProjectsJson,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Aider,
        path_env: "AIDER_ANALYTICS_LOG",
        defaults: DefaultRoots::Static(&[DefaultRoot::simple(".aider")]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Warp,
        path_env: "WARP_HOME",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".local/state/warp-terminal"),
            DefaultRoot::simple(
                "Library/Group Containers/2BBY89MBSN.dev.warp/Library/Application Support/dev.warp.Warp-Stable",
            ),
            DefaultRoot::simple(
                "Library/Group Containers/2BBY89MBSN.dev.warp/Library/Application Support/dev.warp.Warp-Preview",
            ),
            DefaultRoot::simple("AppData/Local/warp/Warp/data"),
        ]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
    SourcePathSpec {
        source: Source::Windsurf,
        path_env: "WINDSURF_ACP_HOME",
        defaults: DefaultRoots::Static(&[
            DefaultRoot::simple(".config/Devin/User/acp-events"),
            DefaultRoot::simple("Library/Application Support/Devin/User/acp-events"),
            DefaultRoot::simple("AppData/Roaming/Devin/User/acp-events"),
            DefaultRoot::simple(".config/Windsurf/User/acp-events"),
            DefaultRoot::simple("Library/Application Support/Windsurf/User/acp-events"),
            DefaultRoot::simple("AppData/Roaming/Windsurf/User/acp-events"),
        ]),
        leaves: &[],
        dynamic: DynamicExpansion::None,
        display: DisplayRule::SameAsUsage,
    },
];

pub fn source_path_spec(source: Source) -> &'static SourcePathSpec {
    SOURCE_PATH_SPECS
        .iter()
        .find(|spec| spec.source == source)
        .expect("SOURCE_PATH_SPECS 必须覆盖 Source::ALL 的每个变体")
}

pub fn source_path_specs() -> &'static [SourcePathSpec] {
    SOURCE_PATH_SPECS
}

/// 各平台 Claude Desktop / Cowork 数据根下已存在的嵌套 `.claude/projects`。
pub(crate) fn claude_desktop_project_dirs(home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for root in claude_desktop_data_roots(home) {
        collect_nested_claude_projects(&root, &mut found, &mut seen);
    }
    found
}

fn claude_desktop_data_roots(home: &Path) -> Vec<PathBuf> {
    let mut roots = vec![
        home.join("Library/Application Support/Claude"),
        home.join("Library/Application Support/Claude-3p"),
        home.join(".config/Claude"),
        home.join(".config/Claude-3p"),
        home.join("AppData/Roaming/Claude"),
        home.join("AppData/Local/Claude"),
        home.join("AppData/Local/Claude-3p"),
    ];
    let packages = home.join("AppData/Local/Packages");
    if is_real_dir(&packages) {
        if let Ok(entries) = fs::read_dir(&packages) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                if !name.starts_with("Claude_") {
                    continue;
                }
                let cache = entry.path().join("LocalCache");
                roots.push(cache.join("Roaming/Claude"));
                roots.push(cache.join("Local/Claude"));
                roots.push(cache.join("Local/Claude-3p"));
            }
        }
    }
    roots
}

fn collect_nested_claude_projects(
    root: &Path,
    found: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    let sessions = root.join("local-agent-mode-sessions");
    if !is_real_dir(&sessions) {
        return;
    }
    for account in real_child_dirs(&sessions) {
        if account.file_name().and_then(|name| name.to_str()) == Some("skills-plugin") {
            continue;
        }
        for org in real_child_dirs(&account) {
            for local in real_child_dirs(&org) {
                let Some(name) = local.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !name.starts_with("local_") {
                    continue;
                }
                let projects = local.join(".claude/projects");
                if is_real_dir(&projects) && seen.insert(projects.clone()) {
                    found.push(projects);
                }
            }
        }
    }
}

fn real_child_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_real_dir(path))
        .collect()
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_dir())
        .unwrap_or(false)
}

/// OpenCode 扫描库解析：无 `OPENCODE_DB` 时每个数据根发现 `opencode.db` / `opencode-*.db`
/// （找不到则回退 `<root>/opencode.db` 供 legacy JSON 探测）；有 `OPENCODE_DB` 时定位指定库，
/// 绝对路径尚不存在则回退数据目录 `<root>/opencode.db`。
pub(crate) fn resolve_opencode_dbs(roots: &[PathBuf], db: Option<&Path>) -> Vec<PathBuf> {
    if db.is_none() {
        let mut dirs = Vec::new();
        for root in roots {
            let mut found = list_opencode_dbs(root);
            found.sort();
            found.dedup();
            if found.is_empty() {
                push_unique(&mut dirs, root.join("opencode.db"));
            } else {
                for path in found {
                    push_unique(&mut dirs, path);
                }
            }
        }
        return dirs;
    }

    let mut planned = Vec::new();
    for root in roots {
        push_unique(&mut planned, opencode_db_file(root, db));
    }
    let existing: Vec<PathBuf> = planned
        .iter()
        .filter(|path| path.is_file())
        .cloned()
        .collect();
    if !existing.is_empty() {
        return existing;
    }
    // 绝对路径的库还不存在时，旧 JSON 仍在数据目录的 storage/ 下，不在那个绝对路径旁边。
    if db.is_some_and(|path| path.is_absolute()) {
        let mut fallback = Vec::new();
        for root in roots {
            push_unique(&mut fallback, root.join("opencode.db"));
        }
        return fallback;
    }
    planned
}

fn list_opencode_dbs(root: &Path) -> Vec<PathBuf> {
    if root.is_file() {
        return vec![root.to_path_buf()];
    }
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        if name == "opencode.db" || (name.starts_with("opencode-") && name.ends_with(".db")) {
            files.push(path);
        }
    }
    files
}

fn opencode_db_file(root: &Path, db: Option<&Path>) -> PathBuf {
    match db {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => root.join(path),
        None => root.join("opencode.db"),
    }
}

/// Hermes 将 `…/profiles/<name>` 归一回根，并追加非隐藏子 profile 目录。
pub(crate) fn expand_hermes_profiles(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for raw in roots {
        let root = hermes_root(&raw);
        push_unique(&mut dirs, root.clone());
        let profiles = root.join("profiles");
        let Ok(entries) = fs::read_dir(&profiles) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                push_unique(&mut dirs, path);
            }
        }
    }
    dirs
}

fn hermes_root(path: &Path) -> PathBuf {
    let is_profile = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        == Some("profiles");
    if is_profile {
        if let Some(root) = path.parent().and_then(|parent| parent.parent()) {
            return root.to_path_buf();
        }
    }
    path.to_path_buf()
}

/// Crush 保留注册表目录在最前，并追加 `projects.json` 指向的项目 `data_dir`。
pub(crate) fn expand_crush_projects(registries: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut dirs = registries.clone();
    for root in &registries {
        for data_dir in crate::adapters::crush::project_data_dirs(root) {
            push_unique(&mut dirs, data_dir);
        }
    }
    dirs
}
