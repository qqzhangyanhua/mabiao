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
pub mod fetch;
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

// 取数调度住在 `fetch` 里，但调用方一直是按 `official_quota::…` 引用的，
// 这里原样转出去，免得为一次拆文件把各处调用点全改一遍。
pub use fetch::{
    apply_fetch_results, custom_targets_for_fetch, fetch_all_targets, fetch_in_parallel,
    fetch_provider, fetch_target, fetch_target_forced, fetch_target_throttled, parse_provider,
    resolve_target, FetchTarget, ProviderFetch, QuotaTarget, ThrottledFetch,
};

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
    // 本机没凭证、也没历史缓存的 provider 不占一行——否则家数一多，
    // 界面上全是永远好不了的红字。曾经拉到过数据的仍然保留，避免临时登出就丢历史。
    let mut rows: Vec<OfficialQuotaRow> = OfficialQuotaProvider::ALL
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
        .map(|provider| provider.as_str().to_string())
        .collect();
    // 自定义行不套上面那条「没凭证就不占位」的规则：用户是自己动手登记的，
    // 登记完却看不到那一行，只会以为保存没生效。停用的才不占行。
    rows.extend(
        custom
            .iter()
            .filter(|provider| provider.config.enabled)
            .map(|provider| load_custom_row(conn, provider, now)),
    );
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
    load_row_by_id(conn, provider.as_str(), provider.display_name(), now)
}

/// 自定义行：标识跟着配置走、名称只是展示。改名不改标识，因此缓存照旧命中。
fn load_custom_row(
    conn: &Connection,
    provider: &custom::ResolvedProvider,
    now: DateTime<Utc>,
) -> OfficialQuotaRow {
    let mut row = load_row_by_id(conn, &provider.config.id, &provider.config.name, now);
    attach_missing_secret_todo(&mut row, provider.secret.is_none());
    row
}

/// 缺密钥是待办，不是取数失败。sqlite 里可能残留一句同样的 error（上一轮
/// 刷新写进去的），这里把它挪到 `todo`，避免首页画成红字。
fn attach_missing_secret_todo(row: &mut OfficialQuotaRow, secret_missing: bool) {
    let leftover_todo = row.error.as_deref() == Some(custom::MISSING_SECRET);
    if leftover_todo {
        row.error = None;
    }
    if secret_missing {
        row.todo = Some(custom::MISSING_SECRET.to_string());
    }
}

fn load_row_by_id(
    conn: &Connection,
    id: &str,
    display_name: &str,
    now: DateTime<Utc>,
) -> OfficialQuotaRow {
    match store::load_official_quota_row(conn, id) {
        Ok(Some((windows, captured_at, error))) => {
            let freshness = if windows.is_empty() && captured_at.is_empty() {
                OfficialQuotaFreshness::Unavailable
            } else {
                freshness(&captured_at, now)
            };
            OfficialQuotaRow {
                provider: id.to_string(),
                application: display_name.to_string(),
                windows,
                freshness,
                captured_at: if captured_at.is_empty() {
                    None
                } else {
                    Some(captured_at)
                },
                error,
                todo: None,
            }
        }
        Ok(None) => empty_row(id, display_name, None),
        Err(error) => empty_row(id, display_name, Some(error)),
    }
}

fn empty_row(id: &str, display_name: &str, error: Option<String>) -> OfficialQuotaRow {
    OfficialQuotaRow {
        provider: id.to_string(),
        application: display_name.to_string(),
        windows: Vec::new(),
        freshness: OfficialQuotaFreshness::Unavailable,
        captured_at: None,
        error,
        todo: None,
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

/// 托盘标题上的「最紧一档」。
///
/// 自定义提供商被整体跳过。「最紧」的语义是「最快撞线、撞了会自己重置」；
/// 中转站那种充值制余额是存量不是流量，不充值就永远不回落，一条长期 95% 的余额
/// 会把标题钉死，把每天真正在动的 5 小时窗挤掉。
pub fn tightest_window(dto: &OfficialQuotaDto) -> Option<TightestQuota> {
    let mut best: Option<TightestQuota> = None;
    for row in &dto.rows {
        if custom::is_custom_id(&row.provider) {
            continue;
        }
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
