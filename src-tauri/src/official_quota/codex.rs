use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::{parse_resets_at, sanitize_percent};

const TIMEOUT: Duration = Duration::from_secs(12);

pub fn fetch_rate_limits() -> Result<(Vec<OfficialQuotaWindow>, String), String> {
    let raw = query_app_server()?;
    let windows = parse_rate_limits(&raw)?;
    Ok((windows, Utc::now().to_rfc3339()))
}

pub fn parse_rate_limits(raw: &str) -> Result<Vec<OfficialQuotaWindow>, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|e| format!("Codex 限额 JSON 无效：{e}"))?;
    let result = value.get("result").unwrap_or(&value);
    let mut windows = Vec::new();
    if let Some(by_id) = result.get("rateLimitsByLimitId").and_then(Value::as_object) {
        for (key, node) in by_id {
            windows.extend(windows_from_bucket(node, key));
        }
    } else if let Some(primary) = result.get("rateLimits") {
        windows.extend(windows_from_bucket(primary, "codex"));
    }
    if windows.is_empty() {
        return Err("Codex 限额响应里没有可用窗口".to_string());
    }
    Ok(windows)
}

fn windows_from_bucket(node: &Value, fallback: &str) -> Vec<OfficialQuotaWindow> {
    let mut windows = Vec::new();
    if let Some(window) = parse_slot(node.get("primary"), fallback, "primary") {
        windows.push(window);
    }
    if let Some(window) = parse_slot(node.get("secondary"), fallback, "secondary") {
        windows.push(window);
    }
    windows
}

fn parse_slot(node: Option<&Value>, bucket: &str, slot: &str) -> Option<OfficialQuotaWindow> {
    let node = node?;
    if node.is_null() {
        return None;
    }
    let mins = node.get("windowDurationMins").and_then(Value::as_i64);
    let kind = kind_for(mins, slot);
    let label = label_for(mins, bucket, slot);
    let percent = node
        .get("usedPercent")
        .and_then(Value::as_f64)
        .and_then(sanitize_percent);
    let resets_at = node.get("resetsAt").and_then(parse_resets_at);
    if percent.is_none() && resets_at.is_none() {
        return None;
    }
    Some(OfficialQuotaWindow {
        kind,
        label,
        used_percent: percent,
        resets_at,
        ..Default::default()
    })
}

fn kind_for(mins: Option<i64>, slot: &str) -> String {
    match mins {
        Some(value) if value >= 10_000 => "weekly".to_string(),
        Some(value) if value >= 240 => "session_5h".to_string(),
        _ => slot.to_string(),
    }
}

fn label_for(mins: Option<i64>, bucket: &str, slot: &str) -> String {
    match mins {
        Some(value) if value % 1_440 == 0 && value > 0 => format!("{} 天", value / 1_440),
        Some(value) if value % 60 == 0 && value > 0 => format!("{} 小时", value / 60),
        Some(value) if value > 0 => format!("{value} 分钟"),
        _ => format!("{bucket} {slot}"),
    }
}

fn query_app_server() -> Result<String, String> {
    let mut child = Command::new("codex")
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "未找到 Codex CLI，或无法启动 app-server".to_string())?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "无法写入 Codex app-server".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取 Codex app-server".to_string())?;

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(read_rate_limits(stdin, stdout));
    });

    let result = match rx.recv_timeout(TIMEOUT) {
        Ok(value) => value,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Codex app-server 超时".to_string());
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn read_rate_limits(mut stdin: impl Write, stdout: impl std::io::Read) -> Result<String, String> {
    writeln!(
        stdin,
        "{}",
        json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": {
                    "name": "mabiao",
                    "title": "码表",
                    "version": "0.1.1"
                }
            }
        })
    )
    .map_err(|e| format!("写入 Codex initialize 失败：{e}"))?;
    stdin.flush().map_err(|e| e.to_string())?;

    let mut reader = BufReader::new(stdout);
    wait_for_id(&mut reader, 1)?;

    writeln!(stdin, "{}", json!({"method": "initialized", "params": {}}))
        .map_err(|e| format!("写入 Codex initialized 失败：{e}"))?;
    writeln!(
        stdin,
        "{}",
        json!({"method": "account/rateLimits/read", "id": 2})
    )
    .map_err(|e| format!("写入 Codex rateLimits 失败：{e}"))?;
    stdin.flush().map_err(|e| e.to_string())?;
    wait_for_id(&mut reader, 2)
}

fn wait_for_id(reader: &mut impl BufRead, id: i64) -> Result<String, String> {
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| format!("读取 Codex app-server 失败：{e}"))?;
        if read == 0 {
            return Err("Codex app-server 已关闭".to_string());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if value.get("id").and_then(Value::as_i64) == Some(id) {
            if value.get("error").is_some() {
                let message = value
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Codex 返回错误");
                return Err(message.to_string());
            }
            return Ok(trimmed.to_string());
        }
    }
}
