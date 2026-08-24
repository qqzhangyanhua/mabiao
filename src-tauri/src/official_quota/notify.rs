use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::{OfficialQuotaConfig, OfficialQuotaDto, OfficialQuotaFreshness};
use crate::official_quota::load_config;

pub const THRESHOLDS: [u32; 2] = [80, 100];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyKey {
    pub provider: String,
    pub window_kind: String,
    pub resets_at: String,
    #[serde(default)]
    pub notified: Vec<u32>,
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
}

pub fn prepare_notifications(
    state: NotifyState,
    dto: &OfficialQuotaDto,
) -> (NotifyState, Vec<QuotaAlert>) {
    // 总开关同时管住内置和自定义：用户不必在两个地方关提醒。
    if !dto.alerts_enabled {
        return (state, Vec::new());
    }
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
            // 有百分比仍然走 80%/100%；去重键里重置时间为空，充值后再涨到
            // 同一档不会二次提醒——已知缺陷，见 #81 / #88。
            // 内置账号仍然要求重置时间：Cursor Auto 那种长期 100% 的窗口
            // 没有周期，放行会在升级后把一堆陈年满格一次弹完。
            let resets_at = match window.resets_at.as_deref() {
                Some(value) => value,
                None if super::custom::is_custom_id(&row.provider) => "",
                None => continue,
            };
            let crossed = thresholds_to_notify(
                percent,
                existing(&next, &row.provider, &window.kind, resets_at),
            );
            if crossed.is_empty() {
                continue;
            }
            if let Some(highest) = crossed.iter().copied().max() {
                alerts.push(QuotaAlert {
                    provider: row.application.clone(),
                    label: window.label.clone(),
                    threshold: highest,
                    used_percent: percent,
                });
            }
            upsert(&mut next, &row.provider, &window.kind, resets_at, &crossed);
        }
    }
    (next, alerts)
}

pub fn thresholds_to_notify(percent_used: f64, already_notified: &[u32]) -> Vec<u32> {
    THRESHOLDS
        .into_iter()
        .filter(|threshold| {
            percent_used >= f64::from(*threshold) && !already_notified.contains(threshold)
        })
        .collect()
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

fn upsert(
    state: &mut NotifyState,
    provider: &str,
    window_kind: &str,
    resets_at: &str,
    crossed: &[u32],
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
    } else {
        state.entries.push(NotifyKey {
            provider: provider.to_string(),
            window_kind: window_kind.to_string(),
            resets_at: resets_at.to_string(),
            notified: crossed.to_vec(),
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
    let state = load_notify_state(notify_state_path);
    let (next, alerts) = prepare_notifications(state, dto);
    if alerts.is_empty() {
        return Ok(());
    }
    for alert in alerts {
        let body = format!(
            "{} {} 已达 {}%（当前 {:.0}%）",
            alert.provider, alert.label, alert.threshold, alert.used_percent
        );
        send_notification(app, "官方额度提醒", &body);
    }
    save_notify_state(notify_state_path, &next)
}

fn send_notification<R: tauri::Runtime>(app: &tauri::AppHandle<R>, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}
