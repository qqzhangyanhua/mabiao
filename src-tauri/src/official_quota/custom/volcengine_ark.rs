//! 火山方舟 Coding Plan / Agent Plan 额度。
//!
//! 套餐窗口只告诉控制面：`POST https://open.volcengineapi.com/?Action=…&Version=2024-01-01`，
//! 用账号 AccessKey ID + Secret Access Key 按火山引擎 HMAC-SHA256 签名。
//! **不是**推理 API Key。签名算法对齐官方文档与 magpie / volc-sdk-golang。
//!
//! 用户填的 base URL 只用来认套餐与地域：
//! `https://ark.<region>.volces.com/api/coding` → GetCodingPlanUsage；
//! `…/api/plan` → GetAFPUsage。按量 `…/api/v3` 没有窗口。

use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::parse_resets_at;

use super::{origin_of_normalized, QuotaRequest};

const WHO: &str = "火山方舟额度接口";
const HOST: &str = "open.volcengineapi.com";
const SERVICE: &str = "ark";
const VERSION: &str = "2024-01-01";
const CONTENT_TYPE: &str = "application/json";
const BODY: &[u8] = b"{}";
const SIGNED_HEADERS: &str = "content-type;host;x-content-sha256;x-date";
const DEFAULT_REGION: &str = "cn-beijing";

pub const MISSING_ACCESS_KEY: &str = "未配置 AccessKey ID，请在设置页重新填写";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolcPlan {
    pub action: &'static str,
    pub label: &'static str,
    pub region: String,
}

pub fn urls(base: &str) -> Result<Vec<QuotaRequest>, String> {
    let plan = plan_of(base)?;
    Ok(vec![QuotaRequest {
        url: open_api_url(plan.action),
        required: true,
    }])
}

pub fn open_api_url(action: &str) -> String {
    format!("https://{HOST}/?{query}", query = canonical_query(action))
}

pub fn plan_of(base: &str) -> Result<VolcPlan, String> {
    let origin = origin_of_normalized(base);
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .unwrap_or(&origin);
    let host = rest.split('/').next().unwrap_or("").to_ascii_lowercase();
    if !host.starts_with("ark.") || !host.ends_with(".volces.com") {
        return Err(
            "请填火山方舟套餐接入点，例如 https://ark.cn-beijing.volces.com/api/coding".to_string(),
        );
    }
    let region = region_of(&host);
    let path = normalized_path(base);
    if path.starts_with("/api/coding") {
        return Ok(VolcPlan {
            action: "GetCodingPlanUsage",
            label: "Coding Plan",
            region,
        });
    }
    if path.starts_with("/api/plan") {
        return Ok(VolcPlan {
            action: "GetAFPUsage",
            label: "Agent Plan",
            region,
        });
    }
    Err("这个地址是按量接入点，没有套餐窗口。请填 /api/coding（Coding Plan）或 /api/plan（Agent Plan）".to_string())
}

fn region_of(host: &str) -> String {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() == 4 && labels[0] == "ark" && !labels[1].is_empty() {
        labels[1].to_string()
    } else {
        DEFAULT_REGION.to_string()
    }
}

fn normalized_path(base: &str) -> String {
    let rest = base
        .strip_prefix("https://")
        .or_else(|| base.strip_prefix("http://"))
        .unwrap_or(base);
    let path = rest.split_once('/').map(|(_, path)| path).unwrap_or("");
    format!("/{}", path.trim_matches('/'))
}

fn canonical_query(action: &str) -> String {
    format!("Action={action}&Version={VERSION}")
}

/// 火山引擎 HMAC-SHA256：与官方文档、volc-sdk-golang `GetSignRequest` 同一套。
pub fn sign(
    access_key_id: &str,
    secret: &str,
    region: &str,
    action: &str,
    body: &[u8],
    now: DateTime<Utc>,
) -> (String, String, String) {
    let x_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = &x_date[..8];
    let content_sha = hex_sha256(body);
    let canonical = format!(
        "POST\n/\n{query}\ncontent-type:{CONTENT_TYPE}\nhost:{HOST}\nx-content-sha256:{content_sha}\nx-date:{x_date}\n\n{SIGNED_HEADERS}\n{content_sha}",
        query = canonical_query(action),
    );
    let scope = format!("{date}/{region}/{SERVICE}/request");
    let to_sign = format!(
        "HMAC-SHA256\n{x_date}\n{scope}\n{hash}",
        hash = hex_sha256(canonical.as_bytes())
    );
    let mut key = hmac_sha256(secret.as_bytes(), date.as_bytes());
    for part in [region, SERVICE, "request"] {
        key = hmac_sha256(&key, part.as_bytes());
    }
    let signature = hex_encode(hmac_sha256(&key, to_sign.as_bytes()));
    let authorization = format!(
        "HMAC-SHA256 Credential={access_key_id}/{scope}, SignedHeaders={SIGNED_HEADERS}, Signature={signature}"
    );
    (authorization, x_date, content_sha)
}

pub fn fetch(
    base_url: &str,
    access_key_id: Option<&str>,
    secret: Option<&str>,
) -> super::super::ProviderFetch {
    let access_key_id = access_key_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| MISSING_ACCESS_KEY.to_string())?;
    let secret = secret
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| super::MISSING_SECRET.to_string())?;
    let plan = plan_of(base_url)?;
    let body = signed_post(access_key_id, secret, &plan.region, plan.action)?;
    let windows = parse(&[body.as_str()])?;
    Ok(super::super::QuotaSnapshot::new(
        windows,
        Utc::now().to_rfc3339(),
    ))
}

fn signed_post(
    access_key_id: &str,
    secret: &str,
    region: &str,
    action: &str,
) -> Result<String, String> {
    let (authorization, x_date, content_sha) =
        sign(access_key_id, secret, region, action, BODY, Utc::now());
    let url = open_api_url(action);
    let request = crate::net::agent_with_timeout(std::time::Duration::from_secs(15))
        .post(&url)
        .set("Content-Type", CONTENT_TYPE)
        .set("X-Date", &x_date)
        .set("X-Content-Sha256", &content_sha)
        .set("Authorization", &authorization)
        .set("Accept", "application/json");
    match request.send_bytes(BODY) {
        Ok(response) => response
            .into_string()
            .map_err(|_| "读取响应失败，接口返回的内容不是文本".to_string()),
        Err(ureq::Error::Status(401 | 403, _)) => Err(
            "AccessKey 无效或没有方舟查询权限，请在设置页更新 AccessKey ID 与 Secret Access Key"
                .to_string(),
        ),
        Err(ureq::Error::Status(404, _)) => {
            Err("地址不对：接口不存在，请检查 base URL 与预设类型是否匹配".to_string())
        }
        Err(ureq::Error::Status(429, _)) => Err("对方限流了，稍后会自动重试".to_string()),
        Err(ureq::Error::Status(code, _)) => Err(format!(
            "接口返回异常（HTTP {code}），请确认 base URL 与预设类型是否匹配"
        )),
        Err(_) => Err("网络不通，连不上这个地址，请检查网络或代理设置".to_string()),
    }
}

pub fn parse(bodies: &[&str]) -> Result<Vec<OfficialQuotaWindow>, String> {
    let raw = bodies.first().copied().unwrap_or_default();
    if raw.trim().is_empty() {
        return Err(format!("{WHO}返回了空响应，请确认 base URL 与预设类型"));
    }
    let root: Value = serde_json::from_str(raw).map_err(|_| {
        format!("{WHO}返回的不是合法 JSON（多半是网页或登录页），请确认 base URL 与预设类型")
    })?;
    if let Some(error) = metadata_error(&root) {
        return Err(error);
    }
    let result = root.get("Result").cloned().unwrap_or(Value::Null);
    let mut windows = read_coding_plan(&result);
    if windows.is_empty() {
        windows = read_afp(&result);
    }
    if windows.is_empty() {
        return Err(format!(
            "{WHO}里没有套餐窗口，请确认账号开了 Coding Plan 或 Agent Plan"
        ));
    }
    Ok(windows)
}

fn metadata_error(root: &Value) -> Option<String> {
    let err = root.get("ResponseMetadata")?.get("Error")?;
    let code = err.get("Code").and_then(Value::as_str).unwrap_or("");
    let message = err.get("Message").and_then(Value::as_str).unwrap_or("");
    if code.is_empty() && message.is_empty() {
        return None;
    }
    let lower = code.to_ascii_lowercase();
    if [
        "accesskey",
        "signature",
        "accessdenied",
        "unauthorized",
        "forbidden",
        "credential",
    ]
    .iter()
    .any(|mark| lower.contains(mark))
    {
        return Some(
            "AccessKey 无效或没有方舟查询权限，请在设置页更新 AccessKey ID 与 Secret Access Key"
                .to_string(),
        );
    }
    Some(
        format!("{WHO}返回了错误：{code} {message}")
            .trim()
            .to_string(),
    )
}

fn read_coding_plan(result: &Value) -> Vec<OfficialQuotaWindow> {
    let Some(items) = result.get("QuotaUsage").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut windows = Vec::new();
    for item in items {
        let Some((kind, label)) = window_label(item.get("Level").and_then(Value::as_str)) else {
            continue;
        };
        let Some(used) = json_f64(item, "Percent") else {
            continue;
        };
        windows.push(OfficialQuotaWindow {
            kind,
            label,
            used_percent: Some(used.clamp(0.0, 100.0)),
            resets_at: item.get("ResetTimestamp").and_then(parse_resets_at),
            used_amount: None,
            limit_amount: None,
            currency: None,
            ..Default::default()
        });
    }
    windows
}

fn read_afp(result: &Value) -> Vec<OfficialQuotaWindow> {
    let mut windows = Vec::new();
    for (field, level) in [
        ("AFPFiveHour", "session"),
        ("AFPWeekly", "weekly"),
        ("AFPMonthly", "monthly"),
    ] {
        let Some(bucket) = result.get(field).filter(|node| !node.is_null()) else {
            continue;
        };
        let Some((kind, label)) = window_label(Some(level)) else {
            continue;
        };
        let (used_percent, used_amount, limit_amount) =
            if let Some(percent) = json_f64(bucket, "Percent") {
                (Some(percent.clamp(0.0, 100.0)), None, None)
            } else {
                let Some(used) = json_f64(bucket, "Used") else {
                    continue;
                };
                let Some(limit) = json_f64(bucket, "Quota")
                    .or_else(|| json_f64(bucket, "Total"))
                    .filter(|value| *value > 0.0)
                else {
                    continue;
                };
                (
                    Some((used / limit * 100.0).clamp(0.0, 100.0)),
                    Some(used),
                    Some(limit),
                )
            };
        let resets_at = ["ResetTime", "ResetTimestamp"]
            .into_iter()
            .find_map(|name| bucket.get(name).and_then(parse_resets_at));
        windows.push(OfficialQuotaWindow {
            kind,
            label,
            used_percent,
            resets_at,
            used_amount,
            limit_amount,
            currency: None,
            ..Default::default()
        });
    }
    windows
}

fn window_label(level: Option<&str>) -> Option<(String, String)> {
    match level?.trim().to_ascii_lowercase().as_str() {
        "session" => Some(("hours_5".to_string(), "5 小时".to_string())),
        "weekly" => Some(("days_7".to_string(), "7 天".to_string())),
        "monthly" => Some(("days_30".to_string(), "30 天".to_string())),
        _ => None,
    }
}

fn json_f64(value: &Value, name: &str) -> Option<f64> {
    let node = value.get(name)?;
    if let Some(number) = node.as_f64() {
        return Some(number);
    }
    node.as_str()?.trim().parse().ok()
}

fn hex_sha256(data: &[u8]) -> String {
    hex_encode(Sha256::digest(data))
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        key_block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(msg);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
