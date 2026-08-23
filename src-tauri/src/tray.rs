//! 菜单栏今日花费：本地时区当天合计，关闭主窗口后继续刷新。
//! 左键打开自定义额度悬浮窗；打开主窗口 / 刷新 / 退出留在右键原生菜单。

use std::time::Duration;

use chrono::{DateTime, Local, SecondsFormat, Utc};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewWindowBuilder};

use crate::domain::{Filter, OfficialQuotaDto, OverviewDto};
use crate::{ingest, official_quota, query, tray_popup, AppState};

const TRAY_ID: &str = "today-cost";
const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

pub fn local_day_filter(now: DateTime<Local>) -> Filter {
    let date = now.date_naive();
    let start = date
        .and_hms_milli_opt(0, 0, 0, 0)
        .expect("local midnight is valid");
    let end = date
        .and_hms_milli_opt(23, 59, 59, 999)
        .expect("local end of day is valid");
    Filter {
        from: Some(to_utc_z(local_or_now(start, now))),
        to: Some(to_utc_z(local_or_now(end, now))),
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

pub fn format_title(cost: Option<f64>, unpriced: bool) -> String {
    format_title_with_quota(cost, unpriced, None)
}

pub fn format_title_with_quota(
    cost: Option<f64>,
    unpriced: bool,
    quota: Option<&official_quota::TightestQuota>,
) -> String {
    let cost = match (cost, unpriced) {
        (None, true) => "—".to_string(),
        (None, false) => "$0.00".to_string(),
        (Some(amount), true) => format!("${amount:.2}*"),
        (Some(amount), false) => format!("${amount:.2}"),
    };
    match quota {
        Some(item) => {
            let mark = if item.stale { "*" } else { "" };
            format!(
                "{cost} · {} {} {:.0}%{mark}",
                item.provider, item.label, item.used_percent
            )
        }
        None => cost,
    }
}

pub fn setup(app: &AppHandle) -> Result<(), String> {
    app.manage(tray_popup::PopupGuard::default());

    let show = MenuItem::with_id(app, "show", "打开主窗口", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let refresh_item =
        MenuItem::with_id(app, "refresh", "刷新", true, None::<&str>).map_err(|e| e.to_string())?;
    let quit =
        MenuItem::with_id(app, "quit", "退出", true, None::<&str>).map_err(|e| e.to_string())?;
    let sep = PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?;
    let menu =
        Menu::with_items(app, &[&show, &refresh_item, &sep, &quit]).map_err(|e| e.to_string())?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        // Linux 托盘点按事件本身就不发，关掉左键菜单等于左键没反应。
        .show_menu_on_left_click(cfg!(target_os = "linux"))
        .tooltip("今日花费")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "refresh" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = tauri::async_runtime::spawn_blocking(move || refresh_with_ingest(&app))
                        .await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tray_popup::on_tray_event(tray.app_handle(), &event);
        });

    // 彩色 app 图标不是做成 alpha 模板的单色图，`icon_as_template` 会把它按
    // 系统菜单栏配色抹成纯黑/白轮廓，颜色基本丢光——按用户要求走彩色原图。
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app).map_err(|e| e.to_string())?;

    if let Ok(overview) = query_today(app) {
        let quota = load_quota_dto(app).ok();
        apply_labels_now(app, &overview, quota.as_ref());
    }

    let handle = app.clone();
    std::thread::spawn(move || {
        // 启动只刷菜单栏缓存，全量摄取交给主窗口。两边一起扫盘会把首屏查询拖死。
        let _ = refresh(&handle);
        loop {
            std::thread::sleep(REFRESH_INTERVAL);
            let _ = refresh_if_stale(&handle);
        }
    });

    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    if let Err(error) = create_main_window(app) {
        eprintln!("打开主窗口失败：{error}");
    }
}

fn create_main_window(app: &AppHandle) -> Result<(), String> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()
        .ok_or_else(|| "缺少 main 窗口配置".to_string())?;
    WebviewWindowBuilder::from_config(app, &config)
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn refresh(app: &AppHandle) -> Result<(), String> {
    let overview = query_today(app)?;
    apply_labels(app, &overview)
}

/// 源文件元数据没变时只重算今日菜单栏，避免关闭主窗口后每 5 分钟全量扫盘。
pub fn refresh_if_stale(app: &AppHandle) -> Result<(), String> {
    let cache = {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        ingest::load_scan_cache(&conn)?
    };
    if ingest::scan_is_stale_from_cache(&cache, &ingest::default_home())? {
        refresh_with_ingest(app)
    } else {
        let _ = sync_official_quota(app);
        refresh(app)
    }
}

pub fn refresh_with_ingest(app: &AppHandle) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let conn = state.lock_write()?;
        ingest::ingest_all(&conn, &ingest::default_home())?;
        let prices = state.effective_prices();
        let _ = crate::budget::check_and_notify(
            app,
            &conn,
            &prices,
            &state.budget_path,
            &state.budget_notify_path,
        );
        crate::release_idle_memory(&state, &conn);
    }
    let _ = sync_official_quota(app);
    refresh(app)
}

/// 联网拉一遍各家官方额度（受退避冷却约束，不会比主窗口的刷新更激进），
/// 再落库刷新托盘标题/悬浮面板。取数放在锁外，避免持锁期间打网络。
fn sync_official_quota(app: &AppHandle) -> Result<(), String> {
    let results = official_quota::fetch_all_providers();
    let state = app.state::<AppState>();
    let conn = state.lock_write()?;
    let _ = official_quota::sync_claude_capture(&conn);
    official_quota::apply_fetch_results(&conn, results)?;
    let config = official_quota::load_config(&state.official_quota_path);
    let dto = official_quota::load_dto(&conn, &config, chrono::Utc::now());
    official_quota::notify::check_and_notify_with_config(
        app,
        &dto,
        &config,
        &state.official_quota_notify_path,
    )
}

fn query_today(app: &AppHandle) -> Result<OverviewDto, String> {
    let state = app.state::<AppState>();
    let prices = state.effective_prices();
    let conn = state.lock_read()?;
    query::overview(&conn, &local_day_filter(Local::now()), &prices)
}

fn apply_labels(app: &AppHandle, overview: &OverviewDto) -> Result<(), String> {
    let quota = load_quota_dto(app).ok();
    let app = app.clone();
    let overview = overview.clone();
    app.clone()
        .run_on_main_thread(move || {
            apply_labels_now(&app, &overview, quota.as_ref());
        })
        .map_err(|e| e.to_string())
}

/// 托盘专用：额度行按 `official_quota.json` 里 `hidden_providers` 过滤——
/// 那份配置就是主窗口「配置显示」写的那份，两边共用，改一处两边一起少一行。
/// 设置页/主窗口的官方额度请求都不走这里，所以隐藏账号仍能在那边看到状态。
pub fn load_quota_dto(app: &AppHandle) -> Result<OfficialQuotaDto, String> {
    let state = app.state::<AppState>();
    let conn = state.lock_read()?;
    let config = official_quota::load_config(&state.official_quota_path);
    let mut dto = official_quota::load_dto(&conn, &config, chrono::Utc::now());
    dto.rows = official_quota::visible_rows(dto.rows, &dto.hidden_providers);
    Ok(dto)
}

fn apply_labels_now(app: &AppHandle, overview: &OverviewDto, quota: Option<&OfficialQuotaDto>) {
    let tightest = quota.and_then(official_quota::tightest_window);
    let title = format_title_with_quota(overview.cost, overview.unpriced, tightest.as_ref());
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_title(None::<&str>);
        let _ = tray.set_tooltip(Some(format!("今日花费 {title}")));
    }
    if let Some(quota) = quota {
        tray_popup::notify_if_open(app, quota);
    }
}
