//! 月度预算阈值：按自然月（本地时区）汇总费用，达到 50/80/100% 时发本地系统通知。
//! 仅本地估算，非官方账单；不访问网络，不上报数据。

use std::fs;
use std::path::Path;

use chrono::{DateTime, Datelike, Local, NaiveDate, SecondsFormat, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::domain::{BudgetConfig, BudgetStatusDto, Filter, PriceTable};
use crate::query;

/// 达到这些百分比时各提醒一次（按自然月重置）。
pub const THRESHOLDS: [u32; 3] = [50, 80, 100];

pub fn load_config(path: &Path) -> BudgetConfig {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_config(path: &Path, config: &BudgetConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// 已通知过的阈值，按月重置；避免同一档位每次 ingest 都重复弹通知。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NotifyState {
    pub month: String,
    pub notified: Vec<u32>,
}

pub(crate) fn load_notify_state(path: &Path) -> NotifyState {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(crate) fn save_notify_state(path: &Path, state: &NotifyState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub fn local_month_filter(now: DateTime<Local>) -> Filter {
    let date = now.date_naive();
    let start = NaiveDate::from_ymd_opt(date.year(), date.month(), 1)
        .expect("first day of month is valid")
        .and_hms_milli_opt(0, 0, 0, 0)
        .expect("local midnight is valid");
    Filter {
        from: Some(to_utc_z(local_or_now(start, now))),
        to: Some(to_utc_z(now)),
        sources: Vec::new(),
        models: Vec::new(),
        projects: Vec::new(),
        providers: Vec::new(),
    }
}

fn local_or_now(naive: chrono::NaiveDateTime, now: DateTime<Local>) -> DateTime<Local> {
    naive
        .and_local_timezone(Local)
        .earliest()
        .or_else(|| naive.and_local_timezone(Local).latest())
        .unwrap_or(now)
}

fn to_utc_z(dt: DateTime<Local>) -> String {
    dt.with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn days_in_month(year: i32, month: u32) -> i64 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .expect("next month boundary is valid");
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("first day of month is valid");
    (next - first).num_days()
}

pub fn status(
    conn: &Connection,
    prices: &PriceTable,
    config: &BudgetConfig,
    now: DateTime<Local>,
) -> Result<BudgetStatusDto, String> {
    let filter = local_month_filter(now);
    let overview = query::overview(conn, &filter, prices)?;
    Ok(status_from_overview(&overview, config, now))
}

pub(crate) fn status_from_overview(
    overview: &crate::domain::OverviewDto,
    config: &BudgetConfig,
    now: DateTime<Local>,
) -> BudgetStatusDto {
    let date = now.date_naive();
    let days_elapsed = date.day() as i64;
    let total_days = days_in_month(date.year(), date.month());
    let daily_average = overview
        .cost
        .map(|amount| amount / days_elapsed.max(1) as f64);
    let projected = daily_average.map(|avg| avg * total_days as f64);
    let percent_used = match (overview.cost, config.monthly_usd) {
        (Some(cost), Some(budget)) if budget > 0.0 => Some(cost / budget * 100.0),
        _ => None,
    };
    let percent_projected = match (projected, config.monthly_usd) {
        (Some(proj), Some(budget)) if budget > 0.0 => Some(proj / budget * 100.0),
        _ => None,
    };
    BudgetStatusDto {
        monthly_budget: config.monthly_usd,
        month: date.format("%Y-%m").to_string(),
        days_elapsed,
        days_in_month: total_days,
        month_to_date_cost: overview.cost.unwrap_or(0.0),
        unpriced: overview.unpriced,
        projected_month_cost: projected,
        percent_used,
        percent_projected,
        thresholds: THRESHOLDS.to_vec(),
    }
}

/// 未配置预算或额度非正时跳过阈值检查，避免无谓的本月费用查询。
pub fn should_check_budget(config: &BudgetConfig) -> bool {
    matches!(config.monthly_usd, Some(budget) if budget > 0.0)
}

/// 本次用量百分比相比“已通知过的阈值”，新跨过（且尚未通知过）的阈值，按百分比升序返回。
/// 纯函数，不涉及月份重置逻辑（由调用方决定传入的 `already_notified` 是否要按新月清空）。
pub fn thresholds_to_notify(percent_used: f64, already_notified: &[u32]) -> Vec<u32> {
    THRESHOLDS
        .iter()
        .copied()
        .filter(|threshold| {
            percent_used >= *threshold as f64 && !already_notified.contains(threshold)
        })
        .collect()
}

/// 根据已落盘的通知状态和本月用量百分比，算出本次应通知的阈值以及写回磁盘的新状态。
/// 跨月时先清空 `notified`；未跨过任何新阈值时返回原状态（仅月份对齐）。
pub(crate) fn prepare_notifications(
    mut state: NotifyState,
    month: &str,
    percent_used: f64,
) -> (NotifyState, Vec<u32>) {
    if state.month != month {
        state.month = month.to_string();
        state.notified.clear();
    }
    let crossed = thresholds_to_notify(percent_used, &state.notified);
    for threshold in &crossed {
        if !state.notified.contains(threshold) {
            state.notified.push(*threshold);
        }
    }
    (state, crossed)
}

/// 摄取完成后调用：若配置了月度预算且本月用量新跨过阈值，则发一次本地系统通知。
/// 未配置预算时直接跳过，不产生任何额外查询开销。
pub fn check_and_notify<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    conn: &Connection,
    prices: &PriceTable,
    config_path: &Path,
    notify_state_path: &Path,
) -> Result<(), String> {
    let config = load_config(config_path);
    if !should_check_budget(&config) {
        return Ok(());
    }
    let budget = config.monthly_usd.expect("should_check_budget 已确认");
    let dto = status(conn, prices, &config, Local::now())?;
    let Some(percent_used) = dto.percent_used else {
        return Ok(());
    };
    let state = load_notify_state(notify_state_path);
    let (next_state, crossed) = prepare_notifications(state, &dto.month, percent_used);
    if crossed.is_empty() {
        return Ok(());
    }
    let highest = *crossed.iter().max().expect("crossed 非空");
    let body = format!(
        "本月 AI 使用费用已达预算的 {highest}%（${:.2} / ${budget:.2}）",
        dto.month_to_date_cost
    );
    send_notification(app, "预算提醒", &body);
    save_notify_state(notify_state_path, &next_state)
}

fn send_notification<R: tauri::Runtime>(app: &tauri::AppHandle<R>, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}
