pub mod adapters;
pub mod aggregate;
pub mod backup;
pub mod billing_window;
pub mod budget;
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

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rusqlite::Connection;
use serde::Deserialize;
use tauri::Manager;

use crate::domain::{
    ApplicationAnalyticsDto, BillingWindowsDto, BudgetConfig, BudgetStatusDto, CodeVolumeSummary,
    ConversationAttachmentContentDto, ConversationDetailDto, ConversationDetailStateDto,
    ConversationEventAnchor, ConversationEventContentDto, ConversationEventPage,
    ConversationExportFormat, ConversationIndexProgressDto, ConversationPage, ConversationQuery,
    ConversationUsagePage, CursorAccountEventPage, CursorAccountEventQuery, CursorAccountUsageDto,
    CursorSessionDetailDto, CursorSessionPage, CursorSessionQuery, CursorSessionSummaryDto, Filter,
    FilterOptions, GlobalInstructionDto, IngestReport, NamedAmount, OfficialQuotaConfig,
    OfficialQuotaDto, OfficialQuotaFreshness, OfficialQuotaHookDto, OfficialQuotaRow, OverviewDto,
    PriceSnapshot, PriceSnapshotMeta, PriceTable, SeriesPoint, SessionRow, Source,
    SourceDiagnostic, UsageCallPage, WorkTimelineDto, WriteUserFileRequest, WriteUserFileResult,
};
use crate::official_quota::QuotaTarget;

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

fn save_prices(path: &PathBuf, prices: &PriceTable) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(prices).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

#[tauri::command]
async fn ingest(app: tauri::AppHandle) -> Result<IngestReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        let report = ingest::ingest_all(&conn, &ingest::default_home())?;
        let prices = state.effective_prices();
        let _ = budget::check_and_notify(
            &app,
            &conn,
            &prices,
            &state.budget_path,
            &state.budget_notify_path,
        );
        release_idle_memory(&state, &conn);
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_overview(app: tauri::AppHandle, filter: Filter) -> Result<OverviewDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::overview(&conn, &filter, &prices)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_billing_windows(
    app: tauri::AppHandle,
    filter: Filter,
) -> Result<BillingWindowsDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::billing_windows(&conn, &filter, &prices, chrono::Utc::now())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_trend(
    app: tauri::AppHandle,
    filter: Filter,
    grain: String,
) -> Result<Vec<SeriesPoint>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::trend(&conn, &filter, &prices, &grain)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_application_analytics(
    app: tauri::AppHandle,
    filter: Filter,
    grain: String,
) -> Result<ApplicationAnalyticsDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        query::application_analytics(&conn, &filter, &grain)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Deserialize)]
struct NamedQuery {
    filter: Filter,
    dimension: String,
}

#[tauri::command]
async fn get_breakdown(
    app: tauri::AppHandle,
    query: NamedQuery,
) -> Result<Vec<NamedAmount>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::breakdown(&conn, &query.filter, &prices, &query.dimension)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_usage_calls_page(
    app: tauri::AppHandle,
    filter: Filter,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<UsageCallPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::usage_calls_page(
            &conn,
            &filter,
            &prices,
            page.unwrap_or(1),
            page_size.unwrap_or(20),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_top_sessions(
    app: tauri::AppHandle,
    filter: Filter,
    limit: Option<usize>,
) -> Result<Vec<SessionRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        query::top_sessions(&conn, &filter, &prices, limit.unwrap_or(20))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 单日工作时间线：只看 `day`（本地日历日 `YYYY-MM-DD`），独立于顶栏范围筛选。
#[tauri::command]
async fn get_work_timeline(app: tauri::AppHandle, day: String) -> Result<WorkTimelineDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        query::work_timeline(&conn, &day)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_filter_options(app: tauri::AppHandle) -> Result<FilterOptions, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        query::filter_options(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_prices(state: tauri::State<AppState>) -> PriceTable {
    load_prices(&state.prices_path)
}

#[tauri::command]
fn save_price_table(state: tauri::State<AppState>, prices: PriceTable) -> Result<(), String> {
    save_prices(&state.prices_path, &prices)
}

/// 当前自然月的预算执行情况：本地估算的月度费用、进度与预测，供设置页展示。
#[tauri::command]
async fn get_budget_status(app: tauri::AppHandle) -> Result<BudgetStatusDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        let config = budget::load_config(&state.budget_path);
        budget::status(&conn, &prices, &config, chrono::Local::now())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn save_budget(state: tauri::State<AppState>, config: BudgetConfig) -> Result<(), String> {
    budget::save_config(&state.budget_path, &config)
}

/// 当前生效的 LiteLLM 价目快照元信息（内置或已刷新）。
#[tauri::command]
fn get_price_snapshot(state: tauri::State<AppState>) -> PriceSnapshotMeta {
    state.snapshot_meta()
}

/// 可选刷新：webview 拉取上游原始 JSON 后交给这里解析、落盘并热更新内存快照。
#[tauri::command]
async fn refresh_price_snapshot(
    app: tauri::AppHandle,
    raw: String,
) -> Result<PriceSnapshotMeta, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let as_of = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let snapshot = litellm::parse_litellm_raw(&raw, &as_of)?;
        if snapshot.entries.is_empty() {
            return Err("解析 LiteLLM 价目失败：未找到任何有效模型单价".to_string());
        }
        litellm::save_snapshot(&state.snapshot_path, &snapshot)?;
        let count = snapshot.entries.len();
        {
            let mut guard = state.snapshot.lock().map_err(|e| e.to_string())?;
            *guard = snapshot;
        }
        Ok(PriceSnapshotMeta {
            as_of,
            source: litellm::SOURCE_NAME.to_string(),
            count,
            bundled: false,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 恢复为内置快照：删除本地缓存并重载内置数据。
#[tauri::command]
async fn reset_price_snapshot(app: tauri::AppHandle) -> Result<PriceSnapshotMeta, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        if state.snapshot_path.exists() {
            std::fs::remove_file(&state.snapshot_path).map_err(|e| e.to_string())?;
        }
        let bundled = litellm::bundled_snapshot();
        let meta = PriceSnapshotMeta {
            as_of: bundled.as_of.clone(),
            source: bundled.source.clone(),
            count: bundled.entries.len(),
            bundled: true,
        };
        {
            let mut guard = state.snapshot.lock().map_err(|e| e.to_string())?;
            *guard = bundled;
        }
        Ok(meta)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 供 webview 拉取上游价目使用的固定地址。
#[tauri::command]
fn get_price_snapshot_url() -> String {
    litellm::SOURCE_URL.to_string()
}

#[tauri::command]
async fn get_source_diagnostics(app: tauri::AppHandle) -> Result<Vec<SourceDiagnostic>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        ingest::source_diagnostics(&conn, &ingest::default_home())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn rebuild_cache(
    app: tauri::AppHandle,
    source: Option<String>,
) -> Result<IngestReport, String> {
    let source = source
        .as_deref()
        .map(|value| Source::parse(value).ok_or_else(|| format!("未知来源：{value}")))
        .transpose()?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        let report = ingest::rebuild_cache(&conn, &ingest::default_home(), source)?;
        release_idle_memory(&state, &conn);
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 永久删除某来源（或全部来源）已归档的记录，供用户在设置页显式清理旧数据。
#[tauri::command]
async fn purge_archived_records(
    app: tauri::AppHandle,
    source: Option<String>,
) -> Result<u64, String> {
    let source = source
        .as_deref()
        .map(|value| Source::parse(value).ok_or_else(|| format!("未知来源：{value}")))
        .transpose()?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        // 删记录和重建预聚合表要么一起生效要么都不生效，中间态被读到就是错的数字。
        let transaction = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let removed = store::purge_archived(&transaction, source)?;
        if removed > 0 {
            store::rebuild_rollup(&transaction)?;
        }
        transaction.commit().map_err(|e| e.to_string())?;
        Ok(removed)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_code_volume(app: tauri::AppHandle) -> Result<CodeVolumeSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let summary = ingest::load_code_volume(&ingest::default_home())?;
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        // 代码量是至今累计口径。这里只取费用标量，不跑带 COUNT DISTINCT 的全量 overview。
        let (cost, unpriced) = query::lifetime_cost(&conn, &prices)?;
        Ok(adapters::cursor::with_cost_roi(summary, cost, unpriced))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_cursor_session_summary(
    app: tauri::AppHandle,
) -> Result<CursorSessionSummaryDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        cursor_session::load_summary(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_cursor_sessions_page(
    app: tauri::AppHandle,
    query: CursorSessionQuery,
) -> Result<CursorSessionPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        cursor_session::sessions_page(&conn, &query)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_cursor_session_detail(
    app: tauri::AppHandle,
    source_file: String,
) -> Result<CursorSessionDetailDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        cursor_session_detail::load_detail(&conn, &ingest::default_home(), &source_file)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_conversation_sessions_page(
    app: tauri::AppHandle,
    query: ConversationQuery,
) -> Result<ConversationPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        conversation::sessions_page_with_prices(&conn, &query, &prices)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_conversation_detail(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
) -> Result<ConversationDetailDto, String> {
    let home = ingest::default_home();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let read = conversation::prepare_detail_read(&conn, &home, &source, &session_id)?;
        drop(conn);
        conversation::finish_prepared_detail(&home, read)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_conversation_events(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    anchor: ConversationEventAnchor,
    limit: Option<u32>,
) -> Result<ConversationEventPage, String> {
    let home = ingest::default_home();
    let limit = limit.unwrap_or(200);
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let read =
            conversation::prepare_events_read(&conn, &home, &source, &session_id, &anchor, limit)?;
        drop(conn);
        conversation::finish_prepared_events(&home, read, &anchor, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_conversation_index_progress(
    app: tauri::AppHandle,
) -> Result<ConversationIndexProgressDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::event_index_progress(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_conversation_usage_records(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    page: u32,
    page_size: u32,
) -> Result<ConversationUsagePage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::usage_records_page(&conn, &source, &session_id, page, page_size)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_conversation_detail_state(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    known_revision: String,
) -> Result<ConversationDetailStateDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::detail_state(
            &conn,
            &ingest::default_home(),
            &source,
            &session_id,
            &known_revision,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_conversation_event_content(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    event_id: String,
) -> Result<ConversationEventContentDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::load_event_content(
            &conn,
            &ingest::default_home(),
            &source,
            &session_id,
            &event_id,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_conversation_attachment(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    attachment_id: String,
) -> Result<ConversationAttachmentContentDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::load_attachment(
            &conn,
            &ingest::default_home(),
            &source,
            &session_id,
            &attachment_id,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_conversation_attachment_thumbnail(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    attachment_id: String,
) -> Result<ConversationAttachmentContentDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::load_attachment_thumbnail(
            &conn,
            &ingest::default_home(),
            &source,
            &session_id,
            &attachment_id,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn export_conversation(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    format: ConversationExportFormat,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = ingest::default_home();
        let default_name = {
            let state = app.state::<AppState>();
            let conn = state.lock_read()?;
            conversation::export_default_name(&conn, &home, &source, &session_id, format)?
        };
        let (label, extensions): (&str, &[&str]) = match format {
            ConversationExportFormat::Markdown => ("Markdown", &["md"]),
            ConversationExportFormat::Json => ("Raw JSON", &["jsonl"]),
        };
        let path = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter(label, extensions)
            .save_file();
        match path {
            Some(path) => {
                let expected_mtime = user_files::observe_mtime(&path)?;
                let state = app.state::<AppState>();
                let conn = state.lock_read()?;
                conversation::write_conversation_export(
                    &conn,
                    &home,
                    &source,
                    &session_id,
                    format,
                    &path,
                    expected_mtime.as_deref(),
                )?;
                Ok(true)
            }
            None => Ok(false),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_cursor_account_events_page(
    app: tauri::AppHandle,
    query: CursorAccountEventQuery,
) -> Result<CursorAccountEventPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        cursor_account::events_page(&conn, &query)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn refresh_cursor_account_usage(
    app: tauri::AppHandle,
) -> Result<CursorAccountUsageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = cursor_account::current_token()?;
        let state = app.state::<AppState>();
        let start_date_ms = {
            let conn = state.lock_read()?;
            cursor_account::incremental_start_ms(&conn)?
        };
        let pages = cursor_account::fetch_refresh_pages(&resolved, start_date_ms);
        let conn = state.lock_write()?;
        cursor_account::apply_fetched_pages(&conn, pages)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_cursor_account_usage(
    app: tauri::AppHandle,
    filter: Option<Filter>,
) -> Result<CursorAccountUsageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        cursor_account::load_summary_filtered(&conn, filter.as_ref())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn has_cursor_session_token() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(cursor_account::has_token)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_cursor_credential_status() -> Result<cursor_account::CursorCredentialStatus, String> {
    tauri::async_runtime::spawn_blocking(cursor_account::credential_status)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn clear_cursor_account_usage(
    app: tauri::AppHandle,
) -> Result<CursorAccountUsageDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        cursor_account::clear_cache(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn app_data_paths(state: &AppState) -> backup::AppDataPaths {
    backup::AppDataPaths {
        db_path: state.db_path.clone(),
        prices_path: state.prices_path.clone(),
        snapshot_path: state.snapshot_path.clone(),
        budget_path: state.budget_path.clone(),
        budget_notify_path: state.budget_notify_path.clone(),
        official_quota_path: state.official_quota_path.clone(),
        official_quota_notify_path: state.official_quota_notify_path.clone(),
    }
}

fn official_quota_snapshot(app: &tauri::AppHandle) -> Result<OfficialQuotaDto, String> {
    let state = app.state::<AppState>();
    let conn = state.lock_write()?;
    let config = official_quota::load_config(&state.official_quota_path);
    let custom = official_quota::custom::store::load_providers(&state.custom_quota_paths);
    let dto = official_quota::load_dto(&conn, &config, &custom, chrono::Utc::now());
    official_quota::notify::check_and_notify_with_config(
        app,
        &dto,
        &config,
        &state.official_quota_notify_path,
    )?;
    Ok(dto)
}

#[tauri::command]
async fn get_global_instructions(
    app: tauri::AppHandle,
    project: Option<String>,
) -> Result<GlobalInstructionDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = dirs::home_dir().ok_or_else(|| "无法确定用户主目录".to_string())?;
        let state = app.state::<AppState>();
        let (recent, usage) = {
            let conn = state.lock_read()?;
            (
                query::recent_projects(&conn)?,
                query::source_token_totals(&conn)?,
            )
        };
        Ok(instructions::scan_for_projects(
            &home,
            project.as_deref(),
            &recent,
            &usage,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn write_global_instruction(
    request: WriteUserFileRequest,
) -> Result<WriteUserFileResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = dirs::home_dir().ok_or_else(|| "无法确定用户主目录".to_string())?;
        crate::user_files::write(
            &home,
            &crate::paths::app_data_dir(),
            std::path::Path::new(&request.abs_path),
            &request.content,
            request.expected_mtime.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_global_instruction(abs_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || instructions::open_in_external_editor(&abs_path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_cursor_instruction_settings() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(instructions::cursor::open_settings)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_official_quota(app: tauri::AppHandle) -> Result<OfficialQuotaDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        {
            let state = app.state::<AppState>();
            let conn = state.lock_write()?;
            let _ = official_quota::sync_claude_capture(&conn);
        }
        official_quota_snapshot(&app)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn refresh_official_quota(app: tauri::AppHandle) -> Result<OfficialQuotaDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let results = official_quota::fetch_all_targets(&load_custom_providers(&app));
        persist_official_quota_fetches(&app, results)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn refresh_official_quota_provider(
    app: tauri::AppHandle,
    provider: String,
) -> Result<OfficialQuotaDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // 先试内置枚举，认不出再回落到自定义通道——`custom:` 那些标识
        // 走的是后一条路，不该再撞上「未知的官方额度账号」。
        let target = official_quota::resolve_target(&provider, &load_custom_providers(&app))?;
        match official_quota::fetch_target_throttled(&target) {
            // 冷却期短路：不写库，避免把上一次真实失败原因换成这句「还要等 N 分
            // 钟」——只在这次响应的快照里临时替换该行 error，够按钮即时反馈用。
            official_quota::ThrottledFetch::Cooldown(message) => overlay_cooldown_message(
                &app,
                target.quota_id(),
                target.quota_display_name(),
                message,
            ),
            official_quota::ThrottledFetch::Attempted(result) => {
                persist_official_quota_fetches(&app, [(target, result)])
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 悬浮额度面板每行的强制刷新：跳过冷却检查，用户点了就是要现在就试一次
/// （托盘弹窗那边空间小，不方便像主窗口那样先弹一句「还要等 N 分钟」再让人
/// 决定要不要硬刷）。结果照样记入退避状态，连续失败仍会拉长下次自动重试。
#[tauri::command]
async fn refresh_official_quota_provider_force(
    app: tauri::AppHandle,
    provider: String,
) -> Result<OfficialQuotaDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = official_quota::resolve_target(&provider, &load_custom_providers(&app))?;
        let result = official_quota::fetch_target_forced(&target);
        persist_official_quota_fetches(&app, [(target, result)])
    })
    .await
    .map_err(|e| e.to_string())?
}

fn load_custom_providers(app: &tauri::AppHandle) -> Vec<official_quota::custom::ResolvedProvider> {
    let state = app.state::<AppState>();
    official_quota::custom::store::load_providers(&state.custom_quota_paths)
}

fn persist_official_quota_fetches<T: official_quota::QuotaTarget>(
    app: &tauri::AppHandle,
    results: impl IntoIterator<Item = (T, official_quota::ProviderFetch)>,
) -> Result<OfficialQuotaDto, String> {
    {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        official_quota::apply_fetch_results(&conn, results)?;
    }
    // snapshot 自己再取锁；这里必须先放下，std::sync::Mutex 不可重入。
    official_quota_snapshot(app)
}

/// 冷却提示只挂在这次返回的快照上，不落库。
fn overlay_cooldown_message(
    app: &tauri::AppHandle,
    id: &str,
    display_name: &str,
    message: String,
) -> Result<OfficialQuotaDto, String> {
    let mut dto = official_quota_snapshot(app)?;
    match dto.rows.iter_mut().find(|row| row.provider == id) {
        Some(row) => row.error = Some(message),
        None => dto.rows.push(OfficialQuotaRow {
            provider: id.to_string(),
            application: display_name.to_string(),
            windows: Vec::new(),
            freshness: OfficialQuotaFreshness::Unavailable,
            captured_at: None,
            error: Some(message),
            todo: None,
        }),
    }
    Ok(dto)
}

#[tauri::command]
fn get_official_quota_hook() -> OfficialQuotaHookDto {
    official_quota::hook::preview(
        &official_quota::hook::default_settings_path(),
        &official_quota::hook::hook_command(),
    )
}

#[tauri::command]
fn apply_official_quota_hook() -> Result<OfficialQuotaHookDto, String> {
    official_quota::hook::apply(
        &official_quota::hook::default_settings_path(),
        &official_quota::hook::hook_command(),
    )
}

#[tauri::command]
fn save_official_quota_config(
    state: tauri::State<AppState>,
    config: OfficialQuotaConfig,
) -> Result<(), String> {
    official_quota::save_config(&state.official_quota_path, &config)
}

#[tauri::command]
fn list_custom_quota_providers(
    state: tauri::State<AppState>,
) -> official_quota::custom::panel::CustomQuotaPanelDto {
    official_quota::custom::panel::list(&state.custom_quota_paths)
}

#[tauri::command]
fn save_custom_quota_provider(
    state: tauri::State<AppState>,
    request: official_quota::custom::panel::SaveCustomQuotaProvider,
) -> Result<official_quota::custom::panel::SavedCustomQuotaDto, String> {
    let saved = official_quota::custom::panel::save(&state.custom_quota_paths, request)?;
    // 用户刚改过这一条（多半正是在轮换密钥或换域名来修上一轮的失败），
    // 旧的退避不该再拦着它：否则保存后那次刷新只会回「刚取数失败，N 分钟后
    // 自动重试」，把刚做完的修复盖掉，用户会以为改了没用。
    official_quota::backoff::clear(&official_quota::backoff::state_path(), &saved.saved_id);
    Ok(saved)
}

#[tauri::command]
fn delete_custom_quota_provider(
    state: tauri::State<AppState>,
    id: String,
) -> Result<official_quota::custom::panel::CustomQuotaPanelDto, String> {
    official_quota::custom::panel::delete(&state.custom_quota_paths, &id)
}

/// base URL 输入框下方那行回显。纯计算、不打网，边打边问也不会有负担。
#[tauri::command]
fn preview_custom_quota_request(
    preset: official_quota::custom::CustomQuotaPreset,
    base_url: String,
) -> official_quota::custom::panel::CustomQuotaRequestPreviewDto {
    official_quota::custom::panel::preview_requests(
        preset,
        &base_url,
        chrono::Utc::now().date_naive(),
    )
}

/// 用表单里尚未保存的配置直接打一次，把解析出的额度交回去。
///
/// 走 `custom::fetch` 而不是 `fetch_target_throttled`：这是用户点出来的一次性验证，
/// 既不该被上一轮失败留下的冷却拦住（点测试往往正是为了修好它），也不该把失败
/// 记进退避——那条配置还没保存，退避里没有它的位置。同理不写额度缓存：
/// 首页那一行归已保存的配置管。
#[tauri::command]
async fn test_custom_quota_provider(
    app: tauri::AppHandle,
    request: official_quota::custom::panel::TestCustomQuotaProvider,
) -> Result<official_quota::custom::panel::CustomQuotaTestDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let paths = app.state::<AppState>().custom_quota_paths.clone();
        let secret = official_quota::custom::panel::resolve_secret(&paths, &request)?;
        let (windows, captured_at) =
            official_quota::custom::fetch_quota(request.preset, &request.base_url, Some(&secret))?;
        Ok(official_quota::custom::panel::CustomQuotaTestDto {
            windows,
            captured_at,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 备份 sqlite 与用户配置到用户选择的目录；不含 Cursor 钥匙串 token，也不含自定义提供商密钥。返回 `false` 表示取消。
#[tauri::command]
async fn backup_data(app: tauri::AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dest = rfd::FileDialog::new()
            .set_title("选择备份目录")
            .pick_folder();
        let Some(base) = dest else {
            return Ok(false);
        };
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let dest = base.join(format!("mabiao-{stamp}"));
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        backup::backup_to(&conn, &dest, &app_data_paths(&state))?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 从备份目录恢复 sqlite 与用户配置，覆盖当前缓存。自定义提供商密钥不在备份里，本机已有的密钥文件不会被覆盖。返回 `false` 表示取消。
#[tauri::command]
async fn restore_data(app: tauri::AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let src = rfd::FileDialog::new()
            .set_title("选择备份目录")
            .pick_folder();
        let Some(src) = src else {
            return Ok(false);
        };
        let state = app.state::<AppState>();
        let paths = app_data_paths(&state);
        backup::validate_restore(&src)?;
        {
            let mut conn = state.lock_write()?;
            *conn = store::open_memory()?;
        }
        // 整池切到内存库，把所有指向备份目标文件的只读句柄都放掉。
        state.read_pool.replace_all(store::open_memory)?;
        let restored = backup::restore_from(&src, &paths);
        let db_path = paths.db_path.to_string_lossy().to_string();
        {
            let mut conn = state.lock_write()?;
            *conn = store::open_db(&db_path)?;
        }
        state
            .read_pool
            .replace_all(|| store::open_readonly(&db_path))?;
        restored?;
        let (snapshot, _) = litellm::load_snapshot(&paths.snapshot_path);
        *state.snapshot.lock().map_err(|e| e.to_string())? = snapshot;
        let _ = tray::refresh(&app);
        spawn_event_index_backfill(&app);
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 弹出原生保存对话框并写入 CSV 内容；返回 `false` 表示用户取消。
#[tauri::command]
async fn export_csv(default_name: String, content: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("CSV", &["csv"])
            .save_file();
        match path {
            Some(path) => {
                // UTF-8 BOM 让 Excel 等工具正确识别中文，避免乱码。
                let mut bytes = vec![0xEF, 0xBB, 0xBF];
                bytes.extend_from_slice(content.as_bytes());
                fs::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(true)
            }
            None => Ok(false),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 弹出原生保存对话框并写入 JSON 内容；返回 `false` 表示用户取消。
#[tauri::command]
async fn export_json(default_name: String, content: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("JSON", &["json"])
            .save_file();
        match path {
            Some(path) => {
                fs::write(&path, content.as_bytes()).map_err(|e| e.to_string())?;
                Ok(true)
            }
            None => Ok(false),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn refresh_tray(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || tray::refresh(&app))
        .await
        .map_err(|e| e.to_string())?
}

/// 弹出原生保存对话框并写入图表 PNG（base64 编码）；返回 `false` 表示用户取消。
#[tauri::command]
async fn export_image(default_name: String, base64: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("PNG", &["png"])
            .save_file();
        match path {
            Some(path) => {
                let bytes = BASE64
                    .decode(base64.as_bytes())
                    .map_err(|e| e.to_string())?;
                fs::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(true)
            }
            None => Ok(false),
        }
    })
    .await
    .map_err(|e| e.to_string())?
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
            ping,
            ingest,
            get_overview,
            get_billing_windows,
            get_trend,
            get_application_analytics,
            get_breakdown,
            get_usage_calls_page,
            get_top_sessions,
            get_work_timeline,
            get_filter_options,
            get_prices,
            save_price_table,
            get_budget_status,
            save_budget,
            get_global_instructions,
            write_global_instruction,
            open_global_instruction,
            open_cursor_instruction_settings,
            get_official_quota,
            refresh_official_quota,
            refresh_official_quota_provider,
            refresh_official_quota_provider_force,
            get_official_quota_hook,
            apply_official_quota_hook,
            save_official_quota_config,
            list_custom_quota_providers,
            save_custom_quota_provider,
            delete_custom_quota_provider,
            preview_custom_quota_request,
            test_custom_quota_provider,
            get_price_snapshot,
            get_price_snapshot_url,
            refresh_price_snapshot,
            reset_price_snapshot,
            get_source_diagnostics,
            rebuild_cache,
            purge_archived_records,
            get_code_volume,
            get_cursor_session_summary,
            get_cursor_sessions_page,
            get_cursor_session_detail,
            get_conversation_sessions_page,
            get_conversation_detail,
            get_conversation_events,
            get_conversation_index_progress,
            get_conversation_usage_records,
            get_conversation_detail_state,
            get_conversation_event_content,
            get_conversation_attachment,
            get_conversation_attachment_thumbnail,
            export_conversation,
            refresh_cursor_account_usage,
            get_cursor_account_usage,
            get_cursor_account_events_page,
            has_cursor_session_token,
            get_cursor_credential_status,
            clear_cursor_account_usage,
            export_csv,
            export_json,
            export_image,
            backup_data,
            restore_data,
            refresh_tray
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
