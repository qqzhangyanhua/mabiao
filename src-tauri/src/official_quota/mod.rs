pub mod accounts;
pub mod antigravity;
pub mod backoff;
pub mod claude;
pub mod claude_usage;
pub mod codex;
pub mod codex_usage;
pub mod copilot;
pub mod cursor;
pub mod custom;
pub mod detect;
pub mod devin;
pub mod droid;
pub mod env;
pub(crate) mod exhaust;
pub mod fetch;
pub mod grok;
pub(crate) mod grok_grpc;
pub mod hook;
pub mod identity;
pub mod lifecycle;
pub mod notify;
pub mod opencode;

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::domain::{
    OfficialQuotaConfig, OfficialQuotaConfigPatch, OfficialQuotaDto, OfficialQuotaFreshness,
    OfficialQuotaProvider, OfficialQuotaRow, OfficialQuotaWindow,
};
use crate::store;

// 取数调度住在 `fetch` 里，但调用方一直是按 `official_quota::…` 引用的，
// 这里原样转出去，免得为一次拆文件把各处调用点全改一遍。
pub use env::{
    HttpBody, HttpMethod, HttpRequest, HttpResponse, IsolatedHttpTransport, LiveQuotaTransport,
    QuotaEnv, QuotaPaths, QuotaTransport,
};
pub use fetch::{
    apply_fetch_results, custom_targets_for_fetch, fetch_all_targets, fetch_all_targets_in,
    fetch_all_targets_with, fetch_all_targets_with_accounts, fetch_in_parallel, fetch_provider,
    fetch_provider_in, fetch_target, fetch_target_forced, fetch_target_forced_in, fetch_target_in,
    fetch_target_throttled, fetch_target_throttled_in, parse_provider, resolve_target,
    resolve_target_with_accounts, resolve_target_with_accounts_in, targets_for_fetch,
    targets_for_fetch_in, FetchTarget, ProviderFetch, QuotaSnapshot, QuotaTarget, ThrottledFetch,
};
pub use identity::{read_identity, read_identity_at, QuotaIdentity};
pub use lifecycle::{
    load_dto_in, refresh_and_notify, refresh_saved_account, refresh_target_forced,
    refresh_target_throttled, run_cli_in, snapshot, snapshot_visible, sync_claude_capture_at,
    NotifySink, RefreshOptions, UpdatedSink, EVENT_UPDATED,
};

pub const STALE_AFTER_MINUTES: i64 = 10;
pub const CONFIG_NAME: &str = "official_quota.json";
pub const NOTIFY_NAME: &str = "official_quota_notify_state.json";
pub const CAPTURE_NAME: &str = "claude_statusline.json";

/// 把生产额度 URL 的路径接到调用方给的根上。`None` 时原样返回。
/// 只给额度账号的离线桩用；本机默认取数不传。
pub(crate) fn rebase_quota_url(url: &str, base: Option<&str>) -> String {
    let Some(base) = base.map(str::trim).filter(|value| !value.is_empty()) else {
        return url.to_string();
    };
    let path = url
        .split_once("://")
        .and_then(|(_, rest)| rest.find('/').map(|index| &rest[index..]))
        .unwrap_or("/");
    format!("{}{path}", base.trim_end_matches('/'))
}

pub fn capture_path() -> PathBuf {
    crate::paths::app_data_dir().join(CAPTURE_NAME)
}

fn config_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    &LOCK
}

pub fn load_config(path: &Path) -> OfficialQuotaConfig {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_config(path: &Path, config: &OfficialQuotaConfig) -> Result<(), String> {
    let _guard = config_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    write_sanitized_config(path, config)?;
    Ok(())
}

/// 原子化局部更新 `official_quota.json`：同一把互斥锁内完成读-合并-清洗-临时文件 rename，
/// 避免主窗口切换显示与设置页改告警档位并发互相覆盖。
pub fn patch_config(
    path: &Path,
    patch: &OfficialQuotaConfigPatch,
) -> Result<OfficialQuotaConfig, String> {
    let _guard = config_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut next = load_config(path);
    if let Some(alerts_enabled) = patch.alerts_enabled {
        next.alerts_enabled = alerts_enabled;
    }
    if let Some(hidden_providers) = &patch.hidden_providers {
        next.hidden_providers = sanitize_hidden_providers(hidden_providers);
    }
    if let Some(alert_thresholds) = &patch.alert_thresholds {
        next.alert_thresholds = alert_thresholds.clone();
    }
    if let Some(reset_reminder_hours) = patch.reset_reminder_hours {
        next.reset_reminder_hours = reset_reminder_hours;
    }
    if let Some(reset_reminder_max_used_percent) = patch.reset_reminder_max_used_percent {
        next.reset_reminder_max_used_percent = reset_reminder_max_used_percent;
    }
    write_sanitized_config(path, &next)
}

fn sanitize_hidden_providers(raw: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for item in raw {
        let trimmed = item.trim();
        if trimmed.is_empty() || custom::is_custom_id(trimmed) {
            continue;
        }
        if !out.iter().any(|existing: &String| existing == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn write_sanitized_config(
    path: &Path,
    config: &OfficialQuotaConfig,
) -> Result<OfficialQuotaConfig, String> {
    let mut sanitized = config.clone();
    sanitized.hidden_providers = sanitize_hidden_providers(&config.hidden_providers);
    sanitized.alert_thresholds =
        crate::domain::normalize_alert_thresholds(&config.alert_thresholds);
    if sanitized.reset_reminder_hours > 168 {
        sanitized.reset_reminder_hours = 168;
    }
    if sanitized.reset_reminder_max_used_percent > 100 {
        sanitized.reset_reminder_max_used_percent = 100;
    }
    let bytes = serde_json::to_string_pretty(&sanitized).map_err(|e| e.to_string())?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| CONFIG_NAME.to_string());
    let temp = parent.join(format!(".{file_name}.tmp"));
    let _ = fs::remove_file(&temp);
    fs::write(&temp, bytes.as_bytes()).map_err(|e| {
        let _ = fs::remove_file(&temp);
        e.to_string()
    })?;
    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        e.to_string()
    })?;
    Ok(sanitized)
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

/// 官方额度的唯一出口：首页、托盘、告警都从这里取数据。
///
/// 内置 9 家与自定义提供商在这里合流——放在这一点意味着下游全部零改动。
/// 自定义行排在内置行之后，顺序按用户在设置页登记的顺序。
pub fn load_dto(
    conn: &Connection,
    config: &OfficialQuotaConfig,
    custom: &[custom::ResolvedProvider],
    now: DateTime<Utc>,
) -> OfficialQuotaDto {
    load_dto_with_accounts(conn, config, custom, &[], &[], now)
}

/// 有额度账号时按账号分行；某个提供方一个账号都没有时，仍是原来的单行。
///
/// `logins` 是离线探到的本机登录。测试传入临时目录上的结果，不读家目录。
pub fn load_dto_with_accounts(
    conn: &Connection,
    config: &OfficialQuotaConfig,
    custom: &[custom::ResolvedProvider],
    accounts: &[accounts::ResolvedQuotaAccount],
    logins: &[accounts::LocalLogin],
    now: DateTime<Utc>,
) -> OfficialQuotaDto {
    lifecycle::assemble_dto(
        conn,
        lifecycle::DtoAssemblyInputs {
            config,
            custom,
            accounts,
            logins,
        },
        now,
        None,
        detect::has_local_credentials,
    )
}

/// 按 `hidden_providers` 挑出用户还想看的行，托盘额度面板用它瘦身。
/// 独立成纯函数是为了不用 `AppHandle` 就能单测。
///
/// 额度账号按归属内置提供方隐藏：关掉 `codex` 会同时藏掉 `acct:codex:…`。
/// 自定义提供商仍只按自己的 `custom:` 标识匹配，不跟着内置开关走。
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
        .filter(|row| !hidden.contains(quota_visibility_id(row)))
        .collect()
}

/// 配置显示和托盘用的提供方键：`base_provider`，否则从 `acct:<slug>:<hex>` 取出 slug。
fn quota_visibility_id(row: &OfficialQuotaRow) -> &str {
    if custom::is_custom_id(&row.provider) {
        return row.provider.as_str();
    }
    if let Some(base) = row.base_provider.as_deref() {
        let base = base.trim();
        if !base.is_empty() {
            return base;
        }
    }
    acct_provider_slug(&row.provider).unwrap_or(row.provider.as_str())
}

/// `acct:<slug>:<hex>` 里的内置 slug。不是这个形状就不是额度账号。
fn acct_provider_slug(id: &str) -> Option<&str> {
    let rest = id.strip_prefix("acct:")?;
    let (slug, hex) = rest.split_once(':')?;
    if slug.is_empty()
        || hex.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        || !hex.chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    Some(slug)
}

/// 额度行标题。没有账号标签时保持提供方显示名，有标签时为「显示名 · 标签」。
pub fn quota_row_application(display_name: &str, account_label: Option<&str>) -> String {
    match account_label
        .map(str::trim)
        .filter(|label| !label.is_empty())
    {
        Some(label) => format!("{display_name} · {label}"),
        None => display_name.to_string(),
    }
}

/// 「当前本机」是后来给额度账号打的标记，不是标题本身。
pub fn quota_active_local_mark(is_active_local: bool) -> Option<&'static str> {
    if is_active_local {
        Some("当前本机")
    } else {
        None
    }
}

/// 行标题：有标签时「显示名 · 标签」，当前本机再补「 · 当前本机」。
pub fn quota_row_title(
    display_name: &str,
    account_label: Option<&str>,
    is_active_local: bool,
) -> String {
    let mut title = quota_row_application(display_name, account_label);
    if quota_active_local_mark(is_active_local).is_some() && !title.ends_with("当前本机") {
        title = format!("{title} · 当前本机");
    }
    title
}

pub(crate) fn load_account_row(
    conn: &Connection,
    account: &accounts::ResolvedQuotaAccount,
    active_local: bool,
    now: DateTime<Utc>,
) -> OfficialQuotaRow {
    let provider = OfficialQuotaProvider::parse(&account.config.provider);
    let display = provider
        .map(OfficialQuotaProvider::display_name)
        .unwrap_or(account.config.provider.as_str());
    let title = quota_row_title(display, Some(&account.config.name), active_local);
    let mut row = load_row_by_id(conn, &account.config.id, &title, now);
    row.base_provider = provider.map(|item| item.as_str().to_string());
    row.account_label = Some(account.config.name.clone());
    row.is_active_local = active_local;
    attach_account_missing_secret(&mut row, account.access_token_missing());
    row
}

/// 缺凭证是待办，不是取数失败。sqlite 里可能残留任意上一轮的 `error`；
/// 只要这一行是缺凭证待办，就把 `error` 清掉，避免首页把待办画成红字。
/// 密钥补上之后，只拿掉残留的 todo 文案。登录失效要等下次取数成功再清。
fn attach_account_missing_secret(row: &mut OfficialQuotaRow, missing: bool) {
    if missing {
        row.error = None;
        row.todo = Some(accounts::MISSING_SECRET_TODO.to_string());
        return;
    }
    if row.error.as_deref() == Some(accounts::MISSING_SECRET_TODO) {
        row.error = None;
    }
}

/// 自定义行：标识跟着配置走、名称只是展示。改名不改标识，因此缓存照旧命中。
pub(crate) fn load_custom_row(
    conn: &Connection,
    provider: &custom::ResolvedProvider,
    now: DateTime<Utc>,
) -> OfficialQuotaRow {
    let mut row = load_row_by_id(conn, &provider.config.id, &provider.config.name, now);
    attach_missing_secret_todo(&mut row, provider.missing_credential_todo());
    row
}

/// 缺密钥是待办，不是取数失败。sqlite 里可能残留一句同样的 error（上一轮
/// 刷新写进去的），这里把它挪到 `todo`，避免首页画成红字。
fn attach_missing_secret_todo(row: &mut OfficialQuotaRow, missing: Option<&str>) {
    let leftover_todo = row.error.as_deref() == Some(custom::MISSING_SECRET)
        || row.error.as_deref() == Some(custom::volcengine_ark::MISSING_ACCESS_KEY);
    if leftover_todo {
        row.error = None;
    }
    if let Some(todo) = missing {
        row.todo = Some(todo.to_string());
    }
}

fn load_row_by_id(
    conn: &Connection,
    id: &str,
    display_name: &str,
    now: DateTime<Utc>,
) -> OfficialQuotaRow {
    match store::load_official_quota_row(conn, id) {
        Ok(Some(stored)) => row_from_stored(id, display_name, stored, now),
        Ok(None) => empty_row(id, display_name, None),
        Err(error) => empty_row(id, display_name, Some(error)),
    }
}

pub(crate) fn row_from_stored(
    id: &str,
    display_name: &str,
    stored: store::StoredOfficialQuota,
    now: DateTime<Utc>,
) -> OfficialQuotaRow {
    let freshness = if stored.windows.is_empty() && stored.captured_at.is_empty() {
        OfficialQuotaFreshness::Unavailable
    } else {
        freshness(&stored.captured_at, now)
    };
    let mut windows = stored.windows;
    exhaust::attach(
        &mut windows,
        &stored.captured_at,
        &stored.prev_windows,
        stored.prev_captured_at.as_deref(),
        now,
    );
    OfficialQuotaRow {
        provider: id.to_string(),
        application: display_name.to_string(),
        windows,
        freshness,
        captured_at: if stored.captured_at.is_empty() {
            None
        } else {
            Some(stored.captured_at)
        },
        error: stored.error,
        todo: None,
        plan: stored.plan,
        base_provider: None,
        account_label: None,
        is_active_local: false,
    }
}

pub(crate) fn empty_row(id: &str, display_name: &str, error: Option<String>) -> OfficialQuotaRow {
    OfficialQuotaRow {
        provider: id.to_string(),
        application: display_name.to_string(),
        windows: Vec::new(),
        freshness: OfficialQuotaFreshness::Unavailable,
        captured_at: None,
        error,
        todo: None,
        plan: None,
        base_provider: None,
        account_label: None,
        is_active_local: false,
    }
}

pub fn apply_success(
    conn: &Connection,
    provider: OfficialQuotaProvider,
    windows: Vec<OfficialQuotaWindow>,
    captured_at: &str,
) -> Result<(), String> {
    store::upsert_official_quota(conn, provider.as_str(), &windows, captured_at, None, None)
}

pub fn apply_failure(
    conn: &Connection,
    provider: OfficialQuotaProvider,
    error: &str,
) -> Result<(), String> {
    store::set_official_quota_error(conn, provider.as_str(), error)
}

/// 捕获文件比缓存新时写入 sqlite，返回是否发生了更新。
pub fn sync_claude_capture(conn: &Connection) -> Result<bool, String> {
    lifecycle::sync_claude_capture_at(conn, &capture_path())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TightestQuota {
    pub provider: String,
    pub label: String,
    pub used_percent: f64,
    pub stale: bool,
}

/// 托盘标题上的「最紧一档」。
///
/// 「最紧」的语义是「最快撞线、撞了会自己重置」。自定义提供商因此按窗口
/// 有无重置时间分流：带重置时间的预算窗口能进标题；充值制余额那种永不回落
/// 的存量仍跳过，免得一条长期 95% 的余额把每天真正在动的 5 小时窗挤掉。
/// 内置账号不受这条过滤——没有重置时间的 Auto 窗也照常参与竞争。
pub fn tightest_window(dto: &OfficialQuotaDto) -> Option<TightestQuota> {
    let mut best: Option<TightestQuota> = None;
    for row in &dto.rows {
        let stale = match row.freshness {
            OfficialQuotaFreshness::Official => false,
            OfficialQuotaFreshness::Stale => true,
            OfficialQuotaFreshness::Unavailable => continue,
        };
        for window in &row.windows {
            if custom::is_custom_id(&row.provider) && window.resets_at.is_none() {
                continue;
            }
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
        "budget_window" => "预算".to_string(),
        "auto" => "Auto".to_string(),
        "api" => "API".to_string(),
        "on_demand" => "按需".to_string(),
        "product_grokbuild" => "Build".to_string(),
        _ => label.to_string(),
    }
}

/// 套餐原值（接口 membershipType / subscription_tier_display / JWT tier）→ 界面展示名。
/// 认不出就原样返回（去空白）；空串当没有。
pub fn display_plan_label(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let key: String = trimmed
        .chars()
        .map(|c| match c {
            '-' | ' ' => '_',
            _ => c,
        })
        .collect::<String>()
        .to_ascii_lowercase();
    let label = match key.as_str() {
        "hobby" | "free" | "free_tier" | "freetier" => "Free",
        "pro" | "google_ai_pro" | "gemini_pro" | "g1_pro_tier" => "Pro",
        "plus" => "Plus",
        "pro_plus" | "proplus" | "pro+" => "Pro+",
        "ultra" | "g1_ultra_tier" | "google_ai_ultra" => "Ultra",
        "max" => "Max",
        "business" => "Business",
        "enterprise" => "Enterprise",
        "team" => "Team",
        "individual" => "Individual",
        "standard" | "standard_tier" => "Standard",
        "legacy_tier" => "Legacy",
        "supergrok" | "grokpro" => "SuperGrok",
        "supergrok_heavy" | "supergrokpro" | "supergrok_pro" => "SuperGrok Heavy",
        "supergrok_lite" | "supergroklite" => "SuperGrok Lite",
        "supergrok_plus" | "supergrokplus" => "SuperGrok Plus",
        "x_basic" | "xbasic" => "X Basic",
        "x_premium" | "xpremium" => "X Premium",
        "x_premium_plus" | "xpremiumplus" | "x_premium+" => "X Premium+",
        "api_key" | "apikey" => "API Key",
        _ => trimmed,
    };
    Some(label.to_string())
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
