//! Factory / Droid 官方额度：读本机 `~/.factory` 的登录态，打 `GET /api/billing/limits`
//! 取额度窗口，`GET /api/organization/subscription/schedule` 取套餐名（失败不影响
//! 额度窗口，`x-factory-org-id` 头直接从 access_token 的 JWT `external_org_id`
//! 声明拿，不用额外查）。
//!
//! droid CLI 有三种凭证存储，按其自身优先级依次尝试：
//! 1. `login-keychain-v2`（仅 macOS）：密文在 `auth.v2.loginkeychain`，解密密钥不落盘，
//!    droid 自己也是现场调用 `/usr/bin/security find-generic-password` 从 macOS 登录
//!    钥匙串（service=`Factory CLI`，account=`auth-encryption-key-security-cli`）取出来
//!    的——这个钥匙串条目信任的是 `/usr/bin/security` 这个二进制本身，不是调用它的进程，
//!    所以我们照样调用同一个命令通常不会弹钥匙串授权框。
//! 2. `keyfile-v2`：`auth.v2.file`（AES-256-GCM，`base64(iv):base64(tag):base64(密文)`），
//!    密钥是旁边明文的 `auth.v2.key`。droid 切到钥匙串存储之后这对文件就不再刷新，
//!    留着的话解出来的 access_token 大概率已经过期。
//! 3. 旧版 `auth.json`，明文 JSON，兜底。

use std::path::PathBuf;
use std::time::Duration;

use aes_gcm::aead::consts::{U12, U16};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::aes::Aes256;
use aes_gcm::{AesGcm, Key, Nonce};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::{display_plan_label, sanitize_percent, QuotaSnapshot};

const LIMITS_URL: &str = "https://api.factory.ai/api/billing/limits";
/// 网页版账号设置用的接口，本来给 web-app session 用，但真机验证过 CLI 本地凭证
/// 也能调通。`x-factory-org-id` 其实就是 access_token 自己 JWT 里的
/// `external_org_id`，不用额外查。
const SUBSCRIPTION_SCHEDULE_URL: &str =
    "https://api.factory.ai/api/organization/subscription/schedule";
const TIMEOUT: Duration = Duration::from_secs(12);
/// 响应里的两个额度池：standard 是主池，core 是 Droid Core。
const POOLS: [(&str, &str, &str); 2] = [("standard", "", "标准"), ("core", "core_", "Core")];
const BUCKETS: [(&str, &str); 3] = [("fiveHour", "5 小时"), ("weekly", "周"), ("monthly", "月")];

pub fn fetch_rate_limits() -> Result<QuotaSnapshot, String> {
    let token = load_access_token()?;
    let raw = request_limits(&token)?;
    let windows = parse_limits(&raw, Utc::now())?;
    let plan = fetch_subscription_plan(&token);
    Ok(QuotaSnapshot::new(windows, Utc::now().to_rfc3339()).with_plan(plan))
}

/// 套餐名是加分项，取不到（接口拒绝、JWT 没有 org id 等）不影响额度窗口。
fn fetch_subscription_plan(token: &str) -> Option<String> {
    let org_id = parse_jwt_external_org_id(token)?;
    let raw = request_subscription_schedule(token, &org_id).ok()?;
    parse_subscription_plan(&raw, Utc::now())
}

/// 诊断用：吐出 `/api/organization/subscription/schedule` 的原始 JSON。
pub fn debug_fetch_subscription_schedule() -> Result<String, String> {
    let token = load_access_token()?;
    let org_id = parse_jwt_external_org_id(&token)
        .ok_or_else(|| "access_token 的 JWT 里没有 external_org_id".to_string())?;
    request_subscription_schedule(&token, &org_id)
}

/// WorkOS JWT 的 `external_org_id` 声明，就是网页版请求头 `x-factory-org-id` 用的值。
pub fn parse_jwt_external_org_id(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1).filter(|part| !part.is_empty())?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    claims
        .get("external_org_id")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn request_subscription_schedule(token: &str, org_id: &str) -> Result<String, String> {
    let request = crate::net::agent_with_timeout(TIMEOUT)
        .get(SUBSCRIPTION_SCHEDULE_URL)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json")
        .set("x-factory-org-id", org_id);
    match request.call() {
        Ok(response) => response
            .into_string()
            .map_err(|e| format!("读取 Droid 套餐响应失败：{e}")),
        Err(ureq::Error::Status(401 | 403, _)) => {
            Err("Droid 套餐接口鉴权失败，CLI 本地凭证可能没有这个权限".to_string())
        }
        Err(ureq::Error::Status(code, response)) => {
            let _ = response.into_string();
            Err(format!("拉取 Droid 套餐失败：HTTP {code}"))
        }
        Err(_) => Err("无法连接 Droid 套餐接口，请检查网络后重试".to_string()),
    }
}

/// 响应没有顶层 `plan` 字段：真正的套餐在 `schedule[]` 里，每项是一段生效区间
/// （`start_date`/`end_date` + `plan.name`），账号升级/续费会留下多段历史和未来。
/// 取 `start_date` 不晚于当前时间里最靠后那一段，就是正在生效的套餐；
/// `upcomingTierChanges` 是下一次变更（哪怕只是续费同一档），不是当前套餐，不用它。
/// `plan.name` 是「Factory Pro Annual Plan」这种「品牌 + 档次 + 计费周期 + Plan」的
/// 整句，按空格切开找认得出的档次词，不依赖整句匹配（计费周期、品牌词会变）。
pub fn parse_subscription_plan(raw: &str, now: DateTime<Utc>) -> Option<String> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let schedule = value.get("schedule").and_then(Value::as_array)?;
    let current = schedule
        .iter()
        .filter_map(|entry| {
            let start = entry
                .get("start_date")
                .and_then(Value::as_str)
                .and_then(|text| DateTime::parse_from_rfc3339(text).ok())?
                .with_timezone(&Utc);
            (start <= now).then_some((start, entry))
        })
        .max_by_key(|(start, _)| *start)
        .map(|(_, entry)| entry)?;
    let name = current.pointer("/plan/name").and_then(Value::as_str)?;
    tier_keyword_from_plan_name(name).and_then(|word| display_plan_label(&word))
}

fn tier_keyword_from_plan_name(name: &str) -> Option<String> {
    const KNOWN: [&str; 9] = [
        "free",
        "pro",
        "plus",
        "max",
        "ultra",
        "business",
        "enterprise",
        "team",
        "individual",
    ];
    name.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()))
        .find(|word| {
            let lower = word.to_ascii_lowercase();
            KNOWN.contains(&lower.as_str())
        })
        .map(str::to_string)
}

/// standard / core 两个池各三档；`windowEnd` 已过去的档位说明该桶没在计费窗内，跳过
/// （对齐 droid 自己的显示逻辑）。全部跳过才算结构异常。
pub fn parse_limits(raw: &str, now: DateTime<Utc>) -> Result<Vec<OfficialQuotaWindow>, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|e| format!("Droid 限额 JSON 解析失败：{e}"))?;
    let limits = value
        .get("limits")
        .ok_or_else(|| "Droid 限额响应里没有 limits".to_string())?;

    let mut windows = Vec::new();
    let mut saw_pool = false;
    for (pool_key, kind_prefix, pool_label) in POOLS {
        let Some(pool) = limits.get(pool_key) else {
            continue;
        };
        saw_pool = true;
        for (bucket_key, bucket_label) in BUCKETS {
            let Some(bucket) = pool.get(bucket_key) else {
                continue;
            };
            let resets_at = window_end(bucket);
            if !is_active(resets_at.as_ref(), now) {
                continue;
            }
            let Some(percent) = bucket
                .get("usedPercent")
                .and_then(Value::as_f64)
                .and_then(sanitize_percent)
            else {
                continue;
            };
            windows.push(OfficialQuotaWindow {
                kind: format!("{kind_prefix}{}", bucket_kind(bucket_key)),
                label: format!("{pool_label} {bucket_label}"),
                used_percent: Some(percent),
                resets_at: resets_at.map(|value| value.to_rfc3339()),
                ..Default::default()
            });
        }
    }

    if windows.is_empty() {
        if saw_pool {
            return Err("Droid 限额响应里没有仍在计费窗内的额度".to_string());
        }
        return Err("Droid 限额响应里没有可用的额度池".to_string());
    }
    Ok(windows)
}

fn bucket_kind(bucket_key: &str) -> &'static str {
    match bucket_key {
        "fiveHour" => "five_hour",
        "weekly" => "weekly",
        _ => "monthly",
    }
}

fn window_end(bucket: &Value) -> Option<DateTime<Utc>> {
    bucket
        .get("windowEnd")
        .and_then(Value::as_str)
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|value| value.with_timezone(&Utc))
}

/// 没给 `windowEnd` 就不判断，直接当有效；给了就必须还没过。
fn is_active(resets_at: Option<&DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    resets_at.is_none_or(|value| *value > now)
}

fn factory_home() -> PathBuf {
    std::env::var("FACTORY_HOME_OVERRIDE")
        .ok()
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| crate::ingest::default_home().join(".factory"))
}

pub fn load_access_token() -> Result<String, String> {
    let home = factory_home();
    if let Some(token) = read_loginkeychain_v2(&home) {
        return Ok(token);
    }
    if let Some(token) = read_keyfile_v2(&home)? {
        return Ok(token);
    }
    if let Some(token) = read_legacy(&home) {
        return Ok(token);
    }
    Err("尚未登录 Droid，请先运行 droid 并登录 app.factory.ai".to_string())
}

/// macOS 钥匙串条目的 service/account，取值与 droid CLI 二进制里反编译出来的常量一致。
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "Factory CLI";
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNT: &str = "auth-encryption-key-security-cli";

/// 密文文件存在但取不到密钥（钥匙串条目缺失/解密失败）都当作「这条存储不可用」，
/// 静默落回 keyfile-v2，不让这一步的失败盖过后面可能有效的凭证。
#[cfg(target_os = "macos")]
pub(crate) fn read_loginkeychain_v2(home: &std::path::Path) -> Option<String> {
    let payload = std::fs::read_to_string(home.join("auth.v2.loginkeychain")).ok()?;
    // droid 非生产构建会给 account 加 `-dev` 后缀，正常安装用不到，保底也试一下。
    let key = macos_keychain_password(KEYCHAIN_ACCOUNT)
        .or_else(|| macos_keychain_password(&format!("{KEYCHAIN_ACCOUNT}-dev")))?;
    decrypt_credentials(payload.trim(), &key)
        .ok()
        .and_then(|plain| access_token_from(&plain))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn read_loginkeychain_v2(_home: &std::path::Path) -> Option<String> {
    None
}

/// 调用 `/usr/bin/security`（而不是走 keytar 之类的原生绑定）是关键：droid 写入这个
/// 条目时把访问权限授给了这个二进制本身，所以别的进程只要也调用同一个命令，走的是
/// 同一条已授权路径，不会再弹一次钥匙串授权框。
///
/// 加了个手动超时：钥匙串被锁定 / 需要用户交互授权时 `security` 会挂起等输入，
/// 这条调用跑在 `spawn_blocking` 的线程池里，光靠子进程自己卡住不会冻住 UI，
/// 但会占死一条阻塞线程且这次刷新永远转圈——超时后主动 kill 掉，落回 keyfile-v2。
#[cfg(target_os = "macos")]
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(target_os = "macos")]
fn macos_keychain_password(account: &str) -> Option<String> {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = std::process::Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-a",
            account,
            "-s",
            KEYCHAIN_SERVICE,
            "-w",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let mut stdout = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(out) = stdout.as_mut() {
            let _ = out.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });

    match rx.recv_timeout(KEYCHAIN_TIMEOUT) {
        Ok(stdout_bytes) => {
            let status = child.wait().ok()?;
            parse_security_output(status.success(), &stdout_bytes)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn parse_security_output(success: bool, stdout: &[u8]) -> Option<String> {
    if !success {
        return None;
    }
    let text = String::from_utf8_lossy(stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn read_keyfile_v2(home: &std::path::Path) -> Result<Option<String>, String> {
    let (Ok(payload), Ok(key)) = (
        std::fs::read_to_string(home.join("auth.v2.file")),
        std::fs::read_to_string(home.join("auth.v2.key")),
    ) else {
        return Ok(None);
    };
    let plain = decrypt_credentials(payload.trim(), key.trim())?;
    Ok(access_token_from(&plain))
}

fn read_legacy(home: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(home.join("auth.json")).ok()?;
    access_token_from(&raw)
}

fn access_token_from(raw: &str) -> Option<String> {
    serde_json::from_str::<Value>(raw)
        .ok()?
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

/// `base64(iv):base64(tag):base64(密文)`，AES-256-GCM，密钥是 `auth.v2.key` 的 base64。
pub fn decrypt_credentials(payload: &str, key_b64: &str) -> Result<String, String> {
    let engine = base64::engine::general_purpose::STANDARD;
    let parts: Vec<&str> = payload.split(':').collect();
    let [iv, tag, ciphertext] = parts.as_slice() else {
        return Err("Droid 登录凭证格式不是 iv:tag:密文".to_string());
    };
    let decode = |part: &str, what: &str| {
        engine
            .decode(part)
            .map_err(|e| format!("Droid 登录凭证的{what}解码失败：{e}"))
    };
    let key = decode(key_b64, "密钥")?;
    if key.len() != 32 {
        return Err("Droid 登录凭证密钥长度不是 32 字节".to_string());
    }
    let iv = decode(iv, "iv")?;
    let tag = decode(tag, "tag")?;
    let mut sealed = decode(ciphertext, "密文")?;
    // aes-gcm 要求 tag 拼在密文尾部，droid 是分开存的。
    sealed.extend_from_slice(&tag);

    let plain = decrypt_gcm(&key, &iv, &sealed)
        .ok_or_else(|| "Droid 登录凭证解密失败，可能已被 droid 重新加密".to_string())?;
    String::from_utf8(plain).map_err(|e| format!("Droid 登录凭证不是合法 UTF-8：{e}"))
}

/// droid 用的是 16 字节 IV（GCM 允许非 96-bit），而 `Aes256Gcm` 别名固定 12 字节，
/// 所以按 IV 长度选具体的 nonce 尺寸。
fn decrypt_gcm(key: &[u8], iv: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    let payload = || Payload {
        msg: sealed,
        aad: &[],
    };
    match iv.len() {
        12 => AesGcm::<Aes256, U12>::new(Key::<AesGcm<Aes256, U12>>::from_slice(key))
            .decrypt(Nonce::<U12>::from_slice(iv), payload())
            .ok(),
        16 => AesGcm::<Aes256, U16>::new(Key::<AesGcm<Aes256, U16>>::from_slice(key))
            .decrypt(Nonce::<U16>::from_slice(iv), payload())
            .ok(),
        _ => None,
    }
}

fn request_limits(token: &str) -> Result<String, String> {
    let request = crate::net::agent_with_timeout(TIMEOUT)
        .get(LIMITS_URL)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json");
    match request.call() {
        Ok(response) => response
            .into_string()
            .map_err(|e| format!("读取 Droid 限额响应失败：{e}")),
        Err(ureq::Error::Status(401 | 403, _)) => {
            Err("Droid 登录已过期，请重新运行 droid 登录".to_string())
        }
        Err(ureq::Error::Status(code, response)) => {
            let _ = response.into_string();
            Err(format!("拉取 Droid 限额失败：HTTP {code}"))
        }
        Err(_) => Err("无法连接 Droid 限额接口，请检查网络后重试".to_string()),
    }
}
