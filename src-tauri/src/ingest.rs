//! 可信摄取：发现、比指纹、解析、验证、落库、对账、同步预聚合。
//!
//! 各来源的扫描目录、发现规则、辅助指纹、解析与展示文案由适配器表提供。
//! 本模块只负责缓存命中、失败不覆盖、追加型日志截断检测、删除对账与预聚合同步。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::Connection;

use crate::adapters::cursor::{parse_cursor_commits, summarize_code_volume, CursorCommitRow};
use crate::adapters::{usage_adapter, UsageAdapter};
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
pub(crate) fn resolve_dirs(
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

/// 每个来源实际要扫描的目录（可能不止一个），一律从表里取。
pub(crate) fn source_scan_dirs_with(
    overrides: &PathOverrides,
    home: &Path,
    source: Source,
) -> Vec<PathBuf> {
    (usage_adapter(source).scan_dirs)(overrides, home)
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
        let adapter = usage_adapter(source);
        let dirs = (adapter.scan_dirs)(overrides, home);
        for path in (adapter.discover)(&dirs)? {
            files.push(WatchedInput {
                source,
                path,
                extra_fingerprint: (adapter.sidecar_fingerprint)(&path, &dirs),
            });
        }
    }
    Ok(files)
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
            let adapter = usage_adapter(*source);
            let dirs = adapter.display_or_scan_dirs(&overrides, home);
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
                coverage: adapter.coverage.to_string(),
                archived_record_count,
            })
        })
        .collect()
}

pub fn rebuild_cache(
    conn: &Connection,
    home: &Path,
    source: Option<Source>,
) -> Result<IngestReport, String> {
    let overrides = env_overrides();
    let transaction = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut removed_unknown = 0;
    if let Some(selected) = source {
        store::invalidate_source(&transaction, selected)?;
    } else {
        removed_unknown = store::remove_unknown_sources(&transaction)?;
        for item in Source::ALL {
            store::invalidate_source(&transaction, item)?;
        }
    }

    let mut report = IngestReport {
        records_removed: removed_unknown,
        ..IngestReport::default()
    };
    if let Some(selected) = source {
        ingest_source(&transaction, home, &overrides, selected, &mut report)?;
        if selected == Source::CursorAgent {
            cursor_session::ingest(&transaction, home, &mut report);
        }
        refresh_conversation_catalog(
            &transaction,
            home,
            &overrides,
            std::slice::from_ref(&selected),
            &mut report,
        );
    } else {
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

fn ingest_source(
    conn: &Connection,
    home: &Path,
    overrides: &PathOverrides,
    source: Source,
    report: &mut IngestReport,
) -> Result<(), String> {
    let adapter = usage_adapter(source);
    let dirs = (adapter.scan_dirs)(overrides, home);
    ingest_from_adapter(conn, adapter, &dirs, report)
}

/// 缓存检查、校验、落库。适配器只负责把路径变成消耗记录。
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
    usage_adapter(source).append_log
}

pub(crate) fn validate_jsonl(content: &str) -> Result<(), String> {
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
/// 供流式 jsonl 适配器使用；只读一遍磁盘（内容随后交给适配器再读一遍，
/// 这时文件通常已经在 OS page cache 里，重复读盘的代价远小于把整份文件留在内存里）。
pub(crate) fn validate_jsonl_file(path: &Path) -> Result<(), String> {
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

/// 打开文件并按行流式产出内容；打开失败时返回空迭代器——调用方已经在
/// `validate_jsonl_file` 里对同一个 `path` 做过一次可读性检查，这里理论上不会失败。
pub(crate) fn open_jsonl_lines(path: &Path) -> Box<dyn Iterator<Item = String>> {
    match fs::File::open(path) {
        Ok(file) => Box::new(BufReader::new(file).lines().map_while(Result::ok)),
        Err(_) => Box::new(std::iter::empty()),
    }
}

/// 发现、比指纹、解析、对账。怎么读文件由适配器自己决定。
fn ingest_from_adapter(
    conn: &Connection,
    adapter: &UsageAdapter,
    dirs: &[PathBuf],
    report: &mut IngestReport,
) -> Result<(), String> {
    set_detected(report, adapter.source, adapter.roots_detected(dirs));
    let mut seen = BTreeSet::new();
    for root in dirs {
        if let Some(prepare_dir) = adapter.prepare_dir {
            if let Err((path, error)) = prepare_dir(root) {
                record_failure(report, adapter.source, &path.to_string_lossy(), &error);
                continue;
            }
        }
        for path in (adapter.discover)(std::slice::from_ref(root))? {
            seen.insert(path.to_string_lossy().to_string());
            let fingerprint = (adapter.sidecar_fingerprint)(&path, dirs);
            ingest_one_prepared(conn, adapter.source, &path, &fingerprint, report, |_| {
                (adapter.parse)(&path, root)
            })?;
        }
    }
    reconcile_source(conn, adapter.source, &seen, report)
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

pub(crate) fn walk_suffix(root: &Path, suffix: &str) -> Result<Vec<PathBuf>, String> {
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
