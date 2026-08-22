//! 托盘左键弹出的无边框额度窗口：只展示，不放操作。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::tray::TrayIconEvent;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Rect, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use crate::domain::OfficialQuotaDto;
use crate::tray;

pub const LABEL: &str = "tray-quota";
pub const EVENT_SHOWN: &str = "tray-quota-shown";

const WIDTH: f64 = 372.0;
const MIN_HEIGHT: f64 = 120.0;
const MAX_HEIGHT: f64 = 640.0;
const GAP: f64 = 8.0;
const BLUR_GRACE: Duration = Duration::from_millis(400);

pub struct PopupGuard {
    ignore_blur_until: Mutex<Instant>,
}

impl Default for PopupGuard {
    fn default() -> Self {
        Self {
            ignore_blur_until: Mutex::new(Instant::now()),
        }
    }
}

impl PopupGuard {
    fn arm(&self) {
        if let Ok(mut until) = self.ignore_blur_until.lock() {
            *until = Instant::now() + BLUR_GRACE;
        }
    }

    fn armed(&self) -> bool {
        self.ignore_blur_until
            .lock()
            .map(|until| Instant::now() < *until)
            .unwrap_or(false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 托盘在上就往下弹，在下就往上弹；水平居中对准图标，超出工作区就夹紧。
pub fn place_popup(tray: RectF, popup_w: f64, popup_h: f64, work: RectF, gap: f64) -> (f64, f64) {
    let min_x = work.x + gap;
    let max_x = (work.x + work.w - popup_w - gap).max(min_x);
    let x = (tray.x + tray.w / 2.0 - popup_w / 2.0).clamp(min_x, max_x);

    let below = tray.y + tray.h + gap;
    let above = tray.y - popup_h - gap;
    let y = if below + popup_h + gap <= work.y + work.h {
        below
    } else if above >= work.y + gap {
        above
    } else {
        (work.y + work.h - popup_h - gap).max(work.y + gap)
    };
    (x, y)
}

pub fn popup_logical_size(row_count: usize, window_count: usize) -> (f64, f64) {
    let body = if row_count == 0 {
        72.0
    } else {
        row_count as f64 * 40.0 + window_count as f64 * 26.0 + 12.0
    };
    let height = (16.0 + 48.0 + body).clamp(MIN_HEIGHT, MAX_HEIGHT);
    (WIDTH, height)
}

pub fn toggle(app: &AppHandle, tray_rect: &Rect) -> Result<(), String> {
    let guard = app
        .try_state::<PopupGuard>()
        .ok_or_else(|| "托盘悬浮窗尚未初始化".to_string())?;
    guard.arm();

    let window = ensure(app)?;
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return Ok(());
    }

    let quota = tray::load_quota_dto(app).ok();
    show_at(app, &window, tray_rect, quota.as_ref())
}

pub fn notify_if_open(app: &AppHandle, quota: &OfficialQuotaDto) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.emit(EVENT_SHOWN, quota);
    }
}

pub fn on_tray_event(app: &AppHandle, event: &TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: tauri::tray::MouseButton::Left,
        button_state: tauri::tray::MouseButtonState::Up,
        rect,
        ..
    } = event
    {
        let _ = toggle(app, rect);
    }
}

fn ensure(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("官方额度")
        .inner_size(WIDTH, MIN_HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .shadow(false)
        .accept_first_mouse(true)
        .build()
        .map_err(|e| e.to_string())?;

    let hide = window.clone();
    let app = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            if app
                .try_state::<PopupGuard>()
                .is_some_and(|guard| guard.armed())
            {
                return;
            }
            let _ = hide.hide();
        }
    });
    Ok(window)
}

fn show_at(
    app: &AppHandle,
    window: &WebviewWindow,
    tray_rect: &Rect,
    quota: Option<&OfficialQuotaDto>,
) -> Result<(), String> {
    let row_count = quota.map(|dto| dto.rows.len()).unwrap_or(0);
    let window_count = quota
        .map(|dto| {
            dto.rows
                .iter()
                .map(|row| row.windows.len().max(1))
                .sum::<usize>()
        })
        .unwrap_or(0);
    let (width, height) = popup_logical_size(row_count, window_count);
    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;

    let scale = window.scale_factor().unwrap_or(1.0);
    let tray = physical_rect(tray_rect, scale);
    let work = monitor_work_area(app, tray.x + tray.w / 2.0, tray.y + tray.h / 2.0, scale);
    let (x, y) = place_popup(tray, width * scale, height * scale, work, GAP * scale);
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    let _ = window.set_focus();
    if let Some(quota) = quota {
        let _ = window.emit(EVENT_SHOWN, quota);
    }
    Ok(())
}

fn physical_rect(rect: &Rect, scale: f64) -> RectF {
    let pos = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    RectF {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
    }
}

fn monitor_work_area(app: &AppHandle, x: f64, y: f64, scale: f64) -> RectF {
    let monitor = app
        .monitor_from_point(x, y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    match monitor {
        Some(monitor) => {
            let area = monitor.work_area();
            RectF {
                x: area.position.x as f64,
                y: area.position.y as f64,
                w: area.size.width as f64,
                h: area.size.height as f64,
            }
        }
        None => RectF {
            x: 0.0,
            y: 0.0,
            w: 1440.0 * scale,
            h: 900.0 * scale,
        },
    }
}
