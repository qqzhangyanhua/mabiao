use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::Connection;

use crate::adapters::cursor::{parse_cursor_commits, summarize_code_volume, CursorCommitRow};
use crate::adapters::opencode::{parse_opencode_messages, OpencodeMessage};
use crate::adapters::{
    claude, codex, copilot, cursor_agent, dsh, factory, gemini, grok, kimi, pi, qwen, LineFactory,
};
use crate::cursor_session;
use crate::domain::{
    CodeVolumeSummary, IngestIssue, IngestReport, Source, SourceDiagnostic, SourceIngestReport,
    UsageRecord,
};
use crate::store;

pub fn default_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// 每个 Source 的路径环境变量名，用于整体覆盖默认扫描根目录（逗号分隔多个绝对路径）。
/// 命名尽量对齐 ccusage 等同类工具的既有约定，方便同时使用多个统计工具的用户复用配置。
const PATH_ENV_VARS: [&str; 12] = [
    "CODEX_HOME",
    "CLAUDE_CONFIG_DIR",
    "PI_AGENT_DIR",
    "OPENCODE_DATA_DIR",
    "KIMI_DATA_DIR",
    "DSH_HOME",
    "GEMINI_DATA_DIR",
    "GROK_HOME",
    "QWEN_DATA_DIR",
    "FACTORY_SESSIONS_DIR",
    "CURSOR_AGENT_USAGE_DIR",
    "COPILOT_HOME",
];

/// 环境变量覆盖表：键为环境变量名，值为解析后的根目录列表。只在真正设置了变量时才有
/// 条目。用一个显式的 map 而不是在各处直接读 `std::env::var`，是为了让路径拼接逻辑可以
/// 脱离进程级环境变量单独做单元测试（并行跑测试时修改真实环境变量并不安全）。
pub(crate) type PathOverrides = BTreeMap<&'static str, Vec<PathBuf>>;

fn env_overrides() -> PathOverrides {
    PATH_ENV_VARS
        .iter()
        .filter_map(|&var| env_override(var).map(|paths| (var, paths)))
        .collect()
}

pub(crate) fn source_scan_dirs(home: &Path, source: Source) -> Vec<PathBuf> {
    source_scan_dirs_with(&env_overrides(), home, source)
}

fn env_override(var: &str) -> Option<Vec<PathBuf>> {
    let raw = std::env::var(var).ok()?;
    let paths: Vec<PathBuf> = raw
        .split(',')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(PathBuf::from)
        .collect();
    (!paths.is_empty()).then_some(paths)
}

/// 解析某个 Source 的根目录列表：环境变量整体覆盖优先，否则退回默认的单个 home 相对路径；
/// 两种情况都按同样的规则拼接叶子子路径（`leaf` 为空表示根目录本身就是扫描目标）。
fn resolve_dirs(
    overrides: &PathOverrides,
    home: &Path,
    env_var: &str,
    default_relative: &str,
    leaf: &str,
) -> Vec<PathBuf> {
    let roots = overrides
        .get(env_var)
        .cloned()
        .unwrap_or_else(|| vec![home.join(default_relative)]);
    if leaf.is_empty() {
        roots
    } else {
        roots.into_iter().map(|root| root.join(leaf)).collect()
    }
}

/// Claude Code 在部分安装方式下把会话写到 XDG 目录（`~/.config/claude`）而不是
/// `~/.claude`；默认两个都扫，显式设置 `CLAUDE_CONFIG_DIR` 后只扫用户指定的目录。
fn resolve_claude_dirs(overrides: &PathOverrides, home: &Path) -> Vec<PathBuf> {
    let roots = overrides
        .get("CLAUDE_CONFIG_DIR")
        .cloned()
        .unwrap_or_else(|| vec![home.join(".claude"), home.join(".config/claude")]);
    roots
        .into_iter()
        .map(|root| root.join("projects"))
        .collect()
}

/// 每个 Source 实际要扫描的目录（可能不止一个）。Kimi 的条目是「工具根目录」而不是
/// 叶子目录，因为 `ingest_kimi` 还要从根目录派生 `sessions/` 和 `kimi.json` 两个子路径。
pub(crate) fn source_scan_dirs_with(
    overrides: &PathOverrides,
    home: &Path,
    source: Source,
) -> Vec<PathBuf> {
    match source {
        Source::Codex => resolve_dirs(overrides, home, "CODEX_HOME", ".codex", "sessions"),
        Source::Claude => resolve_claude_dirs(overrides, home),
        Source::Pi => resolve_dirs(overrides, home, "PI_AGENT_DIR", ".pi/agent/sessions", ""),
        Source::Opencode => resolve_dirs(
            overrides,
            home,
            "OPENCODE_DATA_DIR",
            ".local/share/opencode",
            "opencode.db",
        ),
        Source::Kimi => resolve_dirs(overrides, home, "KIMI_DATA_DIR", ".kimi", ""),
        Source::Dsh => resolve_dirs(overrides, home, "DSH_HOME", ".dsh", "sessions"),
        Source::Gemini => resolve_dirs(overrides, home, "GEMINI_DATA_DIR", ".gemini/tmp", ""),
        Source::Grok => resolve_dirs(overrides, home, "GROK_HOME", ".grok", "sessions"),
        Source::Qwen => resolve_dirs(overrides, home, "QWEN_DATA_DIR", ".qwen", "tmp"),
        Source::Factory => resolve_dirs(
            overrides,
            home,
            "FACTORY_SESSIONS_DIR",
            ".factory/sessions",
            "",
        ),
        // token 包装目录，不是 CLI 原生会话库。会话与 IDE 共用 ~/.cursor。
        Source::CursorAgent => resolve_dirs(
            overrides,
            home,
            "CURSOR_AGENT_USAGE_DIR",
            ".cursor-agent-usage",
            "",
        ),
        Source::Copilot => {
            resolve_dirs(overrides, home, "COPILOT_HOME", ".copilot", "session-state")
        }
    }
}

pub fn ingest_all(conn: &Connection, home: &Path) -> Result<IngestReport, String> {
    ingest_all_with_overrides(conn, home, &env_overrides())
}

/// 与 ingest 使用同一套 cache fingerprint（主文件 metadata + sidecar）。
/// 新文件、fingerprint 变化、或缓存路径已从磁盘消失时视为 stale。
/// 同时覆盖 Cursor 会话 transcript 与代码量 sqlite，避免托盘心跳漏扫这两类输入。
pub fn scan_is_stale(conn: &Connection, home: &Path) -> Result<bool, String> {
    let cache = load_scan_cache(conn)?;
    scan_is_stale_from_cache(&cache, home)
}

/// 只读库快照。托盘心跳先在锁内取出，再松锁扫盘。
pub struct ScanCache {
    ingested: BTreeMap<String, store::IngestedFileCacheRow>,
    cursor_files: BTreeMap<String, (i64, i64)>,
    tracking_fingerprint: String,
}

pub fn load_scan_cache(conn: &Connection) -> Result<ScanCache, String> {
    let ingested = store::cached_ingested_files(conn)?
        .into_iter()
        .map(|row| (row.path.clone(), row))
        .collect();
    let cursor_files = store::cached_cursor_session_file_stats(conn)?
        .into_iter()
        .map(|(path, mtime, size)| (path, (mtime, size)))
        .collect();
    let tracking_fingerprint = store::cursor_tracking_fingerprint(conn)?;
    Ok(ScanCache {
        ingested,
        cursor_files,
        tracking_fingerprint,
    })
}

pub fn scan_is_stale_from_cache(cache: &ScanCache, home: &Path) -> Result<bool, String> {
    scan_is_stale_cached(cache, home, &env_overrides())
}

fn scan_is_stale_cached(
    cache: &ScanCache,
    home: &Path,
    overrides: &PathOverrides,
) -> Result<bool, String> {
    let watched = list_watched_inputs(home, overrides)?;
    let seen: BTreeSet<String> = watched
        .iter()
        .map(|input| input.path.to_string_lossy().into_owned())
        .collect();
    if cache.ingested.len() != seen.len() || cache.ingested.keys().any(|path| !seen.contains(path))
    {
        return Ok(true);
    }
    for input in watched {
        let loc = input.path.to_string_lossy().to_string();
        let meta = match fs::metadata(&input.path) {
            Ok(meta) => meta,
            Err(_) => return Ok(true),
        };
        let key = cache_key(&input.path, &input.extra_fingerprint);
        let Some(row) = cache.ingested.get(&loc) else {
            return Ok(true);
        };
        if row.mtime_ms != modified_millis(&meta)
            || row.size != meta.len() as i64
            || row.source != input.source.as_str()
            || row.fingerprint != key
            || row.adapter_version != store::ADAPTER_VERSION
        {
            return Ok(true);
        }
    }
    cursor_session::scan_is_stale_cached(&cache.cursor_files, &cache.tracking_fingerprint, home)
}

struct WatchedInput {
    source: Source,
    path: PathBuf,
    extra_fingerprint: String,
}

fn cache_key(path: &Path, extra_fingerprint: &str) -> String {
    format!("{}|{extra_fingerprint}", metadata_fingerprint(path))
}

fn list_watched_inputs(
    home: &Path,
    overrides: &PathOverrides,
) -> Result<Vec<WatchedInput>, String> {
    let mut files = Vec::new();
    for source in Source::ALL {
        let dirs = source_scan_dirs_with(overrides, home, source);
        for path in list_source_paths(source, &dirs)? {
            let extra_fingerprint = sidecar_fingerprint(source, &path, &dirs);
            files.push(WatchedInput {
                source,
                path,
                extra_fingerprint,
            });
        }
    }
    Ok(files)
}

fn sidecar_fingerprint(source: Source, path: &Path, dirs: &[PathBuf]) -> String {
    match source {
        Source::Kimi => {
            let root = dirs
                .iter()
                .find(|dir| path.starts_with(dir))
                .cloned()
                .unwrap_or_else(|| path.to_path_buf());
            content_fingerprint(&root.join("kimi.json"))
        }
        Source::Grok => {
            let summary = path
                .parent()
                .map(|parent| parent.join("summary.json"))
                .unwrap_or_default();
            content_fingerprint(&summary)
        }
        Source::Opencode => {
            let wal = PathBuf::from(format!("{}-wal", path.to_string_lossy()));
            metadata_fingerprint(&wal)
        }
        _ => String::new(),
    }
}

fn list_source_paths(source: Source, dirs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    match source {
        Source::Codex | Source::Claude | Source::Pi | Source::CursorAgent | Source::Copilot => {
            list_ext_files(dirs, "jsonl")
        }
        Source::Kimi => {
            let mut paths = Vec::new();
            for root in dirs {
                for path in walk_files(&root.join("sessions"), "jsonl")? {
                    if path.file_name().and_then(|name| name.to_str()) == Some("wire.jsonl") {
                        paths.push(path);
                    }
                }
            }
            Ok(paths)
        }
        Source::Dsh => list_suffix_files(dirs, "session.jsonl.zstd"),
        Source::Gemini => {
            let mut paths = Vec::new();
            for root in dirs {
                for path in walk_files(root, "json")? {
                    if path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("")
                        .starts_with("session-")
                    {
                        paths.push(path);
                    }
                }
            }
            Ok(paths)
        }
        Source::Grok => {
            let mut paths = Vec::new();
            for root in dirs {
                for path in walk_files(root, "jsonl")? {
                    if path.file_name().and_then(|name| name.to_str()) == Some("updates.jsonl") {
                        paths.push(path);
                    }
                }
            }
            Ok(paths)
        }
        Source::Qwen => {
            let mut paths = Vec::new();
            for root in dirs {
                for path in walk_files(root, "json")? {
                    if path.file_name().and_then(|name| name.to_str()) == Some("logs.json") {
                        paths.push(path);
                    }
                }
            }
            Ok(paths)
        }
        Source::Factory => list_suffix_files(dirs, ".settings.json"),
        Source::Opencode => Ok(dirs.iter().filter(|path| path.exists()).cloned().collect()),
    }
}

fn list_ext_files(roots: &[PathBuf], ext: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        paths.extend(walk_files(root, ext)?);
    }
    Ok(paths)
}

fn list_suffix_files(roots: &[PathBuf], suffix: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for root in roots {
        paths.extend(walk_suffix(root, suffix)?);
    }
    Ok(paths)
}

/// 供测试直接注入路径覆盖表，绕开真实进程环境变量（并行跑测试改真实环境变量不安全）。
pub(crate) fn ingest_all_with_overrides(
    conn: &Connection,
    home: &Path,
    overrides: &PathOverrides,
) -> Result<IngestReport, String> {
    let transaction = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let removed_unknown = store::remove_unknown_sources(&transaction)?;
    let mut report = ingest_all_inner(&transaction, home, overrides)?;
    report.records_removed += removed_unknown;
    // 清理未知来源是整表 DELETE，定位不到具体是哪几天，只能整张重来。罕见路径。
    report.rollup_full_rebuild = removed_unknown > 0;
    report.partial_success = report.files_failed > 0 || !report.conversation_issues.is_empty();
    sync_rollup(&transaction, &report)?;
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(report)
}

/// 把预聚合表同步到本轮摄取的结果。
///
/// 放在提交前的同一个事务里：预聚合表和 `usage_records` 必须一起可见，
/// 中间态被查询读到就是错的数字。
///
/// 优先按天重算——一次摄取通常只动到最近一两天，而全量重建在 350 万行的库上要十几秒，
/// 那会把摄取拖成跟历史数据总量成正比。只有整源清理这种定位不到天的改动才整表重来。
/// 供测试直接驱动 `sync_rollup`，验证「补建未完成时不碰预聚合表」这条约束。
#[cfg(test)]
pub(crate) fn sync_rollup_for_tests(
    conn: &Connection,
    report: &IngestReport,
) -> Result<(), String> {
    sync_rollup(conn, report)
}

fn sync_rollup(conn: &Connection, report: &IngestReport) -> Result<(), String> {
    // 还没补建完就别碰：往空表里塞进这一两天，会让它「非空却残缺」。
    // 补建本身会整表重来，这轮的改动到时一并覆盖进去。
    if !store::rollup_is_ready(conn) {
        return Ok(());
    }
    if report.rollup_full_rebuild {
        store::rebuild_rollup(conn)?;
        return Ok(());
    }
    if report.touched_days.is_empty() {
        return Ok(());
    }
    store::rebuild_rollup_days(conn, &report.touched_days)
}

pub fn source_diagnostics(conn: &Connection, home: &Path) -> Result<Vec<SourceDiagnostic>, String> {
    let overrides = env_overrides();
    Source::ALL
        .iter()
        .map(|source| {
            let dirs = source_display_dirs(&overrides, home, *source);
            let (cached_files, record_count, total_tokens, archived_record_count) =
                store::source_cache_stats(conn, *source)?;
            Ok(SourceDiagnostic {
                source: source.as_str().to_string(),
                application: source.application_name().to_string(),
                detected: dirs.iter().any(|dir| dir.exists()),
                root_path: dirs
                    .iter()
                    .map(|dir| dir.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                cached_files,
                record_count,
                total_tokens,
                coverage: source_coverage(*source).to_string(),
                archived_record_count,
            })
        })
        .collect()
}

/// 设置页展示的路径：Cursor Agent 先展示与 IDE 共用的原生目录，包装目录只在实际存在时追加。
fn source_display_dirs(overrides: &PathOverrides, home: &Path, source: Source) -> Vec<PathBuf> {
    match source {
        Source::CursorAgent => {
            let mut dirs = vec![home.join(".cursor/chats"), home.join(".cursor/projects")];
            for dir in source_scan_dirs_with(overrides, home, source) {
                if dir.exists() && !dirs.contains(&dir) {
                    dirs.push(dir);
                }
            }
            dirs
        }
        _ => source_scan_dirs_with(overrides, home, source),
    }
}

pub fn rebuild_cache(
    conn: &Connection,
    home: &Path,
    source: Option<Source>,
) -> Result<IngestReport, String> {
    let overrides = env_overrides();
    let transaction = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut removed_unknown = 0;
    match source {
        Some(source) => store::invalidate_source(&transaction, source)?,
        None => {
            removed_unknown = store::remove_unknown_sources(&transaction)?;
            for source in Source::ALL {
                store::invalidate_source(&transaction, source)?;
            }
        }
    }

    let mut report = IngestReport {
        records_removed: removed_unknown,
        ..IngestReport::default()
    };
    match source {
        Some(source) => {
            ingest_source(&transaction, home, &overrides, source, &mut report)?;
            if source == Source::CursorAgent {
                cursor_session::ingest(&transaction, home, &mut report);
            }
            refresh_conversation_catalog(
                &transaction,
                home,
                &overrides,
                std::slice::from_ref(&source),
                &mut report,
            );
        }
        None => {
            ingest_all_sources(&transaction, home, &overrides, &mut report)?;
            cursor_session::ingest(&transaction, home, &mut report);
            refresh_conversation_catalog(
                &transaction,
                home,
                &overrides,
                crate::conversation::CONVERSATION_SOURCES,
                &mut report,
            );
        }
    }
    report.partial_success = report.files_failed > 0 || !report.conversation_issues.is_empty();
    // 重建缓存必然动了记录，不走 sync_rollup 的「没变就跳过」判断。
    store::rebuild_rollup(&transaction)?;
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(report)
}

fn conversation_source_dirs(
    overrides: &PathOverrides,
    home: &Path,
    source: Source,
) -> Vec<PathBuf> {
    if source == Source::CursorAgent {
        vec![home.join(".cursor/projects")]
    } else {
        source_scan_dirs_with(overrides, home, source)
    }
}

fn refresh_conversation_catalog(
    conn: &Connection,
    home: &Path,
    overrides: &PathOverrides,
    sources: &[Source],
    report: &mut IngestReport,
) {
    for &source in sources {
        if !crate::conversation::CONVERSATION_SOURCES.contains(&source) {
            continue;
        }
        let dirs = conversation_source_dirs(overrides, home, source);
        match crate::conversation::refresh_source_in_roots(conn, source, &dirs) {
            Ok(issues) => report
                .conversation_issues
                .extend(issues.into_iter().map(|issue| IngestIssue {
                    source: source.as_str().to_string(),
                    path: issue.path,
                    message: issue.message,
                    event_type: issue.event_type,
                    line: issue.line,
                })),
            Err(message) => report.conversation_issues.push(IngestIssue {
                source: source.as_str().to_string(),
                path: dirs
                    .iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                message,
                event_type: None,
                line: None,
            }),
        }
    }
}

fn ingest_all_inner(
    conn: &Connection,
    home: &Path,
    overrides: &PathOverrides,
) -> Result<IngestReport, String> {
    let mut report = IngestReport::default();
    ingest_all_sources(conn, home, overrides, &mut report)?;
    cursor_session::ingest(conn, home, &mut report);
    refresh_conversation_catalog(
        conn,
        home,
        overrides,
        crate::conversation::CONVERSATION_SOURCES,
        &mut report,
    );
    report.partial_success = report.partial_success || !report.conversation_issues.is_empty();
    Ok(report)
}

fn ingest_all_sources(
    conn: &Connection,
    home: &Path,
    overrides: &PathOverrides,
    report: &mut IngestReport,
) -> Result<(), String> {
    for source in Source::ALL {
        if let Err(error) = ingest_source(conn, home, overrides, source, report) {
            if error.starts_with("扫描目录") {
                let dirs = source_scan_dirs_with(overrides, home, source);
                let path = dirs
                    .iter()
                    .map(|dir| dir.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                record_failure(report, source, &path, &error);
                continue;
            }
            return Err(error);
        }
    }
    Ok(())
}

fn source_coverage(source: Source) -> &'static str {
    match source {
        Source::Qwen => "本地无 Token",
        Source::Grok => "轮级 Token",
        // Factory/Kimi 本机存储都不含模型名字段，只能按 token 统计，无法按模型定价。
        Source::Factory => "会话累计 Token（无模型名）",
        Source::Kimi => "轮级 Token（无模型名）",
        Source::CursorAgent => "会话与 IDE 共用本机目录；token 仅包装落盘",
        Source::Copilot => "仅会话结束时上报（累计）",
        _ => "轮级 Token",
    }
}

fn ingest_source(
    conn: &Connection,
    home: &Path,
    overrides: &PathOverrides,
    source: Source,
    report: &mut IngestReport,
) -> Result<(), String> {
    let dirs = source_scan_dirs_with(overrides, home, source);
    match source {
        Source::Codex => ingest_jsonl_tree(conn, source, &dirs, "jsonl", report, |lines, path| {
            codex::parse_codex_jsonl(lines, path)
        }),
        Source::Claude => ingest_jsonl_tree(conn, source, &dirs, "jsonl", report, |lines, path| {
            claude::parse_claude_jsonl(lines, path)
        }),
        Source::Pi => ingest_jsonl_tree(conn, source, &dirs, "jsonl", report, |lines, path| {
            pi::parse_pi_jsonl(lines, path)
        }),
        Source::Kimi => ingest_kimi(conn, &dirs, report),
        Source::Dsh => ingest_dsh(conn, &dirs, report),
        Source::Gemini => ingest_gemini(conn, &dirs, report),
        Source::Grok => ingest_grok(conn, &dirs, report),
        Source::Qwen => ingest_qwen(conn, &dirs, report),
        Source::Factory => ingest_factory(conn, &dirs, report),
        Source::CursorAgent => {
            ingest_jsonl_tree(conn, source, &dirs, "jsonl", report, |lines, path| {
                cursor_agent::parse_cursor_agent_jsonl(lines, path)
            })
        }
        Source::Copilot => {
            ingest_jsonl_tree(conn, source, &dirs, "jsonl", report, |lines, path| {
                copilot::parse_copilot_jsonl(lines, path)
            })
        }
        Source::Opencode => ingest_opencode(conn, &dirs, report),
    }
}

/// `roots` 里的每一项都是一个可以直接遍历的扫描目录（叶子目录，已经拼接好子路径）。
///
/// 走 `ingest_one_lines`：按行流式读取磁盘文件，不会先把整份文件内容读进内存。
/// 会话 jsonl 单文件可以到上百 MB（真实观测到 114MB 的 Codex rollout 日志），
/// 启动时全量摄取和对话事件索引两条路径又可能同时处理到同一份大文件，
/// 流式读取能把这条路径的峰值内存从「文件大小」降到几十 KB 的行缓冲区。
fn ingest_jsonl_tree(
    conn: &Connection,
    source: Source,
    roots: &[PathBuf],
    ext: &str,
    report: &mut IngestReport,
    parse: impl Fn(&LineFactory<'_>, &str) -> Vec<UsageRecord>,
) -> Result<(), String> {
    set_detected(report, source, roots.iter().any(|root| root.exists()));
    let mut seen = BTreeSet::new();
    for root in roots {
        for path in walk_files(root, ext)? {
            seen.insert(path.to_string_lossy().to_string());
            ingest_one_lines(conn, source, &path, report, &parse)?;
        }
    }
    reconcile_source(conn, source, &seen, report)
}

fn ingest_one(
    conn: &Connection,
    source: Source,
    path: &Path,
    fingerprint: &str,
    report: &mut IngestReport,
    parse: impl Fn(&[u8], &str) -> Result<Vec<UsageRecord>, String>,
) -> Result<(), String> {
    ingest_one_prepared(conn, source, path, fingerprint, report, |loc| {
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        parse(&bytes, loc)
    })
}

/// jsonl 专用：整份文件从不进内存，逐行校验+解析在同一趟磁盘流式读取里完成。
/// `fingerprint` 固定传空串——jsonl 来源的缓存键只看 `(mtime_ms, size)`。
fn ingest_one_lines(
    conn: &Connection,
    source: Source,
    path: &Path,
    report: &mut IngestReport,
    parse: impl Fn(&LineFactory<'_>, &str) -> Vec<UsageRecord>,
) -> Result<(), String> {
    ingest_one_prepared(conn, source, path, "", report, |loc| {
        validate_jsonl_file(path)?;
        let factory: &LineFactory<'_> = &|| open_jsonl_lines(path);
        Ok(parse(factory, loc))
    })
}

/// `ingest_one`/`ingest_one_lines` 共用的缓存检查 + 落库逻辑，二者只在
/// 「如何把文件内容变成 `Vec<UsageRecord>`」这一步不同（整份读入 vs 流式读取）。
fn ingest_one_prepared(
    conn: &Connection,
    source: Source,
    path: &Path,
    fingerprint: &str,
    report: &mut IngestReport,
    read_records: impl FnOnce(&str) -> Result<Vec<UsageRecord>, String>,
) -> Result<(), String> {
    increment(report, source, |source_report| {
        source_report.files_seen += 1
    });
    report.files_seen += 1;
    let loc = path.to_string_lossy().to_string();
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) => {
            record_failure(report, source, &loc, &error.to_string());
            return Ok(());
        }
    };
    let size = meta.len() as i64;
    let mtime_ms = modified_millis(&meta);
    let cache_fingerprint = cache_key(path, fingerprint);
    if store::file_unchanged(conn, &loc, mtime_ms, size, source, &cache_fingerprint)? {
        increment(report, source, |source_report| {
            source_report.files_skipped += 1
        });
        report.files_skipped += 1;
        return Ok(());
    }
    let records = match read_records(&loc) {
        Ok(records) => records,
        Err(error) => {
            record_failure(report, source, &loc, &error);
            return Ok(());
        }
    };
    let previous_count = store::record_count_for_file(conn, &loc)?;
    if previous_count > 0 && records.len() < previous_count as usize && is_append_log_source(source)
    {
        record_failure(
            report,
            source,
            &loc,
            &format!(
                "解析记录从 {previous_count} 条降为 {} 条，已保留上次正确缓存",
                records.len()
            ),
        );
        return Ok(());
    }
    // 必须在删除之前问：记录一删，就查不出它们原来落在哪几天了。
    let previous_days = store::days_for_file(conn, &loc)?;

    store::delete_records_for_file(conn, &loc)?;
    let written = store::insert_records(conn, &records)?;
    store::mark_file(conn, &loc, mtime_ms, size, source, &cache_fingerprint)?;
    // 删掉的旧记录和刚写入的新记录各自占了哪些天，这些天的预聚合行都得重算。
    // 两次查询都走 idx_usage_source_file，比重建整张表便宜得多。
    report.touched_days.extend(previous_days);
    report
        .touched_days
        .extend(store::days_for_file(conn, &loc)?);
    report.records_written += written;
    report.files_parsed += 1;
    increment(report, source, |source_report| {
        source_report.records_written += written;
        source_report.files_parsed += 1;
    });
    Ok(())
}

fn is_append_log_source(source: Source) -> bool {
    matches!(
        source,
        Source::Codex
            | Source::Claude
            | Source::Pi
            | Source::Kimi
            | Source::Dsh
            | Source::Grok
            | Source::CursorAgent
            | Source::Copilot
    )
}

fn validate_jsonl(content: &str) -> Result<(), String> {
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // 只校验语法，不建 `Value` 树——正文随后由适配器逐行解析，这里没必要留下整份堆表示。
        serde_json::from_str::<serde::de::IgnoredAny>(line)
            .map_err(|error| format!("第 {} 行 JSON 无效：{error}", index + 1))?;
    }
    Ok(())
}

/// `validate_jsonl` 的磁盘流式版本：按行读取校验，不把整份文件读进内存。
/// 供 `ingest_one_lines` 使用；只读一遍磁盘（内容随后交给适配器再读一遍，
/// 这时文件通常已经在 OS page cache 里，重复读盘的代价远小于把整份文件留在内存里）。
fn validate_jsonl_file(path: &Path) -> Result<(), String> {
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|error| format!("第 {} 行读取失败：{error}", index + 1))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        serde_json::from_str::<serde::de::IgnoredAny>(line)
            .map_err(|error| format!("第 {} 行 JSON 无效：{error}", index + 1))?;
    }
    Ok(())
}

/// 打开文件并按行流式产出内容；打开失败时返回空迭代器——`ingest_one_lines` 已经在
/// `validate_jsonl_file` 里对同一个 `path` 做过一次可读性检查，这里理论上不会失败。
fn open_jsonl_lines(path: &Path) -> Box<dyn Iterator<Item = String>> {
    match fs::File::open(path) {
        Ok(file) => Box::new(BufReader::new(file).lines().map_while(Result::ok)),
        Err(_) => Box::new(std::iter::empty()),
    }
}

/// `roots` 里的每一项都是一个 Kimi 工具根目录（下面挂 `sessions/` 和 `kimi.json`）。
fn ingest_kimi(
    conn: &Connection,
    roots: &[PathBuf],
    report: &mut IngestReport,
) -> Result<(), String> {
    let source = Source::Kimi;
    set_detected(
        report,
        source,
        roots.iter().any(|root| root.join("sessions").exists()),
    );
    let mut seen = BTreeSet::new();
    for root in roots {
        let sessions = root.join("sessions");
        let sidecar = root.join("kimi.json");
        let fingerprint = content_fingerprint(&sidecar);
        let projects = match kimi_projects(root) {
            Ok(projects) => projects,
            Err(error) => {
                record_failure(
                    report,
                    source,
                    &sidecar.to_string_lossy(),
                    &format!("Kimi 项目映射无效：{error}"),
                );
                continue;
            }
        };
        for path in walk_files(&sessions, "jsonl")? {
            if path.file_name().and_then(|name| name.to_str()) != Some("wire.jsonl") {
                continue;
            }
            seen.insert(path.to_string_lossy().to_string());
            let session_id = path
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_string();
            let project = projects
                .iter()
                .find(|(id, _)| id == &session_id)
                .map(|(_, project)| project.clone())
                .unwrap_or_else(|| {
                    path.parent()
                        .and_then(|parent| parent.parent())
                        .and_then(|parent| parent.file_name())
                        .and_then(|name| name.to_str())
                        .unwrap_or("")
                        .to_string()
                });
            ingest_one(conn, source, &path, &fingerprint, report, |bytes, loc| {
                let content = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
                validate_jsonl(content)?;
                Ok(kimi::parse_kimi_wire(content, loc, &project))
            })?;
        }
    }
    reconcile_source(conn, source, &seen, report)
}

fn kimi_projects(root: &Path) -> Result<Vec<(String, String)>, String> {
    let path = root.join("kimi.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let value =
        serde_json::from_str::<serde_json::Value>(&text).map_err(|error| error.to_string())?;
    Ok(value
        .get("work_dirs")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some((
                        item.get("last_session_id")?.as_str()?.to_string(),
                        item.get("path")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default())
}

fn ingest_dsh(
    conn: &Connection,
    roots: &[PathBuf],
    report: &mut IngestReport,
) -> Result<(), String> {
    let source = Source::Dsh;
    set_detected(report, source, roots.iter().any(|root| root.exists()));
    let mut seen = BTreeSet::new();
    for root in roots {
        for path in walk_suffix(root, "session.jsonl.zstd")? {
            seen.insert(path.to_string_lossy().to_string());
            ingest_one(conn, source, &path, "", report, dsh::parse_dsh_zstd)?;
        }
    }
    reconcile_source(conn, source, &seen, report)
}

fn ingest_gemini(
    conn: &Connection,
    roots: &[PathBuf],
    report: &mut IngestReport,
) -> Result<(), String> {
    let source = Source::Gemini;
    set_detected(report, source, roots.iter().any(|root| root.exists()));
    let mut seen = BTreeSet::new();
    for root in roots {
        for path in walk_files(root, "json")? {
            if !path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .starts_with("session-")
            {
                continue;
            }
            seen.insert(path.to_string_lossy().to_string());
            ingest_one(conn, source, &path, "", report, |bytes, loc| {
                let content = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
                serde_json::from_str::<serde::de::IgnoredAny>(content)
                    .map_err(|e| e.to_string())?;
                Ok(gemini::parse_gemini_session(content, loc))
            })?;
        }
    }
    reconcile_source(conn, source, &seen, report)
}

fn ingest_grok(
    conn: &Connection,
    roots: &[PathBuf],
    report: &mut IngestReport,
) -> Result<(), String> {
    let source = Source::Grok;
    set_detected(report, source, roots.iter().any(|root| root.exists()));
    let mut seen = BTreeSet::new();
    for root in roots {
        for path in walk_files(root, "jsonl")? {
            if path.file_name().and_then(|name| name.to_str()) != Some("updates.jsonl") {
                continue;
            }
            seen.insert(path.to_string_lossy().to_string());
            let summary_path = path
                .parent()
                .map(|parent| parent.join("summary.json"))
                .unwrap_or_default();
            let fingerprint = content_fingerprint(&summary_path);
            let summary = if summary_path.exists() {
                match fs::read_to_string(&summary_path)
                    .map_err(|error| error.to_string())
                    .and_then(|text| {
                        serde_json::from_str::<serde_json::Value>(&text)
                            .map_err(|error| error.to_string())
                    }) {
                    Ok(summary) => Some(summary),
                    Err(error) => {
                        record_failure(
                            report,
                            source,
                            &summary_path.to_string_lossy(),
                            &format!("Grok 模型摘要无效：{error}"),
                        );
                        continue;
                    }
                }
            } else {
                None
            };
            let model = summary
                .as_ref()
                .and_then(|value| value.get("current_model_id"))
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            ingest_one(conn, source, &path, &fingerprint, report, |bytes, loc| {
                let content = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
                validate_jsonl(content)?;
                Ok(grok::parse_grok_updates(content, loc, &model))
            })?;
        }
    }
    reconcile_source(conn, source, &seen, report)
}

fn ingest_qwen(
    conn: &Connection,
    roots: &[PathBuf],
    report: &mut IngestReport,
) -> Result<(), String> {
    let source = Source::Qwen;
    set_detected(report, source, roots.iter().any(|root| root.exists()));
    let mut seen = BTreeSet::new();
    for root in roots {
        for path in walk_files(root, "json")? {
            if path.file_name().and_then(|name| name.to_str()) != Some("logs.json") {
                continue;
            }
            seen.insert(path.to_string_lossy().to_string());
            ingest_one(conn, source, &path, "", report, |bytes, loc| {
                let content = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
                serde_json::from_str::<serde::de::IgnoredAny>(content)
                    .map_err(|e| e.to_string())?;
                Ok(qwen::parse_qwen_session(content, loc))
            })?;
        }
    }
    reconcile_source(conn, source, &seen, report)
}

fn ingest_factory(
    conn: &Connection,
    roots: &[PathBuf],
    report: &mut IngestReport,
) -> Result<(), String> {
    let source = Source::Factory;
    set_detected(report, source, roots.iter().any(|root| root.exists()));
    let mut seen = BTreeSet::new();
    for root in roots {
        for path in walk_suffix(root, ".settings.json")? {
            seen.insert(path.to_string_lossy().to_string());
            ingest_one(conn, source, &path, "", report, |bytes, loc| {
                let content = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
                serde_json::from_str::<serde::de::IgnoredAny>(content)
                    .map_err(|e| e.to_string())?;
                Ok(factory::parse_factory_settings(content, loc))
            })?;
        }
    }
    reconcile_source(conn, source, &seen, report)
}

/// `db_paths` 是每个候选 OpenCode 数据目录下的 `opencode.db` 文件本身（叶子文件路径）。
fn ingest_opencode(
    conn: &Connection,
    db_paths: &[PathBuf],
    report: &mut IngestReport,
) -> Result<(), String> {
    let source = Source::Opencode;
    set_detected(report, source, db_paths.iter().any(|path| path.exists()));
    let mut seen = BTreeSet::new();
    for db_path in db_paths {
        if !db_path.exists() {
            continue;
        }
        seen.insert(db_path.to_string_lossy().to_string());
        let wal_path = PathBuf::from(format!("{}-wal", db_path.to_string_lossy()));
        let fingerprint = metadata_fingerprint(&wal_path);
        ingest_one(conn, source, db_path, &fingerprint, report, |_, loc| {
            let source_db = open_readonly(db_path)?;
            let mut stmt = source_db
                .prepare("SELECT session_id, data FROM message")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| e.to_string())?;
            let mut messages = Vec::new();
            for row in rows {
                let (session_id, data) = row.map_err(|e| e.to_string())?;
                let data = serde_json::from_str(&data)
                    .map_err(|error| format!("OpenCode message JSON 无效：{error}"))?;
                messages.push(OpencodeMessage {
                    session_id,
                    source_file: loc.to_string(),
                    data,
                });
            }
            Ok(parse_opencode_messages(&messages))
        })?;
    }
    reconcile_source(conn, source, &seen, report)
}

fn reconcile_source(
    conn: &Connection,
    source: Source,
    seen: &BTreeSet<String>,
    report: &mut IngestReport,
) -> Result<(), String> {
    if source_report_mut(report, source).files_failed > 0 {
        return Ok(());
    }
    let archived = store::reconcile_source(conn, source, seen)?;
    report.records_archived += archived;
    increment(report, source, |source_report| {
        source_report.records_archived += archived
    });
    Ok(())
}

fn source_report_mut(report: &mut IngestReport, source: Source) -> &mut SourceIngestReport {
    report
        .sources
        .iter_mut()
        .find(|entry| entry.source == source.as_str())
        .expect("all sources are initialized")
}

fn increment(
    report: &mut IngestReport,
    source: Source,
    update: impl FnOnce(&mut SourceIngestReport),
) {
    update(source_report_mut(report, source));
}

fn set_detected(report: &mut IngestReport, source: Source, detected: bool) {
    source_report_mut(report, source).detected = detected;
}

fn record_failure(report: &mut IngestReport, source: Source, path: &str, message: &str) {
    report.files_failed += 1;
    report.partial_success = true;
    increment(report, source, |source_report| {
        source_report.files_failed += 1
    });
    report.issues.push(IngestIssue {
        source: source.as_str().to_string(),
        path: path.to_string(),
        message: message.to_string(),
        event_type: None,
        line: None,
    });
}

fn modified_millis(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn content_fingerprint(path: &Path) -> String {
    let Ok(bytes) = fs::read(path) else {
        return "missing".to_string();
    };
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in &bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{}:{hash:016x}", bytes.len())
}

pub(crate) fn metadata_fingerprint(path: &Path) -> String {
    let Ok(meta) = fs::metadata(path) else {
        return "missing".to_string();
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!(
            "{}:{modified}:{}:{}:{}",
            meta.len(),
            meta.ino(),
            meta.ctime(),
            meta.ctime_nsec()
        )
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        format!(
            "{}:{modified}:{}:{}",
            meta.len(),
            meta.creation_time(),
            meta.file_attributes()
        )
    }
    #[cfg(not(any(unix, windows)))]
    format!("{}:{modified}", meta.len())
}

pub fn load_code_volume(home: &Path) -> Result<CodeVolumeSummary, String> {
    let db_path = home.join(".cursor/ai-tracking/ai-code-tracking.db");
    if !db_path.exists() {
        return Ok(CodeVolumeSummary::empty());
    }
    let source_db = open_readonly(&db_path)?;
    let mut stmt = source_db
        .prepare(
            r#"
            SELECT commitHash, branchName, scoredAt,
                   COALESCE(commitMessage, ''),
                   COALESCE(linesAdded, 0),
                   COALESCE(linesDeleted, 0),
                   COALESCE(composerLinesAdded, 0),
                   COALESCE(composerLinesDeleted, 0),
                   COALESCE(humanLinesAdded, 0),
                   COALESCE(humanLinesDeleted, 0),
                   COALESCE(tabLinesAdded, 0),
                   COALESCE(tabLinesDeleted, 0),
                   v2AiPercentage
            FROM scored_commits
            WHERE linesAdded IS NOT NULL OR v2AiPercentage IS NOT NULL
            "#,
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let percentage: Option<String> = row.get(12)?;
            Ok(CursorCommitRow {
                commit_hash: row.get(0)?,
                branch: row.get(1)?,
                scored_at_ms: row.get(2)?,
                commit_message: row.get(3)?,
                lines_added: row.get(4)?,
                lines_deleted: row.get(5)?,
                composer_lines_added: row.get(6)?,
                composer_lines_deleted: row.get(7)?,
                human_lines_added: row.get(8)?,
                human_lines_deleted: row.get(9)?,
                tab_lines_added: row.get(10)?,
                tab_lines_deleted: row.get(11)?,
                ai_percentage: percentage.and_then(|value| value.parse().ok()),
            })
        })
        .map_err(|e| e.to_string())?;
    let commits: Result<Vec<_>, _> = rows.collect();
    let parsed = parse_cursor_commits(&commits.map_err(|e| e.to_string())?);
    Ok(summarize_code_volume(&parsed))
}

pub(crate) fn open_readonly(path: &Path) -> Result<rusqlite::Connection, String> {
    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

pub(crate) fn walk_files(root: &Path, extension: &str) -> Result<Vec<PathBuf>, String> {
    walk_matching(root, |path| {
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case(extension))
            .unwrap_or(false)
    })
}

fn walk_suffix(root: &Path, suffix: &str) -> Result<Vec<PathBuf>, String> {
    walk_matching(root, |path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.ends_with(suffix))
            .unwrap_or(false)
    })
}

fn walk_matching(root: &Path, matches: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let entries =
        fs::read_dir(root).map_err(|error| format!("扫描目录 {} 失败：{error}", root.display()))?;
    let mut output = Vec::new();
    let mut stack = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| format!("扫描目录 {} 失败：{error}", root.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    while let Some(path) = stack.pop() {
        let file_type = fs::symlink_metadata(&path)
            .map_err(|error| format!("扫描目录 {} 失败：{error}", path.display()))?
            .file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let entries = fs::read_dir(&path)
                .map_err(|error| format!("扫描目录 {} 失败：{error}", path.display()))?;
            stack.extend(
                entries
                    .map(|entry| {
                        entry
                            .map(|entry| entry.path())
                            .map_err(|error| format!("扫描目录 {} 失败：{error}", path.display()))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            );
        } else if file_type.is_file() && matches(&path) {
            output.push(path);
        }
    }
    Ok(output)
}
