use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::{parse_resets_at, sanitize_percent};

pub fn run_statusline() -> Result<(), String> {
    let mut stdin = String::new();
    io::stdin()
        .read_to_string(&mut stdin)
        .map_err(|e| format!("读取 Claude statusline 失败：{e}"))?;
    let (windows, captured_at) = parse_statusline(&stdin)?;
    write_capture(&super::capture_path(), &stdin, &captured_at)?;
    let line = format_status_line(&windows);
    let mut stdout = io::stdout();
    writeln!(stdout, "{line}").map_err(|e| e.to_string())
}

pub fn refresh_from_capture(path: &Path) -> Result<(Vec<OfficialQuotaWindow>, String), String> {
    let raw = fs::read_to_string(path).map_err(|_| {
        "尚未捕获 Claude statusline，请在设置页写入 hook，并打开一次 Claude Code".to_string()
    })?;
    let (windows, _) = parse_statusline(&raw)?;
    let captured_at = file_captured_at(path)?;
    if windows.is_empty() {
        return Err(
            "本次 statusline 不含可用的 rate_limits（需 Claude Code 2.1.80+ 且已登录订阅）"
                .to_string(),
        );
    }
    Ok((windows, captured_at))
}

pub fn file_captured_at(path: &Path) -> Result<String, String> {
    let meta = fs::metadata(path).map_err(|e| format!("读取 Claude 捕获文件失败：{e}"))?;
    let modified = meta
        .modified()
        .map_err(|e| format!("读取 Claude 捕获时间失败：{e}"))?;
    let secs = modified
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    Ok(DateTime::from_timestamp(secs, 0)
        .unwrap_or_else(Utc::now)
        .to_rfc3339())
}

pub fn parse_statusline(raw: &str) -> Result<(Vec<OfficialQuotaWindow>, String), String> {
    let value: Value = serde_json::from_str(raw.trim())
        .map_err(|e| format!("Claude statusline JSON 无效：{e}"))?;
    let Some(limits) = value.get("rate_limits") else {
        return Err(
            "本次 statusline 不含 rate_limits（需 Claude Code 2.1.80+ 且已登录订阅）".to_string(),
        );
    };
    let mut windows = Vec::new();
    if let Some(window) = parse_window(limits.get("five_hour"), "session_5h", "5 小时") {
        windows.push(window);
    }
    if let Some(window) = parse_window(limits.get("seven_day"), "weekly", "7 天") {
        windows.push(window);
    }
    if windows.is_empty() {
        return Err("Claude rate_limits 没有可用的已用百分比".to_string());
    }
    Ok((windows, Utc::now().to_rfc3339()))
}

fn parse_window(node: Option<&Value>, kind: &str, label: &str) -> Option<OfficialQuotaWindow> {
    let node = node?;
    let percent_raw = node.get("used_percentage").and_then(Value::as_f64);
    let percent = percent_raw.and_then(sanitize_percent);
    if percent_raw.is_some() && percent.is_none() {
        return None;
    }
    let resets_at = node.get("resets_at").and_then(parse_resets_at);
    if percent.is_none() && resets_at.is_none() {
        return None;
    }
    Some(OfficialQuotaWindow {
        kind: kind.to_string(),
        label: label.to_string(),
        used_percent: percent,
        resets_at,
        ..Default::default()
    })
}

fn write_capture(path: &Path, raw: &str, _captured_at: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, raw).map_err(|e| format!("写入 Claude 捕获文件失败：{e}"))
}

fn format_status_line(windows: &[OfficialQuotaWindow]) -> String {
    let five = window_percent(windows, "session_5h");
    let week = window_percent(windows, "weekly");
    format!("5h {five} · 7d {week}")
}

fn window_percent(windows: &[OfficialQuotaWindow], kind: &str) -> String {
    windows
        .iter()
        .find(|window| window.kind == kind)
        .and_then(|window| window.used_percent)
        .map(|value| format!("{value:.0}%"))
        .unwrap_or_else(|| "—".to_string())
}
