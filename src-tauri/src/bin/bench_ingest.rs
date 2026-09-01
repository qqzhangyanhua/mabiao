//! 本机 ingest 分阶段计时（只读真实 home，写到 usage.sqlite 副本）。
//!
//!   cargo run --release --bin bench_ingest --manifest-path src-tauri/Cargo.toml
//!
//! 流程：复制本机库 → 多轮稳定化（消化旧 ADAPTER_VERSION / 可修复项）
//! → 从稳定副本测 warm-skip 与各类脏路径。持续失败的文件会计入基线税。

use std::path::Path;
use std::time::{Duration, Instant};

use mabiao_lib::domain::IngestReport;
use mabiao_lib::ingest::{self, IngestPhaseTimings};
use mabiao_lib::{paths, store};
use rusqlite::{params, Connection};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = if args.iter().any(|a| a == "micro") {
        run_micro()
    } else if args.iter().any(|a| a == "verify-stuck") {
        run_verify_stuck()
    } else {
        run()
    };
    if let Err(error) = result {
        eprintln!("bench_ingest 失败：{error}");
        std::process::exit(1);
    }
}

fn run_verify_stuck() -> Result<(), String> {
    let home = ingest::default_home();
    let live_db = paths::app_data_dir().join(mabiao_lib::backup::DB_NAME);
    let target = largest_codex_path(&live_db)?;
    let work_dir = std::env::temp_dir().join("mabiao-bench-ingest");
    std::fs::create_dir_all(&work_dir).map_err(|e| e.to_string())?;
    let db = work_dir.join(format!("verify-stuck-{}.sqlite", std::process::id()));
    clone_db(&live_db, &db)?;
    let conn = store::open_db(path_str(&db)?)?;

    let before_version = store::cached_adapter_version(&conn, &target.path)?;
    let before_count = store::record_count_for_file(&conn, &target.path)?;
    println!(
        "# verify-stuck\nbefore version={before_version:?} records={before_count}"
    );

    let (report, timings) = ingest::ingest_all_timed(&conn, &home)?;
    let after_version = store::cached_adapter_version(&conn, &target.path)?;
    let after_count = store::record_count_for_file(&conn, &target.path)?;
    println!(
        "ingest parsed={} failed={} skipped={} written={} usage_ms={} total_ms={}",
        report.files_parsed,
        report.files_failed,
        report.files_skipped,
        report.records_written,
        timings.usage_ms,
        timings.total_ms
    );
    println!("after version={after_version:?} records={after_count}");

    let (report2, timings2) = ingest::ingest_all_timed(&conn, &home)?;
    println!(
        "second parsed={} failed={} skipped={} usage_ms={} total_ms={}",
        report2.files_parsed,
        report2.files_failed,
        report2.files_skipped,
        timings2.usage_ms,
        timings2.total_ms
    );
    cleanup_db(&db);
    if after_version != Some(store::ADAPTER_VERSION) {
        return Err(format!(
            "目标文件未升到当前 ADAPTER_VERSION：{after_version:?}"
        ));
    }
    Ok(())
}

fn run_micro() -> Result<(), String> {
    use mabiao_lib::adapters;
    use mabiao_lib::conversation;
    use std::fs;

    let live_db = paths::app_data_dir().join(mabiao_lib::backup::DB_NAME);
    let target = largest_codex_path(&live_db)?;
    let path = Path::new(&target.path);
    println!("# microbench target={}", path.display());
    println!(
        "size={:.1} MB  events≈{}",
        target.size as f64 / (1024.0 * 1024.0),
        target.event_count
    );

    // 1) 用量：流式校验 + 解析
    let rss0 = rss_mb();
    let started = Instant::now();
    let validate = mabiao_lib::ingest::validate_jsonl_file(path);
    let validate_ms = started.elapsed().as_millis();
    println!(
        "usage_validate_ms={}  ok={}  rss_mb={:?}",
        validate_ms,
        validate.is_ok(),
        rss_mb()
    );
    if let Err(error) = &validate {
        println!("  validate_err={error}");
    }

    let started = Instant::now();
    let parsed = adapters::codex::parse(path, path.parent().unwrap_or(path));
    let parse_ms = started.elapsed().as_millis();
    match &parsed {
        Ok(records) => println!(
            "usage_parse_ms={}  records={}  rss_mb={:?}  rss0={:?}",
            parse_ms,
            records.len(),
            rss_mb(),
            rss0
        ),
        Err(error) => println!("usage_parse_ms={parse_ms}  err={error}"),
    }

    // 与缓存条数对照，解释为何可能永远 files_failed
    let conn = Connection::open(&live_db).map_err(|e| e.to_string())?;
    let cached_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM usage_records WHERE source_file = ?1",
            params![target.path],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let cached_version: i64 = conn
        .query_row(
            "SELECT adapter_version FROM ingested_files WHERE path = ?1",
            params![target.path],
            |row| row.get(0),
        )
        .unwrap_or(-1);
    println!(
        "usage_cached_records={}  ingested_adapter_version={}  current_ADAPTER_VERSION={}",
        cached_count,
        cached_version,
        store::ADAPTER_VERSION
    );
    if let Ok(records) = &parsed {
        if cached_count > 0 && (records.len() as i64) < cached_count {
            println!(
                "  → append_log 会拒绝覆盖（{} < {}），每轮重解析且 version 卡在 {}",
                records.len(),
                cached_count,
                cached_version
            );
        }
    }

    // 2) 对话：整文件 index（Codex 当前实现是 read_to_string）
    let rss1 = rss_mb();
    let started = Instant::now();
    let indexed = conversation::codex_index_for_bench(path);
    let index_ms = started.elapsed().as_millis();
    match indexed {
        Ok(events) => println!(
            "conversation_index_ms={}  events={}  rss_before={:?}  rss_after={:?}",
            index_ms,
            events,
            rss1,
            rss_mb()
        ),
        Err(error) => println!("conversation_index_ms={index_ms}  err={error}"),
    }

    // 3) 对话：后缀 256KB
    let offset = (target.size as u64).saturating_sub(256 * 1024);
    let rss2 = rss_mb();
    let started = Instant::now();
    let suffix = conversation::codex_index_suffix_for_bench(
        path,
        offset,
        0,
        &target.session_id,
    );
    let suffix_ms = started.elapsed().as_millis();
    match suffix {
        Ok(events) => println!(
            "conversation_suffix_256k_ms={}  new_events={}  rss_before={:?}  rss_after={:?}",
            suffix_ms,
            events,
            rss2,
            rss_mb()
        ),
        Err(error) => println!("conversation_suffix_256k_ms={suffix_ms}  err={error}"),
    }

    let _ = fs::metadata(path);
    Ok(())
}

fn run() -> Result<(), String> {
    let home = ingest::default_home();
    let live_db = paths::app_data_dir().join(mabiao_lib::backup::DB_NAME);
    if !live_db.exists() {
        return Err(format!("找不到本机库：{}", live_db.display()));
    }

    let target = largest_codex_path(&live_db)?;
    println!("# bench_ingest");
    println!("home={}", home.display());
    println!("live_db={}", live_db.display());
    println!(
        "target={} ({:.1} MB)",
        target.path,
        target.size as f64 / (1024.0 * 1024.0)
    );
    println!(
        "conversation_events≈{}  indexed_offset={}",
        target.event_count, target.indexed_byte_offset
    );
    println!();

    let work_dir = std::env::temp_dir().join("mabiao-bench-ingest");
    std::fs::create_dir_all(&work_dir).map_err(|e| e.to_string())?;
    let base_db = work_dir.join(format!("base-{}.sqlite", std::process::id()));
    clone_db(&live_db, &base_db)?;

    {
        let conn = store::open_db(path_str(&base_db)?)?;
        println!("## stabilize");
        for round in 1..=5 {
            let before = adapter_version_stats(&conn)?;
            let (report, timings) = ingest::ingest_all_timed(&conn, &home)?;
            let after = adapter_version_stats(&conn)?;
            println!(
                "  round{round}: total={}ms usage={} conversation={} parsed={} skipped={} failed={} issues={}",
                timings.total_ms,
                timings.usage_ms,
                timings.conversation_ms,
                report.files_parsed,
                report.files_skipped,
                report.files_failed,
                report.conversation_issues.len()
            );
            println!(
                "           usage_ver_lt_current: {}→{}  conv_ver_lt_current: {}→{}",
                before.0, after.0, before.1, after.1
            );
            // 可升级项消化完后，剩下的 parsed 应接近 0；failed 可能长期存在。
            if report.files_parsed == 0 && after.0 == 0 {
                break;
            }
        }
        println!();
    }

    let scenarios = [
        ("warm-skip", Scenario::WarmSkip),
        ("dirty-usage", Scenario::DirtyUsage),
        ("incremental-conversation", Scenario::IncrementalConversation),
        ("dirty-conversation", Scenario::DirtyConversation),
        ("dirty-both", Scenario::DirtyBoth),
    ];

    for (label, scenario) in scenarios {
        let scenario_db = work_dir.join(format!("{label}-{}.sqlite", std::process::id()));
        clone_db(&base_db, &scenario_db)?;
        let conn = store::open_db(path_str(&scenario_db)?)?;
        apply_scenario(&conn, scenario, &target)?;

        let before_rss = rss_mb();
        let started = Instant::now();
        let (report, timings) = ingest::ingest_all_timed(&conn, &home)?;
        let wall = started.elapsed();
        let after_rss = rss_mb();
        print_result(label, &report, &timings, after_rss);
        println!(
            "  wall_ms={}  rss_before_mb={:?}  rss_after_mb={:?}  rss_delta_mb={:?}",
            wall.as_millis(),
            before_rss,
            after_rss,
            match (before_rss, after_rss) {
                (Some(before), Some(after)) => Some(after.saturating_sub(before)),
                _ => None,
            }
        );
        if let Ok(Some(row)) = target_file_row(&conn, &target) {
            println!(
                "  target_after: size={} offset={} max_seq={:?} events={}",
                row.0, row.1, row.2, row.3
            );
        }
        println!();
        cleanup_db(&scenario_db);
    }

    cleanup_db(&base_db);
    Ok(())
}

#[derive(Clone, Copy)]
enum Scenario {
    WarmSkip,
    DirtyUsage,
    DirtyConversation,
    DirtyBoth,
    IncrementalConversation,
}

struct TargetFile {
    path: String,
    size: i64,
    session_id: String,
    indexed_byte_offset: i64,
    event_count: i64,
}

fn largest_codex_path(live_db: &Path) -> Result<TargetFile, String> {
    let conn = Connection::open(live_db).map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT source_file, source_file_size, session_id, indexed_byte_offset,
                (SELECT COUNT(*) FROM conversation_events e
                 WHERE e.source = f.source AND e.session_id = f.session_id)
         FROM conversation_session_files f
         WHERE source = 'codex'
         ORDER BY source_file_size DESC
         LIMIT 1",
        [],
        |row| {
            Ok(TargetFile {
                path: row.get(0)?,
                size: row.get(1)?,
                session_id: row.get(2)?,
                indexed_byte_offset: row.get(3)?,
                event_count: row.get(4)?,
            })
        },
    )
    .map_err(|e| format!("找不到最大 Codex 会话：{e}"))
}

fn clone_db(src_path: &Path, dest: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(dest);
    let _ = std::fs::remove_file(format!("{}-wal", dest.display()));
    let _ = std::fs::remove_file(format!("{}-shm", dest.display()));
    let started = Instant::now();
    let src = Connection::open(src_path).map_err(|e| e.to_string())?;
    let mut dst = Connection::open(dest).map_err(|e| e.to_string())?;
    {
        let backup = rusqlite::backup::Backup::new(&src, &mut dst)
            .map_err(|e| format!("打开 backup 失败：{e}"))?;
        backup
            .run_to_completion(50_000, Duration::from_millis(0), None)
            .map_err(|e| format!("复制库失败：{e}"))?;
    }
    println!(
        "## clone {} → {} ({:.1}s)",
        src_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?"),
        dest.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?"),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn cleanup_db(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
}

fn path_str(path: &Path) -> Result<&str, String> {
    path.to_str().ok_or_else(|| "路径非 UTF-8".to_string())
}

fn adapter_version_stats(conn: &Connection) -> Result<(i64, i64), String> {
    let usage = conn
        .query_row(
            "SELECT COUNT(*) FROM ingested_files WHERE adapter_version != ?1",
            params![store::ADAPTER_VERSION],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let conversation = conn
        .query_row(
            "SELECT COUNT(*) FROM conversation_session_files WHERE adapter_version != 0 AND adapter_version != 11",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok((usage, conversation))
}

fn apply_scenario(
    conn: &Connection,
    scenario: Scenario,
    target: &TargetFile,
) -> Result<(), String> {
    match scenario {
        Scenario::WarmSkip => Ok(()),
        Scenario::DirtyUsage => {
            conn.execute(
                "UPDATE ingested_files SET size = 0 WHERE path = ?1",
                params![target.path],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        }
        Scenario::DirtyConversation | Scenario::DirtyBoth => {
            if matches!(scenario, Scenario::DirtyBoth) {
                conn.execute(
                    "UPDATE ingested_files SET size = 0 WHERE path = ?1",
                    params![target.path],
                )
                .map_err(|e| e.to_string())?;
            }
            conn.execute(
                "UPDATE conversation_session_files
                 SET source_file_size = 0,
                     source_file_mtime_ns = 0,
                     source_revision = '',
                     indexed_byte_offset = 0,
                     indexed_line = 0
                 WHERE source = 'codex' AND session_id = ?1",
                params![target.session_id],
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE conversation_sessions
                 SET source_file_size = 0,
                     source_file_mtime_ns = 0,
                     source_revision = ''
                 WHERE source = 'codex' AND session_id = ?1",
                params![target.session_id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        }
        Scenario::IncrementalConversation => {
            let rewind = (256 * 1024).min(target.size / 10).max(1);
            let cached_size = target.size - rewind;
            conn.execute(
                "UPDATE conversation_session_files
                 SET source_file_size = ?1,
                     indexed_byte_offset = ?1
                 WHERE source = 'codex' AND session_id = ?2",
                params![cached_size, target.session_id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}

fn target_file_row(
    conn: &Connection,
    target: &TargetFile,
) -> Result<Option<(i64, i64, Option<i64>, i64)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT source_file_size, indexed_byte_offset, max_sequence,
                    (SELECT COUNT(*) FROM conversation_events e
                     WHERE e.source = f.source AND e.session_id = f.session_id)
             FROM conversation_session_files f
             WHERE source = 'codex' AND session_id = ?1
             LIMIT 1",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query(params![target.session_id])
        .map_err(|e| e.to_string())?;
    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        Ok(Some((
            row.get(0).map_err(|e| e.to_string())?,
            row.get(1).map_err(|e| e.to_string())?,
            row.get(2).map_err(|e| e.to_string())?,
            row.get(3).map_err(|e| e.to_string())?,
        )))
    } else {
        Ok(None)
    }
}

fn print_result(
    label: &str,
    report: &IngestReport,
    timings: &IngestPhaseTimings,
    rss: Option<u64>,
) {
    println!("## {label}");
    println!(
        "  total={}  remove_unknown={}  usage={}  cursor={}  conversation={}  rollup={}  commit={}  (ms)",
        timings.total_ms,
        timings.remove_unknown_ms,
        timings.usage_ms,
        timings.cursor_ms,
        timings.conversation_ms,
        timings.rollup_ms,
        timings.commit_ms
    );
    println!(
        "  files_seen={}  files_parsed={}  files_skipped={}  files_failed={}  records_written={}  conversation_issues={}",
        report.files_seen,
        report.files_parsed,
        report.files_skipped,
        report.files_failed,
        report.records_written,
        report.conversation_issues.len()
    );
    if let Some(rss) = rss {
        println!("  rss_mb={rss}");
    }
}

fn rss_mb() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p"])
        .arg(std::process::id().to_string())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let kb: u64 = text.trim().parse().ok()?;
    Some(kb / 1024)
}
