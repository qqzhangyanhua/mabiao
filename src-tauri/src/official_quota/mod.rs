pub mod antigravity;
pub mod backoff;
pub mod claude;
pub mod claude_usage;
pub mod codex;
pub mod codex_usage;
pub mod copilot;
pub mod cursor;
pub mod detect;
pub mod devin;
pub mod droid;
pub mod grok;
pub(crate) mod grok_grpc;
pub mod hook;
pub mod notify;
pub mod opencode;

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::domain::{
    OfficialQuotaConfig, OfficialQuotaDto, OfficialQuotaFreshness, OfficialQuotaProvider,
    OfficialQuotaRow, OfficialQuotaWindow,
};
use crate::store;

pub const STALE_AFTER_MINUTES: i64 = 10;
pub const CONFIG_NAME: &str = "official_quota.json";
pub const NOTIFY_NAME: &str = "official_quota_notify_state.json";
pub const CAPTURE_NAME: &str = "claude_statusline.json";

pub fn capture_path() -> PathBuf {
    crate::paths::app_data_dir().join(CAPTURE_NAME)
}

pub fn load_config(path: &Path) -> OfficialQuotaConfig {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_config(path: &Path, config: &OfficialQuotaConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub fn freshness(captured_at: &str, now: DateTime<Utc>) -> OfficialQuotaFreshness {
    if captured_at.is_empty() {
        return OfficialQuotaFreshness::Unavailable;
    }
    let Ok(captured) = DateTime::parse_from_rfc3339(captured_at) else {
        return OfficialQuotaFreshness::Unavailable;
    };
    if now - captured.with_timezone(&Utc) > Duration::minutes(STALE_AFTER_MINUTES) {
        OfficialQuotaFreshness::Stale
    } else {
        OfficialQuotaFreshness::Official
    }
}

pub fn load_dto(
    conn: &Connection,
    config: &OfficialQuotaConfig,
    now: DateTime<Utc>,
) -> OfficialQuotaDto {
    // 本机没凭证、也没历史缓存的 provider 不占一行——否则家数一多，
    // 界面上全是永远好不了的红字。曾经拉到过数据的仍然保留，避免临时登出就丢历史。
    let rows: Vec<OfficialQuotaRow> = OfficialQuotaProvider::ALL
        .into_iter()
        .map(|provider| load_row(conn, provider, now))
        .filter(|row| {
            !row.windows.is_empty()
                || row.captured_at.is_some()
                || OfficialQuotaProvider::parse(&row.provider)
                    .is_some_and(detect::has_local_credentials)
        })
        .collect();
    let undetected = OfficialQuotaProvider::ALL
        .into_iter()
        .filter(|provider| !rows.iter().any(|row| row.provider == provider.as_str()))
        .map(|provider| provider.display_name().to_string())
        .collect();
    OfficialQuotaDto {
        rows,
        alerts_enabled: config.alerts_enabled,
        stale_after_minutes: STALE_AFTER_MINUTES,
        undetected,
        hidden_providers: config.hidden_providers.clone(),
    }
}

/// 按 `hidden_providers` 挑出用户还想看的行，托盘额度面板用它瘦身。
/// 独立成纯函数是为了不用 `AppHandle` 就能单测。
pub fn visible_rows(
    rows: Vec<OfficialQuotaRow>,
    hidden_providers: &[String],
) -> Vec<OfficialQuotaRow> {
    if hidden_providers.is_empty() {
        return rows;
    }
    let hidden: std::collections::HashSet<&str> =
        hidden_providers.iter().map(String::as_str).collect();
    rows.into_iter()
        .filter(|row| !hidden.contains(row.provider.as_str()))
        .collect()
}

fn load_row(
    conn: &Connection,
    provider: OfficialQuotaProvider,
    now: DateTime<Utc>,
) -> OfficialQuotaRow {
    match store::load_official_quota_row(conn, provider.as_str()) {
        Ok(Some((windows, captured_at, error))) => {
            let freshness = if windows.is_empty() && captured_at.is_empty() {
                OfficialQuotaFreshness::Unavailable
            } else {
                freshness(&captured_at, now)
            };
            OfficialQuotaRow {
                provider: provider.as_str().to_string(),
                application: provider.display_name().to_string(),
                windows,
                freshness,
                captured_at: if captured_at.is_empty() {
                    None
                } else {
                    Some(captured_at)
                },
                error,
            }
        }
        Ok(None) => empty_row(provider, None),
        Err(error) => empty_row(provider, Some(error)),
    }
}

fn empty_row(provider: OfficialQuotaProvider, error: Option<String>) -> OfficialQuotaRow {
    OfficialQuotaRow {
        provider: provider.as_str().to_string(),
        application: provider.display_name().to_string(),
        windows: Vec::new(),
        freshness: OfficialQuotaFreshness::Unavailable,
        captured_at: None,
        error,
    }
}

pub fn apply_success(
    conn: &Connection,
    provider: OfficialQuotaProvider,
    windows: Vec<OfficialQuotaWindow>,
    captured_at: &str,
) -> Result<(), String> {
    store::upsert_official_quota(conn, provider.as_str(), &windows, captured_at, None)
}

pub fn apply_failure(
    conn: &Connection,
    provider: OfficialQuotaProvider,
    error: &str,
) -> Result<(), String> {
    store::set_official_quota_error(conn, provider.as_str(), error)
}

pub fn parse_provider(value: &str) -> Result<OfficialQuotaProvider, String> {
    OfficialQuotaProvider::parse(value).ok_or_else(|| format!("未知的官方额度账号：{value}"))
}

pub type ProviderFetch = Result<(Vec<OfficialQuotaWindow>, String), String>;

/// 先问官方用量接口（零配置），读不到再回落到 statusline 捕获文件——后者是老路径，
/// 装了 hook 的用户和走第三方代理的用户都还得靠它。两条都没有才报错，
/// 错误信息取自动接口那条，因为那是多数人应该走的路。
/// 先打 ChatGPT 的用量接口（不依赖 CLI 装没装），读不到再拉起 `codex app-server`。
/// 两条都失败时报接口那条：多数人应该走的是它。
fn fetch_codex() -> ProviderFetch {
    match codex_usage::fetch_usage() {
        Ok(result) => Ok(result),
        Err(error) => codex::fetch_rate_limits().map_err(|app_server_error| {
            if app_server_error.contains("未找到 Codex CLI") {
                error
            } else {
                app_server_error
            }
        }),
    }
}

fn fetch_claude() -> ProviderFetch {
    match claude_usage::fetch_usage() {
        Ok(result) => Ok(result),
        Err(error) => claude::refresh_from_capture(&capture_path()).map_err(|capture_error| {
            if capture_error.contains("尚未捕获") {
                // 两条路都没有：多数是第三方代理用户，官方登录态是空的，
                // 提示里把 statusline 这条兜底也说出来，否则只能看到一句读不懂的报错。
                format!("{error}。若使用第三方代理，可在设置页写入 statusline hook 后重试")
            } else {
                capture_error
            }
        }),
    }
}

pub fn fetch_provider(provider: OfficialQuotaProvider) -> ProviderFetch {
    match provider {
        OfficialQuotaProvider::Claude => fetch_claude(),
        OfficialQuotaProvider::Codex => fetch_codex(),
        OfficialQuotaProvider::Cursor => cursor::fetch_usage_summary(),
        OfficialQuotaProvider::Grok => grok::fetch_rate_limits(),
        OfficialQuotaProvider::Droid => droid::fetch_rate_limits(),
        OfficialQuotaProvider::Antigravity => antigravity::fetch_rate_limits(),
        OfficialQuotaProvider::OpenCode => opencode::fetch_usage(),
        OfficialQuotaProvider::Copilot => copilot::fetch_usage(),
        OfficialQuotaProvider::Devin => devin::fetch_usage(),
    }
}

/// 先取数再交给调用方加锁写入，避免在持锁期间打网络。
/// 各家并发取数：串行的话总耗时是求和，实测 5 家 7.2 秒，而单家超时上限是 12~20 秒，
/// 网络一差就能拖到分钟级——而这整段跑在一个阻塞线程里，托盘定时刷新也走这条路。
/// 并发之后总耗时变成取最大值。
///
/// 用作用域线程而不是引 async 运行时：每家之间没有共享状态，退避状态在开始前读、
/// 结束后写，不进线程。结果按 `ALL` 的顺序 join，保证输出稳定。
pub fn fetch_all_providers() -> Vec<(OfficialQuotaProvider, ProviderFetch)> {
    let now = Utc::now();
    let mut state = backoff::load_state(&backoff::state_path());
    let targets: Vec<OfficialQuotaProvider> = OfficialQuotaProvider::ALL
        .into_iter()
        .filter(|provider| detect::has_local_credentials(*provider))
        .filter(|provider| backoff::cooldown_remaining(&state, *provider, now).is_none())
        .collect();

    let results = fetch_in_parallel(targets, fetch_provider);
    record_backoff(&mut state, &results, now);
    results
}

/// 并发跑各家、按传入顺序返回。取数函数作为参数传入，这样调度本身可以脱网测试。
pub fn fetch_in_parallel<F>(
    targets: Vec<OfficialQuotaProvider>,
    fetch: F,
) -> Vec<(OfficialQuotaProvider, ProviderFetch)>
where
    F: Fn(OfficialQuotaProvider) -> ProviderFetch + Sync,
{
    std::thread::scope(|scope| {
        let fetch = &fetch;
        let handles: Vec<_> = targets
            .into_iter()
            .map(|provider| (provider, scope.spawn(move || fetch(provider))))
            .collect();
        handles
            .into_iter()
            .map(|(provider, handle)| {
                // 某一家 panic 不该带走整次刷新，其余结果照常写入。
                let result = handle.join().unwrap_or_else(|_| {
                    Err(format!("{} 取数线程异常退出", provider.display_name()))
                });
                (provider, result)
            })
            .collect()
    })
}

/// 单个 provider 的手动刷新。限流期间也拦——「多点几次」正是让限流恢复更慢的原因，
/// 但要明确告诉用户还要等多久，而不是让按钮看起来没反应。
pub fn fetch_provider_throttled(provider: OfficialQuotaProvider) -> ProviderFetch {
    let now = Utc::now();
    let mut state = backoff::load_state(&backoff::state_path());
    if let Some(message) = backoff::cooldown_message(&state, provider, now) {
        return Err(message);
    }
    let result = fetch_provider(provider);
    record_backoff(
        &mut state,
        std::slice::from_ref(&(provider, result.clone())),
        now,
    );
    result
}

fn record_backoff(
    state: &mut backoff::BackoffState,
    results: &[(OfficialQuotaProvider, ProviderFetch)],
    now: DateTime<Utc>,
) {
    if results.is_empty() {
        return;
    }
    for (provider, result) in results {
        match result {
            Ok(_) => backoff::record_success(state, *provider),
            Err(error) => backoff::record_failure(state, *provider, error, now),
        }
    }
    // 状态写不下去不该让刷新失败，最多是下次少歇一会儿。
    let _ = backoff::save_state(&backoff::state_path(), state);
}

/// 打开总览或手动刷新时尝试更新各路；取数在调用方锁外完成，写入彼此隔离。
pub fn apply_fetch_results(
    conn: &Connection,
    results: impl IntoIterator<
        Item = (
            OfficialQuotaProvider,
            Result<(Vec<OfficialQuotaWindow>, String), String>,
        ),
    >,
) -> Result<(), String> {
    for (provider, result) in results {
        apply_result(conn, provider, result)?;
    }
    Ok(())
}

fn apply_result(
    conn: &Connection,
    provider: OfficialQuotaProvider,
    result: Result<(Vec<OfficialQuotaWindow>, String), String>,
) -> Result<(), String> {
    match result {
        Ok((windows, captured_at)) => apply_success(conn, provider, windows, &captured_at),
        Err(error) => apply_failure(conn, provider, &error),
    }
}

/// 捕获文件比缓存新时写入 sqlite，返回是否发生了更新。
pub fn sync_claude_capture(conn: &Connection) -> Result<bool, String> {
    let path = capture_path();
    if !path.exists() {
        return Ok(false);
    }
    let cached = store::load_official_quota_row(conn, OfficialQuotaProvider::Claude.as_str())?;
    let file_stamp = claude::file_captured_at(&path)?;
    if let Some((_, captured_at, _)) = &cached {
        if !captured_at.is_empty() && captured_at == &file_stamp {
            return Ok(false);
        }
    }
    match claude::refresh_from_capture(&path) {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TightestQuota {
    pub provider: String,
    pub label: String,
    pub used_percent: f64,
    pub stale: bool,
}

pub fn tightest_window(dto: &OfficialQuotaDto) -> Option<TightestQuota> {
    let mut best: Option<TightestQuota> = None;
    for row in &dto.rows {
        let stale = match row.freshness {
            OfficialQuotaFreshness::Official => false,
            OfficialQuotaFreshness::Stale => true,
            OfficialQuotaFreshness::Unavailable => continue,
        };
        for window in &row.windows {
            let Some(percent) = window.used_percent else {
                continue;
            };
            let candidate = TightestQuota {
                provider: row.application.clone(),
                label: short_label(&window.kind, &window.label),
                used_percent: percent,
                stale,
            };
            let take = match &best {
                None => true,
                Some(current) => {
                    (!stale && current.stale)
                        || (stale == current.stale && percent > current.used_percent)
                }
            };
            if take {
                best = Some(candidate);
            }
        }
    }
    best
}

fn short_label(kind: &str, label: &str) -> String {
    match kind {
        "session_5h" => "5h".to_string(),
        "weekly" => "7d".to_string(),
        "monthly" => "月".to_string(),
        "billing_cycle" => "总量".to_string(),
        "auto" => "Auto".to_string(),
        "api" => "API".to_string(),
        "on_demand" => "按需".to_string(),
        "product_grokbuild" => "Build".to_string(),
        _ => label.to_string(),
    }
}

pub fn parse_resets_at(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        if DateTime::parse_from_rfc3339(text).is_ok() {
            return Some(if text.ends_with('Z') {
                text.to_string()
            } else {
                DateTime::parse_from_rfc3339(text)
                    .ok()?
                    .with_timezone(&Utc)
                    .to_rfc3339()
            });
        }
        if let Ok(secs) = text.parse::<i64>() {
            return unix_to_rfc3339(secs);
        }
        return None;
    }
    if let Some(secs) = value.as_i64() {
        return unix_to_rfc3339(secs);
    }
    if let Some(secs) = value.as_f64() {
        return unix_to_rfc3339(secs as i64);
    }
    None
}

fn unix_to_rfc3339(raw: i64) -> Option<String> {
    let secs = if raw > 1_000_000_000_000 {
        raw / 1000
    } else {
        raw
    };
    DateTime::from_timestamp(secs, 0).map(|dt| dt.to_rfc3339())
}

pub fn sanitize_percent(value: f64) -> Option<f64> {
    if (0.0..=100.0).contains(&value) {
        Some(value)
    } else {
        None
    }
}
