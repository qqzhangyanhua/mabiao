pub mod adapters;
pub mod aggregate;
pub mod backup;
pub mod billing_window;
pub mod budget;
pub mod clipboard;
mod commands;
pub mod conversation;
pub mod cost;
pub mod cursor_account;
pub mod cursor_credentials;
pub mod cursor_session;
pub mod cursor_session_detail;
pub mod cursor_session_query;
pub mod domain;
pub mod ingest;
pub mod instructions;
pub mod litellm;
pub mod memory;
pub mod net;
pub mod official_quota;
pub mod paths;
pub mod query;
pub mod report;
pub mod rollup_source;
pub mod rollup_split;
pub mod scan_paths;
pub mod store;
pub mod tray;
pub mod tray_popup;
pub mod user_files;
pub mod vscode_state;
pub mod work_timeline;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;
use tauri::Manager;

use crate::domain::{PriceSnapshot, PriceSnapshotMeta, PriceTable};

/// 只读连接池。
///
/// 首屏一次要发 9 个查询，全走 `lock_read`；共用一把锁就是把它们排成队，而 WAL 本来就
/// 允许多读者并发。
///
/// 大小定在 3。这些查询是对十几万行的全表扫描，瓶颈是内存带宽而非 CPU，并发度拉高只会
/// 互相抢带宽：8 核机器上实测（动态取空闲连接，与本池行为一致）2 条 1.3x、3 条 1.43x、
/// 4 条 1.27x、6 条 1.07x、8 条 0.93x——8 条已经比串行还慢。池比并发请求数小时多出来的
/// 请求会排队，但带宽饱和的前提下排队本就比抢更快。
const READ_POOL_SIZE: usize = 3;

pub struct ReadPool {
    conns: Vec<Mutex<Connection>>,
    next: AtomicUsize,
}

impl ReadPool {
    fn open(path: &str, size: usize) -> Result<Self, String> {
        let conns = (0..size.max(1))
            .map(|_| store::open_readonly(path).map(Mutex::new))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            conns,
            next: AtomicUsize::new(0),
        })
    }

    /// 轮询找一条空闲连接；全忙就阻塞在轮到的那条上（阻塞等待也比多开连接抢带宽划算）。
    fn get(&self) -> Result<MutexGuard<'_, Connection>, String> {
        for _ in 0..self.conns.len() {
            let index = self.next.fetch_add(1, Ordering::Relaxed) % self.conns.len();
            if let Ok(guard) = self.conns[index].try_lock() {
                return Ok(guard);
            }
        }
        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.conns.len();
        self.conns[index].lock().map_err(|e| e.to_string())
    }

    /// 换库时必须整池替换：漏掉任何一条，它的文件句柄还指向被覆盖前的旧库，
    /// 之后的查询会随机读到新旧两份数据。
    fn replace_all(
        &self,
        mut make: impl FnMut() -> Result<Connection, String>,
    ) -> Result<(), String> {
        for slot in &self.conns {
            let mut conn = slot.lock().map_err(|e| e.to_string())?;
            *conn = make()?;
        }
        Ok(())
    }

    fn shrink_memory(&self) {
        for slot in &self.conns {
            if let Ok(conn) = slot.lock() {
                let _ = store::shrink_memory(&conn);
            }
        }
    }
}

pub(crate) fn release_idle_memory(state: &AppState, write: &Connection) {
    let _ = store::shrink_memory(write);
    state.read_pool.shrink_memory();
    memory::release_idle();
}

pub struct AppState {
    pub db_path: PathBuf,
    pub prices_path: PathBuf,
    pub snapshot_path: PathBuf,
    pub budget_path: PathBuf,
    pub budget_notify_path: PathBuf,
    pub official_quota_path: PathBuf,
    pub official_quota_notify_path: PathBuf,
    /// 自定义提供商的配置与密钥。配置可以进备份，密钥不进——备份目录是设计成
    /// 给人整个拷走的。
    pub custom_quota_paths: official_quota::custom::store::CustomQuotaPaths,
    pub conn: Mutex<Connection>,
    pub read_pool: ReadPool,
    pub snapshot: Mutex<PriceSnapshot>,
}

impl AppState {
    /// 生效单价表 = 用户配置的单价 + LiteLLM 快照兜底（用户已配置的模型不被兜底覆盖）。
    /// 所有涉及费用的查询都应经由此方法取价，保证兜底语义在各处一致。
    pub(crate) fn effective_prices(&self) -> PriceTable {
        let user = load_prices(&self.prices_path);
        match self.snapshot.lock() {
            Ok(snapshot) => litellm::merge(&user, &snapshot),
            Err(_) => user,
        }
    }

    pub(crate) fn lock_write(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.conn.lock().map_err(|e| e.to_string())
    }

    pub(crate) fn lock_read(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.read_pool.get()
    }

    fn snapshot_meta(&self) -> PriceSnapshotMeta {
        let bundled = !self.snapshot_path.exists();
        match self.snapshot.lock() {
            Ok(snapshot) => PriceSnapshotMeta {
                as_of: snapshot.as_of.clone(),
                source: snapshot.source.clone(),
                count: snapshot.entries.len(),
                bundled,
            },
            Err(_) => PriceSnapshotMeta {
                as_of: String::new(),
                source: litellm::SOURCE_NAME.to_string(),
                count: 0,
                bundled,
            },
        }
    }
}

fn cache_dir() -> PathBuf {
    crate::paths::app_data_dir()
}

pub(crate) fn load_prices(path: &PathBuf) -> PriceTable {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// 老库首次升级、或从不含预聚合表的旧备份恢复后，把它在后台补建起来。
///
/// 350 万行要十几秒，放在 `setup` 里同步做会让应用启动像卡死。挪到后台线程之后：
/// 补建期间 `rollup_is_ready` 为假，查询自动回退原始表——慢，但数字是对的；
/// 摄取也会跳过增量重建，免得往空表里只写进一两天，让它「非空却残缺」。
/// 补建拿的是写锁，与摄取天然互斥；完成时置就绪位，之后的查询自然切到预聚合表。
fn spawn_rollup_backfill(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let needs = {
            let Ok(conn) = state.lock_read() else {
                return;
            };
            store::rollup_needs_backfill(&conn).unwrap_or(false)
        };
        if !needs {
            return;
        }
        let Ok(conn) = state.lock_write() else {
            return;
        };
        // 再查一次：等锁期间可能已经有别的路径把它建好了。
        if !store::rollup_needs_backfill(&conn).unwrap_or(false) {
            return;
        }
        if let Err(error) = store::backfill_rollup(&conn) {
            eprintln!("预聚合表补建失败，查询将继续走原始表：{error}");
        }
    });
}

/// 升级后按会话渐进补建事件索引，最近结束的先做。
///
/// 不在 `setup` 里同步跑：整库重解析会让启动像卡死。补建期间未就绪的会话走整份解析回退。
/// 每次只拿写锁处理一条，避免长时间挡住摄取。
///
/// 每条会话之间故意 sleep 一下：这条路径和首屏的 `ingest_all` 都要整份读会话源文件，
/// 若同时全速跑，遇到大文件（真实观测到 Codex 单个 rollout 日志有 114MB）会让两条路径
/// 的临时内存峰值叠在一起。让一步不影响正确性——补建本身就是"渐进"的，晚一点做完没关系。
fn spawn_event_index_backfill(app: &tauri::AppHandle) {
    const STEP_DELAY: std::time::Duration = std::time::Duration::from_millis(30);
    let app = app.clone();
    std::thread::spawn(move || {
        let home = ingest::default_home();
        let mut skipped = std::collections::BTreeSet::<(String, String)>::new();
        loop {
            let state = app.state::<AppState>();
            let progressed = {
                let Ok(conn) = state.lock_write() else {
                    return;
                };
                match conversation::backfill_event_index_step_skipping(&conn, &home, &skipped) {
                    Ok(progressed) => progressed,
                    Err((key, error)) => {
                        eprintln!("对话事件索引补建失败 {}/{}：{error}", key.0, key.1);
                        skipped.insert(key);
                        true
                    }
                }
            };
            if !progressed {
                return;
            }
            std::thread::sleep(STEP_DELAY);
        }
    });
}

/// 对话派生缓存的两处形态迁移，都只做一次：
///
/// - 事件表原先每行存整条源文件绝对路径、并且靠一条宽索引回答「用过哪个工具」。换成
///   `file_id` + 工具汇总表之后，真实库省下约 200MB。
/// - 正文倒排原先是 FTS5 默认的 `detail=full`，位置表占了整个缓存里最大的一块，而检索侧
///   从不读位置：片段是 Rust 自己从正文切的，排序键也是手写的。换成 `detail=none` 之后，
///   真实库的倒排从 2.7GB 降到 0.3GB。
///
/// 两件事合计要几分钟（1.1GB 表整份复制 + 倒排重灌），放 `setup` 里同步做会让启动像卡死，
/// 所以和预聚合补建一样挪到后台线程、与摄取靠写锁互斥。期间不需要任何「未就绪」状态：
/// 形态迁移在同一个事务里连带重建倒排，提交之前读连接看到的仍是完整的旧快照。
fn spawn_conversation_cache_migration(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let needs = {
            let Ok(conn) = state.lock_read() else {
                return;
            };
            store::conversation_events_needs_layout_migration(&conn).unwrap_or(false)
                || store::conversation_fts_needs_migration(&conn).unwrap_or(false)
        };
        if !needs {
            return;
        }
        let Ok(conn) = state.lock_write() else {
            return;
        };
        if let Err(error) = store::migrate_conversation_events_layout(&conn) {
            eprintln!("对话事件表形态迁移失败：{error}");
            return;
        }
        if let Err(error) = store::migrate_conversation_events_fts(&conn) {
            eprintln!("对话正文索引迁移失败：{error}");
            return;
        }
        // 换表只是把旧结构的几 GB 页挂进 freelist，文件本身不会变小。新形态的稳态体积只有
        // 三分之一，这些页等不到被写入复用的那天，所以顺手还给文件系统——省磁盘正是这次
        // 改动要交付的东西。失败不回滚：迁移已经提交，空间晚点还不影响正确性。
        if let Err(error) = store::vacuum(&conn) {
            eprintln!("对话派生缓存迁移后回收磁盘空间失败：{error}");
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = cache_dir();
            let db_path = dir.join("usage.sqlite");
            let prices_path = dir.join("prices.json");
            let snapshot_path = dir.join("litellm_prices.json");
            let budget_path = dir.join("budget.json");
            let budget_notify_path = dir.join("budget_notify_state.json");
            let official_quota_path = dir.join(official_quota::CONFIG_NAME);
            let official_quota_notify_path = dir.join(official_quota::NOTIFY_NAME);
            let custom_quota_paths = official_quota::custom::store::CustomQuotaPaths::in_dir(&dir);
            let db_path_str = db_path.to_string_lossy().to_string();
            let conn = store::open_db(&db_path_str).map_err(std::io::Error::other)?;
            // open_db 必须先跑：它建表建索引，只读连接开在空库上会查不到表。
            let read_pool =
                ReadPool::open(&db_path_str, READ_POOL_SIZE).map_err(std::io::Error::other)?;
            let (snapshot, _bundled) = litellm::load_snapshot(&snapshot_path);
            app.manage(AppState {
                db_path,
                prices_path,
                snapshot_path,
                budget_path,
                budget_notify_path,
                official_quota_path,
                official_quota_notify_path,
                custom_quota_paths,
                conn: Mutex::new(conn),
                read_pool,
                snapshot: Mutex::new(snapshot),
            });
            tray::setup(app.handle()).map_err(std::io::Error::other)?;
            spawn_rollup_backfill(app.handle());
            spawn_event_index_backfill(app.handle());
            spawn_conversation_cache_migration(app.handle());
            #[cfg(desktop)]
            {
                use tauri_plugin_notification::NotificationExt;
                let _ = app.notification().request_permission();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == tray_popup::LABEL {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::ingest,
            commands::get_overview,
            commands::get_report,
            commands::get_billing_windows,
            commands::get_trend,
            commands::get_application_analytics,
            commands::get_low_cache_hit_sessions,
            commands::get_breakdown,
            commands::get_usage_calls_page,
            commands::get_top_sessions,
            commands::get_work_timeline,
            commands::get_filter_options,
            commands::get_unpriced_diagnosis,
            commands::get_prices,
            commands::save_price_table,
            commands::get_budget_status,
            commands::save_budget,
            commands::get_global_instructions,
            commands::write_global_instruction,
            commands::open_global_instruction,
            commands::open_cursor_instruction_settings,
            commands::get_official_quota,
            commands::refresh_official_quota,
            commands::refresh_official_quota_provider,
            commands::refresh_official_quota_provider_force,
            commands::get_official_quota_hook,
            commands::apply_official_quota_hook,
            commands::save_official_quota_config,
            commands::list_custom_quota_providers,
            commands::save_custom_quota_provider,
            commands::delete_custom_quota_provider,
            commands::preview_custom_quota_request,
            commands::test_custom_quota_provider,
            commands::get_price_snapshot,
            commands::get_price_snapshot_url,
            commands::refresh_price_snapshot,
            commands::reset_price_snapshot,
            commands::get_source_diagnostics,
            commands::get_scan_path_config,
            commands::save_scan_path_config,
            commands::pick_directory,
            commands::rebuild_cache,
            commands::purge_archived_records,
            commands::get_code_volume,
            commands::get_cursor_session_summary,
            commands::get_cursor_sessions_page,
            commands::get_cursor_session_detail,
            commands::get_conversation_sessions_page,
            commands::get_conversation_tool_names,
            commands::get_conversation_detail,
            commands::get_conversation_events,
            commands::get_conversation_index_progress,
            commands::get_conversation_usage_records,
            commands::get_conversation_detail_state,
            commands::get_conversation_event_content,
            commands::get_conversation_attachment,
            commands::get_conversation_attachment_thumbnail,
            commands::export_conversation,
            commands::refresh_cursor_account_usage,
            commands::get_cursor_account_usage,
            commands::get_cursor_account_events_page,
            commands::has_cursor_session_token,
            commands::get_cursor_credential_status,
            commands::clear_cursor_account_usage,
            commands::export_csv,
            commands::export_json,
            commands::copy_image_to_clipboard,
            commands::export_image,
            commands::backup_data,
            commands::restore_data,
            commands::refresh_tray
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { api, code, .. }
                if code.is_none() && app.get_webview_window("main").is_none() =>
            {
                // None = 关最后一扇窗 / Cmd+Q 等用户交互。主窗口没了就留在托盘；
                // 主窗口还在（典型是 Cmd+Q）则放行，让应用退出。
                // 托盘菜单「退出」走 app.exit(0)，code 是 Some，不会进这里。
                api.prevent_exit();
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => tray::show_main(app),
            _ => {}
        });
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
