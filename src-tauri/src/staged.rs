//! 分段摄取的后台编排（ADR 0027）：对话目录阶段、事件补建、启动延后。
//!
//! 线程与 Tauri 事件的胶水放在这里，能脱离运行时测试的状态（单飞加补跑、优先队列、
//! 只触发一次）是下面几个小类型。

use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager};

use crate::domain::{ConversationCatalogProgressDto, IngestIssue};
use crate::ingest::{self, CatalogMode, CatalogProgress};
use crate::{conversation, store, AppState};

pub(crate) const CATALOG_PROGRESS_EVENT: &str = "conversation-catalog-progress";

/// 启动后没人触发过用量阶段（例如只开托盘）时，延后的启动任务最迟在这之后拉起。
const STARTUP_FALLBACK: Duration = Duration::from_secs(60);

/// 补建线程每处理一个会话后让出的间隔，避免和摄取的整份文件读取同时叠高内存峰值。
const BACKFILL_STEP_DELAY: Duration = Duration::from_millis(30);

/// 优先队列最多记住的会话数；用户来回翻阅时丢最旧的。
const PRIORITY_CAPACITY: usize = 32;

/// 全局单飞加一次补跑：运行中再请求只记一个「补跑」标记，不管请求多少次。
#[derive(Debug, Default)]
pub(crate) struct SingleFlight {
    running: bool,
    rerun: bool,
}

impl SingleFlight {
    /// 返回 `true` 表示调用方应当启动一轮；`false` 表示已有一轮在跑，本次请求折成补跑标记。
    pub(crate) fn request(&mut self) -> bool {
        if self.running {
            self.rerun = true;
            false
        } else {
            self.running = true;
            true
        }
    }

    /// 一轮结束时调用。返回 `true` 表示要立即再跑一轮（仍视为在跑）；`false` 表示已空闲。
    pub(crate) fn finish(&mut self) -> bool {
        if self.rerun {
            self.rerun = false;
            true
        } else {
            self.running = false;
            false
        }
    }
}

/// 补建优先队列：最近请求的在最前，同一会话只留一份。
#[derive(Debug, Default)]
pub(crate) struct PriorityQueue {
    keys: Vec<(String, String)>,
}

impl PriorityQueue {
    pub(crate) fn push_front(&mut self, key: (String, String)) {
        self.keys.retain(|existing| existing != &key);
        self.keys.insert(0, key);
        self.keys.truncate(PRIORITY_CAPACITY);
    }

    pub(crate) fn remove(&mut self, key: &(String, String)) {
        self.keys.retain(|existing| existing != key);
    }

    pub(crate) fn snapshot(&self) -> Vec<(String, String)> {
        self.keys.clone()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// 只触发一次的开关，供「首次用量阶段」与「启动 60 秒兜底」两条路径共用。
#[derive(Debug, Default)]
pub(crate) struct OneShot(AtomicBool);

impl OneShot {
    /// 第一次调用返回 `true`，之后都是 `false`。
    pub(crate) fn try_fire(&self) -> bool {
        !self.0.swap(true, Ordering::SeqCst)
    }
}

/// 分段摄取挂在 `AppState` 上的全部可变状态。
#[derive(Default)]
pub(crate) struct StagedState {
    catalog_flight: Mutex<SingleFlight>,
    backfill_running: AtomicBool,
    backfill_queue: Mutex<PriorityQueue>,
    /// 最近一轮对话目录阶段的问题，供设置页诊断读取。
    catalog_issues: Mutex<Vec<IngestIssue>>,
    deferred_startup: OneShot,
}

/// 持锁的临界区都只改几个字段，毒化后数据仍然可用。
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl StagedState {
    pub(crate) fn catalog_issues(&self) -> Vec<IngestIssue> {
        locked(&self.catalog_issues).clone()
    }

    fn store_catalog_issues(&self, issues: Vec<IngestIssue>) {
        *locked(&self.catalog_issues) = issues;
    }
}

pub(crate) fn catalog_progress_dto(
    step: &CatalogProgress,
    issue_count: usize,
) -> ConversationCatalogProgressDto {
    ConversationCatalogProgressDto {
        done: step.done as u32,
        total: step.total as u32,
        source: step.source.map(|source| source.as_str().to_string()),
        finished: step.finished,
        issue_count: issue_count as u32,
    }
}

/// 整个目录阶段失败（拿不到连接、线程异常）时记的那一条问题。
pub(crate) fn catalog_stage_failure(message: String) -> IngestIssue {
    IngestIssue {
        source: String::new(),
        path: String::new(),
        message,
        event_type: None,
        line: None,
    }
}

fn emit_progress(app: &AppHandle, dto: &ConversationCatalogProgressDto) {
    let _ = app.emit(CATALOG_PROGRESS_EVENT, dto);
}

/// 用量阶段提交之后的收尾：预算检查、归还空闲内存。
pub(crate) fn after_usage_stage(app: &AppHandle, state: &AppState, conn: &Connection) {
    let prices = state.effective_prices();
    let _ = crate::budget::check_and_notify(
        app,
        conn,
        &prices,
        &state.budget_path,
        &state.budget_notify_path,
    );
    crate::release_idle_memory(state, conn);
}

/// 后台启动对话目录阶段。已有一轮在跑时只记「补跑」，本轮结束后再跑一轮。
pub(crate) fn start_catalog_stage(app: &AppHandle) {
    if !locked(&app.state::<AppState>().staged.catalog_flight).request() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        run_catalog_stage(&app);
        if !locked(&app.state::<AppState>().staged.catalog_flight).finish() {
            break;
        }
    });
}

fn run_catalog_stage(app: &AppHandle) {
    let state = app.state::<AppState>();
    let state: &AppState = &state;
    let home = ingest::default_home();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        ingest::refresh_conversation_catalogs(
            || state.lock_write(),
            &home,
            CatalogMode::Deferred,
            |step| {
                // 结束事件要带问题数，等返回后由下面统一发。
                if !step.finished {
                    emit_progress(app, &catalog_progress_dto(&step, 0));
                }
            },
        )
    }));
    let issues = match outcome {
        Ok(Ok(issues)) => issues,
        Ok(Err(error)) => vec![catalog_stage_failure(error)],
        Err(_) => vec![catalog_stage_failure("对话目录同步异常中断".to_string())],
    };
    let total = conversation::CONVERSATION_SOURCES.len();
    let finished = CatalogProgress {
        done: total,
        total,
        source: None,
        finished: true,
    };
    let dto = catalog_progress_dto(&finished, issues.len());
    state.staged.store_catalog_issues(issues);
    emit_progress(app, &dto);

    ensure_event_backfill(app);
    crate::spawn_conversation_fts_maintenance(app);
}

/// 确保事件补建线程在跑（单飞）。线程把所有未就绪会话补完后退出。
pub(crate) fn ensure_event_backfill(app: &AppHandle) {
    if app
        .state::<AppState>()
        .staged
        .backfill_running
        .swap(true, Ordering::SeqCst)
    {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| run_event_backfill(&app)));
        let staged = &app.state::<AppState>().staged;
        staged.backfill_running.store(false, Ordering::SeqCst);
        // 线程正准备退出时可能有会话刚入队，先放开单飞位再看一眼队列，不丢请求。
        if outcome.is_ok() && !locked(&staged.backfill_queue).is_empty() {
            ensure_event_backfill(&app);
        }
    });
}

/// 用户打开了一个事件未就绪的会话：插到补建队列最前，并确保补建线程在跑。
/// 详情本身照常走整份解析回退，这里只是让它尽快就绪。
pub(crate) fn prioritize_event_backfill(
    app: &AppHandle,
    conn: &Connection,
    source: &str,
    session_id: &str,
) {
    if !matches!(
        conversation::session_needs_event_backfill(conn, source, session_id),
        Ok(true)
    ) {
        return;
    }
    locked(&app.state::<AppState>().staged.backfill_queue)
        .push_front((source.to_string(), session_id.to_string()));
    ensure_event_backfill(app);
}

fn run_event_backfill(app: &AppHandle) {
    let state = app.state::<AppState>();
    let home = ingest::default_home();
    let mut skipped = BTreeSet::<(String, String)>::new();
    let mut processed_any = false;
    loop {
        let priority = locked(&state.staged.backfill_queue).snapshot();
        let progressed = {
            let Ok(conn) = state.lock_write() else {
                return;
            };
            let result = conversation::backfill_event_index_step_prioritized(
                &conn, &home, &skipped, &priority,
            );
            let progressed = match result {
                Ok(progressed) => {
                    processed_any |= progressed;
                    progressed
                }
                Err((key, error)) => {
                    // 空键是挑选下一个会话的查询本身失败，重试也不会变好。
                    if key.0.is_empty() {
                        eprintln!("对话事件索引补建取下一个会话失败：{error}");
                        return;
                    }
                    eprintln!("对话事件索引补建失败 {}/{}：{error}", key.0, key.1);
                    skipped.insert(key);
                    true
                }
            };
            prune_priority_queue(&state, &conn, &skipped);
            progressed
        };
        if !progressed {
            break;
        }
        std::thread::sleep(BACKFILL_STEP_DELAY);
    }
    // 补建结束：小步合并补建期间产生的小段，再触发全量整理兜底（ADR 0024）。
    if processed_any {
        if let Ok(conn) = state.lock_write() {
            if let Err(error) = store::merge_conversation_fts_step(&conn) {
                eprintln!("对话正文索引补建后小步合并失败：{error}");
            }
            let bytes = store::measure_conversation_index_bytes(&conn);
            let _ = store::store_conversation_index_bytes(&conn, bytes);
        }
        crate::spawn_conversation_fts_maintenance(app);
    }
}

/// 已就绪或补建失败的会话不再留在队列里，否则队列永远不空，线程会反复重启。
fn prune_priority_queue(state: &AppState, conn: &Connection, skipped: &BTreeSet<(String, String)>) {
    let queued = locked(&state.staged.backfill_queue).snapshot();
    for key in queued {
        let pending = !skipped.contains(&key)
            && matches!(
                conversation::session_needs_event_backfill(conn, &key.0, &key.1),
                Ok(true)
            );
        if !pending {
            locked(&state.staged.backfill_queue).remove(&key);
        }
    }
}

/// 事件补建、对话派生缓存迁移、正文倒排维护都要抢写锁，启动时先让用量阶段拿到锁。
/// 首次用量阶段完成或启动 `STARTUP_FALLBACK` 之后，取先到者，只触发一次。
pub(crate) fn trigger_deferred_startup(app: &AppHandle) {
    if !app.state::<AppState>().staged.deferred_startup.try_fire() {
        return;
    }
    ensure_event_backfill(app);
    crate::spawn_conversation_cache_migration(app);
    crate::spawn_conversation_fts_maintenance(app);
}

pub(crate) fn spawn_startup_fallback(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(STARTUP_FALLBACK);
        trigger_deferred_startup(&app);
    });
}
