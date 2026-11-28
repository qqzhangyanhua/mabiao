//! 官方额度统一生命周期编排：纯只读快照 [`snapshot`]、活跃刷新与告警 [`refresh_and_notify`]、
//! 单目标刷新与 `mabiao-quota` CLI 入口。
//!
//! 桌面命令、系统托盘和 CLI 全部经由本模块与 [`QuotaEnv`] 对接，保证：
//! - 纯读（`snapshot` / `get_official_quota` / `mabiao-quota`）在 `open_readonly` 上零写入、
//!   零告警、零状态文件变更，同时在内存里叠加更新的本地 Claude statusline 捕获。
//! - 活跃刷新（`refresh_and_notify`）在数据库锁外并发取数，写完即放锁，最后只评估一次
//!   阈值告警并广播 [`EVENT_UPDATED`] 事件（托盘与主窗口同口径）。

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use super::{
    accounts, apply_failure, apply_fetch_results, apply_success, claude, custom, detect,
    fetch_all_targets_in, fetch_target_forced_in, fetch_target_throttled_in, load_config, notify,
    quota_row_title, resolve_target_with_accounts_in, row_from_stored, visible_rows, QuotaEnv,
    QuotaTarget, ThrottledFetch, STALE_AFTER_MINUTES,
};
use crate::domain::{
    OfficialQuotaConfig, OfficialQuotaDto, OfficialQuotaFreshness, OfficialQuotaProvider,
    OfficialQuotaRow,
};
use crate::store;
use crate::work_notes::ConnectionSource;

/// 刷新或配置变更后广播给主窗口与托盘悬浮面板的全量快照事件名。
pub const EVENT_UPDATED: &str = "official-quota-updated";

/// 告警推送回调：`(alert, title, body)`。
pub type NotifySink<'a> = &'a (dyn Fn(&notify::QuotaAlert, &str, &str) + Sync);
/// 快照更新广播回调（用于 Tauri `app.emit(EVENT_UPDATED, dto)`）。
pub type UpdatedSink<'a> = &'a (dyn Fn(&OfficialQuotaDto) + Sync);

/// 活跃刷新选项：桌面端传入通知与事件回调，CLI 传入 [`RefreshOptions::silent`]。
#[derive(Default)]
pub struct RefreshOptions<'a> {
    /// `None` 时静默跳过告警评估与 `notify_state` 写入（给 `mabiao-quota --refresh` 用）。
    pub notify: Option<NotifySink<'a>>,
    /// 刷新落地后（以及 `stream_progress` 开启时每家落库后）触发。
    pub on_updated: Option<UpdatedSink<'a>>,
    /// 手动刷新时每取完一家立刻落库并推送快照；托盘与 CLI 关掉即可。
    pub stream_progress: bool,
}

impl RefreshOptions<'static> {
    pub const fn silent() -> Self {
        Self {
            notify: None,
            on_updated: None,
            stream_progress: false,
        }
    }
}

/// 纯只读快照：不写 SQLite、不写通知状态、不弹系统提醒。
/// 若本地 `claude_statusline.json` 比库内 Claude 缓存更新，只在内存里叠加展示。
pub fn snapshot(conn: &Connection, env: &QuotaEnv) -> OfficialQuotaDto {
    let config = load_config(&env.paths.config);
    let custom = custom::store::load_providers(&env.paths.custom);
    let accounts = accounts::load_resolved(&env.paths.accounts);
    let logins = accounts::probe_default_logins_in(env, &accounts);
    load_dto_in(conn, env, &config, &custom, &accounts, &logins)
}

/// 托盘专用纯只读快照：复用 [`snapshot`] 后按 `official_quota.json` 的 `hidden_providers` 过滤行。
pub fn snapshot_visible(conn: &Connection, env: &QuotaEnv) -> OfficialQuotaDto {
    let mut dto = snapshot(conn, env);
    dto.rows = visible_rows(dto.rows, &dto.hidden_providers);
    dto
}

/// 用显式注入的 [`QuotaEnv`] 拼装官方额度快照（纯读，含内存 Claude statusline 叠加）。
pub fn load_dto_in(
    conn: &Connection,
    env: &QuotaEnv,
    config: &OfficialQuotaConfig,
    custom: &[custom::ResolvedProvider],
    accounts: &[accounts::ResolvedQuotaAccount],
    logins: &[accounts::LocalLogin],
) -> OfficialQuotaDto {
    let now = env.now();
    let accounts = accounts::with_live_profiles(accounts);
    let mut rows = Vec::new();
    for provider in OfficialQuotaProvider::ALL {
        match accounts::plan::plan_provider(provider, &accounts, logins) {
            accounts::plan::ProviderPlan::Unchanged => {
                let row = load_builtin_row_in(conn, env, provider, now);
                if !row.windows.is_empty()
                    || row.captured_at.is_some()
                    || detect::has_local_credentials_in(env, provider)
                {
                    rows.push(row);
                }
            }
            accounts::plan::ProviderPlan::Managed {
                show_builtin,
                placements,
            } => {
                if show_builtin {
                    let mut row = load_builtin_row_in(conn, env, provider, now);
                    row.is_active_local = true;
                    row.application = quota_row_title(provider.display_name(), None, true);
                    rows.push(row);
                }
                for placement in placements {
                    rows.push(super::load_account_row(
                        conn,
                        placement.account,
                        placement.active_local,
                        now,
                    ));
                }
            }
        }
    }
    let undetected = OfficialQuotaProvider::ALL
        .into_iter()
        .filter(|provider| {
            let slug = provider.as_str();
            let present = rows
                .iter()
                .any(|row| row.provider == slug || row.base_provider.as_deref() == Some(slug));
            let registered = accounts
                .iter()
                .any(|account| account.config.provider == slug);
            !present && !registered
        })
        .map(|provider| provider.as_str().to_string())
        .collect();
    rows.extend(
        custom
            .iter()
            .filter(|provider| provider.config.enabled)
            .map(|provider| super::load_custom_row(conn, provider, now)),
    );
    OfficialQuotaDto {
        rows,
        alerts_enabled: config.alerts_enabled,
        stale_after_minutes: STALE_AFTER_MINUTES,
        undetected,
        hidden_providers: config.hidden_providers.clone(),
    }
}

fn load_builtin_row_in(
    conn: &Connection,
    env: &QuotaEnv,
    provider: OfficialQuotaProvider,
    now: DateTime<Utc>,
) -> OfficialQuotaRow {
    let id = provider.as_str();
    let display_name = provider.display_name();
    match store::load_official_quota_row(conn, id) {
        Ok(stored) => {
            let stored = if provider == OfficialQuotaProvider::Claude {
                overlay_claude_capture(stored, &env.paths.claude_capture)
            } else {
                stored
            };
            match stored {
                Some(row) => row_from_stored(id, display_name, row, now),
                None => super::empty_row(id, display_name, None),
            }
        }
        Err(error) => super::empty_row(id, display_name, Some(error)),
    }
}

/// 纯内存叠加更新的 `claude_statusline.json`，不写数据库；保留上一拍用于撞线估算。
fn overlay_claude_capture(
    stored: Option<store::StoredOfficialQuota>,
    capture_path: &Path,
) -> Option<store::StoredOfficialQuota> {
    if !capture_path.exists() {
        return stored;
    }
    let Ok(file_stamp) = claude::file_captured_at(capture_path) else {
        return stored;
    };
    let cached_stamp = stored
        .as_ref()
        .map(|row| row.captured_at.as_str())
        .unwrap_or("");
    if !is_capture_newer(&file_stamp, cached_stamp) {
        return stored;
    }
    match claude::refresh_from_capture(capture_path) {
        Ok((windows, captured_at)) => {
            let (prev_windows, prev_captured_at, plan) = match stored {
                Some(row) => {
                    if !row.captured_at.is_empty()
                        && row.captured_at != captured_at
                        && !row.windows.is_empty()
                    {
                        (row.windows, Some(row.captured_at), row.plan)
                    } else {
                        (row.prev_windows, row.prev_captured_at, row.plan)
                    }
                }
                None => (Vec::new(), None, None),
            };
            Some(store::StoredOfficialQuota {
                windows,
                captured_at,
                error: None,
                plan,
                prev_windows,
                prev_captured_at,
            })
        }
        Err(error) => match stored {
            Some(mut row) => {
                row.error = Some(error);
                Some(row)
            }
            None => Some(store::StoredOfficialQuota {
                windows: Vec::new(),
                captured_at: String::new(),
                error: Some(error),
                plan: None,
                prev_windows: Vec::new(),
                prev_captured_at: None,
            }),
        },
    }
}

/// 只有捕获文件修改时间严格晚于缓存时刻（或缓存时刻缺失/损坏）时才认定捕获更新。
pub(crate) fn is_capture_newer(file_stamp: &str, cached_stamp: &str) -> bool {
    if cached_stamp.is_empty() {
        return true;
    }
    match (
        DateTime::parse_from_rfc3339(file_stamp),
        DateTime::parse_from_rfc3339(cached_stamp),
    ) {
        (Ok(capture), Ok(cached)) => capture.with_timezone(&Utc) > cached.with_timezone(&Utc),
        (Ok(_), Err(_)) => true,
        _ => false,
    }
}

/// 捕获文件比库内缓存严格更新时写入 sqlite，返回是否发生了更新。
pub fn sync_claude_capture_at(conn: &Connection, capture_path: &Path) -> Result<bool, String> {
    if !capture_path.exists() {
        return Ok(false);
    }
    let cached = store::load_official_quota_row(conn, OfficialQuotaProvider::Claude.as_str())?;
    let file_stamp = claude::file_captured_at(capture_path)?;
    let cached_stamp = cached
        .as_ref()
        .map(|row| row.captured_at.as_str())
        .unwrap_or("");
    if !is_capture_newer(&file_stamp, cached_stamp) {
        return Ok(false);
    }
    match claude::refresh_from_capture(capture_path) {
        Ok((windows, captured_at)) => {
            apply_success(conn, OfficialQuotaProvider::Claude, windows, &captured_at)?;
            Ok(true)
        }
        Err(error) => {
            apply_failure(conn, OfficialQuotaProvider::Claude, &error)?;
            Ok(false)
        }
    }
}

/// 活跃刷新全量目标：锁外并发拉取 → 落库 + 同步 Claude statusline 捕获 →
/// 放锁后读快照 → 评估一次阈值告警 → 广播 `official-quota-updated`。
pub fn refresh_and_notify(
    conns: &(dyn ConnectionSource + Sync),
    env: &QuotaEnv,
    options: RefreshOptions<'_>,
) -> Result<OfficialQuotaDto, String> {
    let custom = custom::store::load_providers(&env.paths.custom);
    let accounts = accounts::load_resolved(&env.paths.accounts);
    let logins = accounts::probe_default_logins_in(env, &accounts);

    let publishing = Mutex::new(());
    let persisted = Mutex::new(HashSet::<String>::new());

    let results = fetch_all_targets_in(env, &custom, &accounts, &logins, |target, result| {
        if !options.stream_progress {
            return;
        }
        let _serial = publishing.lock().unwrap_or_else(PoisonError::into_inner);
        let id = target.quota_id().to_string();
        let wrote = {
            let Ok(conn) = conns.write() else {
                return;
            };
            apply_fetch_results(&conn, [(id.clone(), result.clone())]).is_ok()
        };
        if wrote {
            persisted
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id);
            if let Some(on_updated) = options.on_updated {
                if let Ok(conn) = conns.read() {
                    let dto = snapshot(&conn, env);
                    drop(conn);
                    on_updated(&dto);
                }
            }
        }
    });

    let persisted = persisted
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    let leftover: Vec<_> = results
        .into_iter()
        .filter(|(target, _)| !persisted.contains(target.quota_id()))
        .collect();
    {
        let conn = conns.write()?;
        let _ = sync_claude_capture_at(&conn, &env.paths.claude_capture);
        apply_fetch_results(&conn, leftover)?;
    }

    finalize_after_write(conns, env, &options)
}

/// 单目标受退避约束刷新：冷却期短路不写库、不发通知，仅把提示挂在返回快照上。
pub fn refresh_target_throttled(
    conns: &dyn ConnectionSource,
    env: &QuotaEnv,
    id: &str,
    options: RefreshOptions<'_>,
) -> Result<OfficialQuotaDto, String> {
    let custom = custom::store::load_providers(&env.paths.custom);
    let accounts = accounts::load_resolved(&env.paths.accounts);
    let logins = accounts::probe_default_logins_in(env, &accounts);
    let target = resolve_target_with_accounts_in(env, id, &custom, &accounts, &logins)?;
    match fetch_target_throttled_in(env, &target) {
        ThrottledFetch::Cooldown(message) => {
            let conn = conns.read()?;
            let mut dto = snapshot(&conn, env);
            drop(conn);
            overlay_cooldown_message(
                &mut dto,
                target.quota_id(),
                target.quota_display_name(),
                message,
            );
            Ok(dto)
        }
        ThrottledFetch::Attempted(result) => {
            {
                let conn = conns.write()?;
                apply_fetch_results(&conn, [(target, result)])?;
            }
            finalize_after_write(conns, env, &options)
        }
    }
}

/// 单目标强制刷新（托盘悬浮窗用）：跳过冷却检查，结果仍记入退避并评估告警/广播快照。
pub fn refresh_target_forced(
    conns: &dyn ConnectionSource,
    env: &QuotaEnv,
    id: &str,
    options: RefreshOptions<'_>,
) -> Result<OfficialQuotaDto, String> {
    let custom = custom::store::load_providers(&env.paths.custom);
    let accounts = accounts::load_resolved(&env.paths.accounts);
    let logins = accounts::probe_default_logins_in(env, &accounts);
    let target = resolve_target_with_accounts_in(env, id, &custom, &accounts, &logins)?;
    let result = fetch_target_forced_in(env, &target);
    {
        let conn = conns.write()?;
        apply_fetch_results(&conn, [(target, result)])?;
    }
    finalize_after_write(conns, env, &options)
}

/// 保存或导入一条启用账号后立刻取一次；失败落进该行 error，不让保存本身失败。
pub fn refresh_saved_account(
    conns: &dyn ConnectionSource,
    env: &QuotaEnv,
    id: &str,
    on_updated: Option<UpdatedSink<'_>>,
) {
    let custom = custom::store::load_providers(&env.paths.custom);
    let accounts = accounts::load_resolved(&env.paths.accounts);
    let logins = accounts::probe_default_logins_in(env, &accounts);
    let Ok(target) = resolve_target_with_accounts_in(env, id, &custom, &accounts, &logins) else {
        return;
    };
    let result = match fetch_target_throttled_in(env, &target) {
        ThrottledFetch::Cooldown(_) => return,
        ThrottledFetch::Attempted(result) => result,
    };
    {
        let Ok(conn) = conns.write() else {
            return;
        };
        let _ = apply_fetch_results(&conn, [(target, result)]);
    }
    if let Some(on_updated) = on_updated {
        if let Ok(conn) = conns.read() {
            let dto = snapshot(&conn, env);
            drop(conn);
            on_updated(&dto);
        }
    }
}

fn finalize_after_write(
    conns: &dyn ConnectionSource,
    env: &QuotaEnv,
    options: &RefreshOptions<'_>,
) -> Result<OfficialQuotaDto, String> {
    let dto = {
        let conn = conns.read()?;
        snapshot(&conn, env)
    };
    if let Some(notify_sink) = options.notify {
        let config = load_config(&env.paths.config);
        notify::check_and_notify_with_sink(
            &dto,
            &config,
            &env.paths.notify_state,
            env.now(),
            |alert, title, body| notify_sink(alert, title, body),
        )?;
    }
    if let Some(on_updated) = options.on_updated {
        on_updated(&dto);
    }
    Ok(dto)
}

fn overlay_cooldown_message(
    dto: &mut OfficialQuotaDto,
    id: &str,
    display_name: &str,
    message: String,
) {
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
            plan: None,
            base_provider: None,
            account_label: None,
            is_active_local: false,
        }),
    }
}

struct FileConnectionSource {
    read: Mutex<Connection>,
    write: Mutex<Connection>,
}

impl FileConnectionSource {
    fn open(path: &str) -> Result<Self, String> {
        let write = store::open_db(path)?;
        let read = store::open_readonly(path)?;
        Ok(Self {
            read: Mutex::new(read),
            write: Mutex::new(write),
        })
    }
}

impl ConnectionSource for FileConnectionSource {
    fn read(&self) -> Result<MutexGuard<'_, Connection>, String> {
        Ok(self.read.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn write(&self) -> Result<MutexGuard<'_, Connection>, String> {
        Ok(self.write.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// `mabiao-quota` CLI 的核心执行入口。
/// - `refresh = false`：只开 `open_readonly`，走纯 [`snapshot`]（内存叠加新捕获，不写库不告警）。
/// - `refresh = true`：走 [`refresh_and_notify`]（持久化取数与新捕获，静默不弹桌面通知）。
pub fn run_cli_in(
    db_path: &Path,
    env: &QuotaEnv,
    refresh: bool,
) -> Result<OfficialQuotaDto, String> {
    if !db_path.exists() {
        return Ok(OfficialQuotaDto {
            rows: Vec::new(),
            alerts_enabled: false,
            stale_after_minutes: STALE_AFTER_MINUTES,
            undetected: Vec::new(),
            hidden_providers: Vec::new(),
        });
    }
    let path = db_path.to_string_lossy().to_string();
    if refresh {
        let conns = FileConnectionSource::open(&path)?;
        refresh_and_notify(&conns, env, RefreshOptions::silent())
    } else {
        let conn = store::open_readonly(&path)?;
        Ok(snapshot(&conn, env))
    }
}
