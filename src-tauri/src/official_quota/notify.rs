use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::{
    normalize_alert_thresholds, OfficialQuotaConfig, OfficialQuotaDto, OfficialQuotaFreshness,
};
use crate::official_quota::load_config;

pub const THRESHOLDS: [u32; 2] = [80, 100];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaAlertKind {
    Threshold,
    ResetSoon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyKey {
    pub provider: String,
    pub window_kind: String,
    pub resets_at: String,
    #[serde(default)]
    pub notified: Vec<u32>,
    #[serde(default)]
    pub renew_reminded: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyState {
    #[serde(default)]
    pub entries: Vec<NotifyKey>,
}

pub fn load_notify_state(path: &Path) -> NotifyState {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_notify_state(path: &Path, state: &NotifyState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuotaAlert {
    pub provider: String,
    pub label: String,
    pub threshold: u32,
    pub used_percent: f64,
    pub kind: QuotaAlertKind,
}

pub fn prepare_notifications(
    state: NotifyState,
    dto: &OfficialQuotaDto,
) -> (NotifyState, Vec<QuotaAlert>) {
    prepare_notifications_with(state, dto, &OfficialQuotaConfig::default(), Utc::now())
}

pub fn prepare_notifications_with(
    state: NotifyState,
    dto: &OfficialQuotaDto,
    config: &OfficialQuotaConfig,
    now: DateTime<Utc>,
) -> (NotifyState, Vec<QuotaAlert>) {
    // 总开关同时管住内置和自定义：用户不必在两个地方关提醒。
    if !dto.alerts_enabled || !config.alerts_enabled {
        return (state, Vec::new());
    }
    let thresholds = normalize_alert_thresholds(&config.alert_thresholds);
    let mut next = state;
    let mut alerts = Vec::new();
    for row in &dto.rows {
        if row.freshness != OfficialQuotaFreshness::Official {
            continue;
        }
        for window in &row.windows {
            let Some(percent) = window.used_percent else {
                continue;
            };
            // 自定义提供商常常给不出重置时间（OpenAI 兼容计费就是这样）。
            // 有百分比仍然走阈值档；去重键里重置时间为空，充值后再涨到
            // 同一档不会二次提醒——已知缺陷，见 #81 / #88。
            // 内置账号仍然要求重置时间：Cursor Auto 那种长期 100% 的窗口
            // 没有周期，放行会在升级后把一堆陈年满格一次弹完。
            let resets_at = match window.resets_at.as_deref() {
                Some(value) => value,
                None if super::custom::is_custom_id(&row.provider) => "",
                None => continue,
            };
            let crossed = thresholds_to_notify_with(
                percent,
                existing(&next, &row.provider, &window.kind, resets_at),
                &thresholds,
            );
            if let Some(highest) = crossed.iter().copied().max() {
                alerts.push(QuotaAlert {
                    provider: row.application.clone(),
                    label: window.label.clone(),
                    threshold: highest,
                    used_percent: percent,
                    kind: QuotaAlertKind::Threshold,
                });
                upsert(
                    &mut next,
                    &row.provider,
                    &window.kind,
                    resets_at,
                    &crossed,
                    false,
                );
            }
            if should_remind_reset(config, window.kind.as_str(), percent, resets_at, now)
                && !renew_reminded(&next, &row.provider, &window.kind, resets_at)
            {
                alerts.push(QuotaAlert {
                    provider: row.application.clone(),
                    label: window.label.clone(),
                    threshold: 0,
                    used_percent: percent,
                    kind: QuotaAlertKind::ResetSoon,
                });
                upsert(&mut next, &row.provider, &window.kind, resets_at, &[], true);
            }
        }
    }
    (next, alerts)
}

pub fn thresholds_to_notify(percent_used: f64, already_notified: &[u32]) -> Vec<u32> {
    thresholds_to_notify_with(percent_used, already_notified, &THRESHOLDS)
}

fn thresholds_to_notify_with(
    percent_used: f64,
    already_notified: &[u32],
    thresholds: &[u32],
) -> Vec<u32> {
    thresholds
        .iter()
        .copied()
        .filter(|threshold| {
            percent_used >= f64::from(*threshold) && !already_notified.contains(threshold)
        })
        .collect()
}

/// 5 小时窗重置太勤，不当「还剩很多」提醒。周 / 月 / 预算窗才提醒。
pub fn is_short_quota_window(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind.contains("5h") || kind.contains("five_hour") || kind.starts_with("session")
}

fn should_remind_reset(
    config: &OfficialQuotaConfig,
    kind: &str,
    used_percent: f64,
    resets_at: &str,
    now: DateTime<Utc>,
) -> bool {
    if config.reset_reminder_hours == 0 || resets_at.is_empty() || is_short_quota_window(kind) {
        return false;
    }
    if used_percent >= f64::from(config.reset_reminder_max_used_percent) {
        return false;
    }
    let Ok(reset) = DateTime::parse_from_rfc3339(resets_at) else {
        return false;
    };
    let reset = reset.with_timezone(&Utc);
    let remaining = reset - now;
    remaining > Duration::zero()
        && remaining <= Duration::hours(i64::from(config.reset_reminder_hours))
}

fn existing<'a>(
    state: &'a NotifyState,
    provider: &str,
    window_kind: &str,
    resets_at: &str,
) -> &'a [u32] {
    state
        .entries
        .iter()
        .find(|entry| {
            entry.provider == provider
                && entry.window_kind == window_kind
                && entry.resets_at == resets_at
        })
        .map(|entry| entry.notified.as_slice())
        .unwrap_or(&[])
}

fn renew_reminded(state: &NotifyState, provider: &str, window_kind: &str, resets_at: &str) -> bool {
    state.entries.iter().any(|entry| {
        entry.provider == provider
            && entry.window_kind == window_kind
            && entry.resets_at == resets_at
            && entry.renew_reminded
    })
}

fn upsert(
    state: &mut NotifyState,
    provider: &str,
    window_kind: &str,
    resets_at: &str,
    crossed: &[u32],
    renew: bool,
) {
    if let Some(entry) = state.entries.iter_mut().find(|entry| {
        entry.provider == provider
            && entry.window_kind == window_kind
            && entry.resets_at == resets_at
    }) {
        for threshold in crossed {
            if !entry.notified.contains(threshold) {
                entry.notified.push(*threshold);
            }
        }
        entry.renew_reminded |= renew;
    } else {
        state.entries.push(NotifyKey {
            provider: provider.to_string(),
            window_kind: window_kind.to_string(),
            resets_at: resets_at.to_string(),
            notified: crossed.to_vec(),
            renew_reminded: renew,
        });
    }
}

pub fn check_and_notify<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    dto: &OfficialQuotaDto,
    config_path: &Path,
    notify_state_path: &Path,
) -> Result<(), String> {
    let config = load_config(config_path);
    check_and_notify_with_config(app, dto, &config, notify_state_path)
}

pub fn check_and_notify_with_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    dto: &OfficialQuotaDto,
    config: &OfficialQuotaConfig,
    notify_state_path: &Path,
) -> Result<(), String> {
    if !config.alerts_enabled {
        return Ok(());
    }
    // 读-判-写这份状态文件必须串行：读快照不再拿数据库写锁，几路并发进来会各自
    // 读到同一份旧状态，同一条提醒弹好几次。
    static NOTIFY_STATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _serial = NOTIFY_STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let state = load_notify_state(notify_state_path);
    let (next, alerts) = prepare_notifications_with(state, dto, config, Utc::now());
    if alerts.is_empty() {
        return Ok(());
    }
    for alert in alerts {
        let body = match alert.kind {
            QuotaAlertKind::Threshold => format!(
                "{} {} 已达 {}%（当前 {:.0}%）",
                alert.provider, alert.label, alert.threshold, alert.used_percent
            ),
            QuotaAlertKind::ResetSoon => format!(
                "{} {} 还剩 {:.0}% 没用，即将重置",
                alert.provider,
                alert.label,
                100.0 - alert.used_percent
            ),
        };
        send_notification(app, "官方额度提醒", &body);
    }
    save_notify_state(notify_state_path, &next)
}

fn send_notification<R: tauri::Runtime>(app: &tauri::AppHandle<R>, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}
