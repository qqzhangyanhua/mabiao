//! grok.com gRPC-web 账单回落。
//!
//! REST `GET /v1/billing?format=credits` 对部分账号会 500
//!（`Failed to serialize billing response`）。官方网页和竞品走
//! `GetGrokCreditsConfig`。无公开 .proto，按 live capture 扫字段。

use std::io::Read;
use std::time::Duration;

use chrono::{TimeZone, Utc};

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::sanitize_percent;

const BILLING_GRPC_URL: &str = "https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";
const TIMEOUT: Duration = Duration::from_secs(12);
const TOKEN_AUTH: &str = "xai-grok-cli";
const EMPTY_GRPC_WEB_FRAME: [u8; 5] = [0, 0, 0, 0, 0];

pub fn should_fallback_to_grpc(error: &str) -> bool {
    error.contains("Failed to serialize billing response")
        || error.contains("HTTP 500")
        || error.contains("HTTP 502")
        || error.contains("HTTP 503")
        || error.contains("HTTP 504")
}

pub fn fetch_credits(
    token: &str,
    user_id: Option<&str>,
    client_version: &str,
) -> Result<Vec<OfficialQuotaWindow>, String> {
    let bytes = request_credits(token, user_id, client_version)?;
    parse_credits_grpc(&bytes, Utc::now().timestamp())
}

pub fn parse_credits_grpc(bytes: &[u8], now_secs: i64) -> Result<Vec<OfficialQuotaWindow>, String> {
    let payload =
        first_data_payload(bytes).ok_or_else(|| "Grok gRPC 限额响应里没有可用数据".to_string())?;
    let config = length_delimited_field(payload, 1).unwrap_or(payload);
    let percent = fixed32_field(config, 1)
        .and_then(usage_percent)
        .or_else(|| has_weekly_period(config).then_some(0.0));
    let Some(percent) = percent else {
        return Err("Grok gRPC 限额响应里没有可用的已用百分比".to_string());
    };
    let resets_at = length_delimited_field(config, 5)
        .or_else(|| length_delimited_field(config, 4))
        .and_then(timestamp_seconds)
        .filter(|secs| *secs > now_secs - 86_400)
        .and_then(|secs| Utc.timestamp_opt(secs, 0).single())
        .map(|dt| dt.to_rfc3339());
    Ok(vec![OfficialQuotaWindow {
        kind: "weekly".to_string(),
        label: "周额度".to_string(),
        used_percent: Some(percent),
        resets_at,
        ..Default::default()
    }])
}

fn request_credits(
    token: &str,
    user_id: Option<&str>,
    client_version: &str,
) -> Result<Vec<u8>, String> {
    let mut request = crate::net::agent_with_timeout(TIMEOUT)
        .post(BILLING_GRPC_URL)
        .set("Authorization", &format!("Bearer {token}"))
        .set("X-XAI-Token-Auth", TOKEN_AUTH)
        .set("Accept", "application/grpc-web+proto")
        .set("Content-Type", "application/grpc-web+proto")
        .set("x-grpc-web", "1")
        .set("Origin", "https://grok.com")
        .set("Referer", "https://grok.com/?_s=usage")
        .set("User-Agent", TOKEN_AUTH)
        .set("x-grok-client-version", client_version)
        .set("x-grok-client-mode", "interactive");
    if let Some(user_id) = user_id.filter(|id| !id.is_empty()) {
        request = request.set("x-userid", user_id);
    }
    match request.send_bytes(&EMPTY_GRPC_WEB_FRAME) {
        Ok(response) => read_grpc_response(response),
        Err(ureq::Error::Status(401 | 403, _)) => {
            Err("Grok 登录已过期，请重新运行 grok login".to_string())
        }
        Err(ureq::Error::Status(_, response)) => read_grpc_response(response),
        Err(_) => Err("无法连接 Grok 限额接口，请检查网络后重试".to_string()),
    }
}

fn read_grpc_response(response: ureq::Response) -> Result<Vec<u8>, String> {
    let header_status = response.header("grpc-status").map(ToOwned::to_owned);
    let header_message = response.header("grpc-message").map(ToOwned::to_owned);
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("读取 Grok 限额响应失败：{e}"))?;
    let trailers = trailer_fields(&bytes);
    let status = header_status
        .as_deref()
        .or_else(|| {
            trailers
                .iter()
                .find(|(k, _)| k == "grpc-status")
                .map(|(_, v)| v.as_str())
        })
        .unwrap_or("0");
    let message = header_message
        .as_deref()
        .or_else(|| {
            trailers
                .iter()
                .find(|(k, _)| k == "grpc-message")
                .map(|(_, v)| v.as_str())
        })
        .unwrap_or("");
    match status {
        "0" | "" => Ok(bytes),
        "16" => Err("Grok 登录已过期，请重新运行 grok login".to_string()),
        _ => Err(format!(
            "拉取 Grok 限额失败：{}",
            if message.is_empty() {
                format!("gRPC {status}")
            } else {
                message.to_string()
            }
        )),
    }
}

fn usage_percent(value: f32) -> Option<f64> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let percent = if value <= 1.0 {
        f64::from(value) * 100.0
    } else {
        f64::from(value)
    };
    sanitize_percent(percent)
}

fn first_data_payload(bytes: &[u8]) -> Option<&[u8]> {
    if let Some(payload) = framed_data_payload(bytes) {
        return Some(payload);
    }
    looks_like_protobuf(bytes).then_some(bytes)
}

fn framed_data_payload(bytes: &[u8]) -> Option<&[u8]> {
    let mut index = 0;
    while index + 5 <= bytes.len() {
        let flags = bytes[index];
        if flags != 0 && flags != 1 && flags != 0x80 && flags != 0x81 {
            return None;
        }
        let length = u32::from_be_bytes([
            bytes[index + 1],
            bytes[index + 2],
            bytes[index + 3],
            bytes[index + 4],
        ]) as usize;
        let start = index + 5;
        let end = start.checked_add(length)?;
        if end > bytes.len() {
            return None;
        }
        if flags & 0x80 == 0 {
            return Some(&bytes[start..end]);
        }
        index = end;
    }
    None
}

fn trailer_fields(bytes: &[u8]) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut index = 0;
    while index + 5 <= bytes.len() {
        let flags = bytes[index];
        let length = u32::from_be_bytes([
            bytes[index + 1],
            bytes[index + 2],
            bytes[index + 3],
            bytes[index + 4],
        ]) as usize;
        let start = index + 5;
        let end = start.saturating_add(length);
        if end > bytes.len() {
            break;
        }
        if flags & 0x80 != 0 {
            if let Ok(text) = std::str::from_utf8(&bytes[start..end]) {
                for line in text.lines().filter(|line| !line.is_empty()) {
                    if let Some((key, value)) = line.split_once(':') {
                        fields.push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
                    }
                }
            }
        }
        index = end;
    }
    fields
}

fn looks_like_protobuf(bytes: &[u8]) -> bool {
    let Some(first) = bytes.first() else {
        return false;
    };
    let field = first >> 3;
    let wire = first & 0x07;
    field > 0 && matches!(wire, 0 | 1 | 2 | 5)
}

fn length_delimited_field(message: &[u8], field_number: u64) -> Option<&[u8]> {
    let mut index = 0;
    while index < message.len() {
        let key = read_varint(message, &mut index)?;
        let field = key >> 3;
        let wire = key & 7;
        match wire {
            0 => {
                read_varint(message, &mut index)?;
            }
            1 => {
                index = index.checked_add(8)?;
            }
            2 => {
                let length = read_varint(message, &mut index)? as usize;
                let end = index.checked_add(length)?;
                let value = message.get(index..end)?;
                if field == field_number {
                    return Some(value);
                }
                index = end;
            }
            5 => {
                index = index.checked_add(4)?;
            }
            _ => return None,
        }
        if index > message.len() {
            return None;
        }
    }
    None
}

fn fixed32_field(message: &[u8], field_number: u64) -> Option<f32> {
    let mut index = 0;
    while index < message.len() {
        let key = read_varint(message, &mut index)?;
        let field = key >> 3;
        let wire = key & 7;
        match wire {
            0 => {
                read_varint(message, &mut index)?;
            }
            1 => {
                index = index.checked_add(8)?;
            }
            2 => {
                let length = read_varint(message, &mut index)? as usize;
                index = index.checked_add(length)?;
            }
            5 => {
                let end = index.checked_add(4)?;
                let bits = u32::from_le_bytes(message.get(index..end)?.try_into().ok()?);
                if field == field_number {
                    return Some(f32::from_bits(bits));
                }
                index = end;
            }
            _ => return None,
        }
        if index > message.len() {
            return None;
        }
    }
    None
}

fn timestamp_seconds(message: &[u8]) -> Option<i64> {
    let mut index = 0;
    while index < message.len() {
        let key = read_varint(message, &mut index)?;
        let field = key >> 3;
        let wire = key & 7;
        match wire {
            0 => {
                let value = read_varint(message, &mut index)?;
                if field == 1 {
                    return i64::try_from(value).ok();
                }
            }
            1 => {
                index = index.checked_add(8)?;
            }
            2 => {
                let length = read_varint(message, &mut index)? as usize;
                index = index.checked_add(length)?;
            }
            5 => {
                index = index.checked_add(4)?;
            }
            _ => return None,
        }
        if index > message.len() {
            return None;
        }
    }
    None
}

fn has_weekly_period(config: &[u8]) -> bool {
    length_delimited_field(config, 5).is_some() || length_delimited_field(config, 4).is_some()
}

fn read_varint(bytes: &[u8], index: &mut usize) -> Option<u64> {
    let mut value: u64 = 0;
    let mut shift = 0;
    while *index < bytes.len() && shift < 64 {
        let byte = bytes[*index];
        *index += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}
