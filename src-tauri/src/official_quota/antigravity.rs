//! Antigravity 官方额度：读本机 Antigravity 客户端的 Google 登录态，
//! 打 `POST /v1internal:retrieveUserQuotaSummary`。
//!
//! 登录态有两条，macOS 上 **AGY / 2.7+ 走钥匙串**：service=`gemini`、
//! account=`antigravity`，值是 zalando go-keyring 的 `go-keyring-base64:` + JSON。
//! 旧 VSCode 壳仍写 `state.vscdb`（`Antigravity` / `Antigravity IDE`），作兜底。
//! access token（`ya29.`）只活约 1 小时，401 了再用 refresh token 现刷。
//! 旧壳的 refresh token 埋在 `antigravityUnifiedStateSync.oauthToken` 的嵌套
//! protobuf 里（外层 base64 → protobuf → 内层 base64 → protobuf）。
//!
//! 刷新要用 Antigravity 自己的 OAuth 客户端。**我们不内嵌它的 client secret**——
//! 那是 Google 发给 Antigravity 的凭证，不该进本仓库，GitHub 的 secret scanning 也会拦。
//! 运行时从本机安装里扫：老版本在 `out/main.js`，macOS 2.7+ 把客户端打进
//! `language_server` 二进制；`Antigravity IDE.app` 仍是旧布局。Google 轮换密钥时能跟上。
//!
//! cloudcode-pa 按 **User-Agent** 判定 Code Assist 权限：UA 里不带 `Antigravity/`
//! 标记就一律 403「no valid license」。实测版本号不影响，只认这个标记。

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use chrono::Utc;
use serde_json::Value;

use crate::domain::OfficialQuotaWindow;
use crate::official_quota::{display_plan_label, parse_resets_at, sanitize_percent, QuotaSnapshot};
use crate::vscode_state;

/// 新版客户端目录叫 `Antigravity`，旧 macOS 包叫 `Antigravity IDE`，两边都可能有登录态。
pub(crate) const APP_DIRS: [&str; 2] = ["Antigravity", "Antigravity IDE"];
/// AGY CLI / 2.7+ Hub 用 zalando go-keyring 写下的条目。
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "gemini";
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNT: &str = "antigravity";
#[cfg(target_os = "macos")]
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(5);
const AUTH_STATUS_KEY: &str = "antigravityAuthStatus";
const OAUTH_TOKEN_KEY: &str = "antigravityUnifiedStateSync.oauthToken";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// 必须带 `Antigravity/` 标记，否则 cloudcode-pa 直接 403。
const USER_AGENT: &str = "vscode/1.X.X (Antigravity/4.3.0)";
/// prod 已验证可用；daily / sandbox 是 Antigravity 自己也会走的备用环境。
const SUMMARY_URLS: [&str; 3] = [
    "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:retrieveUserQuotaSummary",
];
/// Gemini CLI / Antigravity 都认这组枚举；`ideType: ANTIGRAVITY` 不在协议里，会空响应。
const CODE_ASSIST_BODY: &str = r#"{"metadata":{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}}"#;
const TIMEOUT: Duration = Duration::from_secs(15);
const NOT_SIGNED_IN: &str = "尚未登录 Antigravity，请先打开 Antigravity 客户端并登录 Google 账号";

#[derive(Debug, Default, Clone, PartialEq)]
pub struct LocalTokens {
    /// `ya29.` 开头，约 1 小时过期。
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
}

enum SummaryError {
    Unauthorized,
    Other(String),
}

pub fn fetch_rate_limits() -> Result<QuotaSnapshot, String> {
    let (raw, token) = fetch_summary()?;
    let windows = parse_quota_summary(&raw)?;
    let plan = parse_code_assist_tier(&raw).or_else(|| fetch_plan(&token));
    Ok(QuotaSnapshot::new(windows, Utc::now().to_rfc3339()).with_plan(plan))
}

fn fetch_summary() -> Result<(String, String), String> {
    let tokens = load_local_tokens()?;
    if let Some(token) = tokens.access_token.as_deref() {
        match request_summary(token) {
            Ok(raw) => return Ok((raw, token.to_string())),
            Err(SummaryError::Other(error)) => return Err(error),
            Err(SummaryError::Unauthorized) => {}
        }
    }
    let refresh_token = tokens
        .refresh_token
        .ok_or_else(|| NOT_SIGNED_IN.to_string())?;
    let access_token = refresh_access_token(&refresh_token)?;
    let raw = request_summary(&access_token).map_err(|error| match error {
        SummaryError::Unauthorized => "Antigravity 登录已失效，请重新打开客户端登录".to_string(),
        SummaryError::Other(message) => message,
    })?;
    Ok((raw, access_token))
}

/// `POST loadCodeAssist` 的 `currentTier.id`。失败不影响额度窗口。
fn fetch_plan(access_token: &str) -> Option<String> {
    let raw = request_code_assist(access_token)?;
    parse_code_assist_tier(&raw)
}

/// 诊断用：吐出 `loadCodeAssist` 的原始 JSON，核对套餐档位字段到底叫什么。
/// 不进生产取数路径，只给 `antigravity_debug_dump_plan_fields` 那个忽略测试用。
pub fn debug_fetch_code_assist_raw() -> Result<String, String> {
    let (_, token) = fetch_summary()?;
    request_code_assist(&token)
        .ok_or_else(|| "loadCodeAssist 没有返回内容，或者鉴权失败".to_string())
}

pub fn parse_code_assist_tier(raw: &str) -> Option<String> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let root = value.get("response").unwrap_or(&value);
    // 真机验证过（Google AI Pro 账号）：`currentTier` 停留在 `free-tier`——那是 Code
    // Assist 自己的 GCP 项目档位，跟消费者订阅是两套体系，个人账号基本永远是它。
    // `paidTier` 才是账号实际生效的 Google AI 订阅档（Pro/Ultra），其
    // `availableCredits` 挂着真实的 Google One 额度，`upgradeSubscriptionText`
    // 说的是「往上升到 Ultra」而不是「开通这一档」。没有 `paidTier` 才落回
    // `currentTier`——那时才真的是免费账号。
    tier_label(root.get("paidTier"))
        .or_else(|| tier_label(root.get("paid_tier")))
        .or_else(|| tier_label(root.get("currentTier")))
        .or_else(|| tier_label(root.get("current_tier")))
        .or_else(|| string_label(root.get("planType")))
        .or_else(|| string_label(root.get("plan_tier")))
        .or_else(|| default_allowed_tier(root))
}

fn default_allowed_tier(root: &Value) -> Option<String> {
    let tiers = root
        .get("allowedTiers")
        .or_else(|| root.get("allowed_tiers"))?
        .as_array()?;
    let chosen = tiers
        .iter()
        .find(|tier| tier.get("isDefault").and_then(Value::as_bool) == Some(true))
        .or_else(|| tiers.first())?;
    tier_label(Some(chosen))
}

fn tier_label(node: Option<&Value>) -> Option<String> {
    let node = node?;
    // `name` 对 Antigravity 是产品品牌（两个档都叫 "Antigravity"），档次在 `id`。
    string_label(node.get("id"))
        .or_else(|| string_label(node.get("name")).filter(|label| !is_product_brand(label)))
        .or_else(|| string_label(Some(node)).filter(|label| !is_product_brand(label)))
}

fn is_product_brand(label: &str) -> bool {
    matches!(
        label.trim().to_ascii_lowercase().as_str(),
        "antigravity" | "gemini" | "gemini code assist" | "code assist"
    )
}

fn string_label(node: Option<&Value>) -> Option<String> {
    node.and_then(Value::as_str).and_then(display_plan_label)
}

/// `groups[].buckets[]` → 每个桶一个窗口。`remainingFraction` 是「剩余」，取反才是已用。
pub fn parse_quota_summary(raw: &str) -> Result<Vec<OfficialQuotaWindow>, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|e| format!("Antigravity 限额 JSON 解析失败：{e}"))?;
    let groups = value
        .get("groups")
        .and_then(Value::as_array)
        .ok_or_else(|| "Antigravity 限额响应里没有 groups".to_string())?;

    let mut windows = Vec::new();
    for group in groups {
        let group_label = group
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        for bucket in group
            .get("buckets")
            .and_then(Value::as_array)
            .unwrap_or(&Vec::new())
        {
            let Some(percent) = bucket
                .get("remainingFraction")
                .and_then(Value::as_f64)
                .map(|remaining| (1.0 - remaining) * 100.0)
                .and_then(sanitize_percent)
            else {
                continue;
            };
            let Some(kind) = bucket
                .get("bucketId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            windows.push(OfficialQuotaWindow {
                kind: kind.replace('-', "_"),
                label: bucket_label(group_label, bucket),
                used_percent: Some(percent),
                resets_at: bucket.get("resetTime").and_then(parse_resets_at),
                ..Default::default()
            });
        }
    }

    if windows.is_empty() {
        return Err("Antigravity 限额响应里没有可用的额度桶".to_string());
    }
    Ok(windows)
}

/// 官方给的是「Weekly Limit Remaining」这种剩余口径的名字，我们展示的是已用，
/// 直接沿用会读反，所以按窗口自己起名，group 名做前缀区分模型池。
fn bucket_label(group_label: &str, bucket: &Value) -> String {
    let window = match bucket.get("window").and_then(Value::as_str) {
        Some("weekly") => "周",
        Some("5h") => "5 小时",
        Some(other) if !other.is_empty() => return format!("{group_label} {other}").trim().into(),
        _ => "额度",
    };
    format!("{group_label} {window}").trim().to_string()
}

fn load_local_tokens() -> Result<LocalTokens, String> {
    if let Some(tokens) = read_keychain_tokens() {
        if tokens != LocalTokens::default() {
            return Ok(tokens);
        }
    }
    let mut last_error = None;
    for app in APP_DIRS {
        let Some(dir) = vscode_state::global_storage_dir(app) else {
            continue;
        };
        match read_local_tokens_at(&dir) {
            Ok(tokens) if tokens != LocalTokens::default() => return Ok(tokens),
            Ok(_) => {}
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| NOT_SIGNED_IN.to_string()))
}

/// 钥匙串条目存在但解析失败时不当成没登录——让刷新去报准确的错。
fn read_keychain_tokens() -> Option<LocalTokens> {
    parse_keyring_blob(&macos_keychain_password()?)
}

/// zalando go-keyring：`go-keyring-base64:` + `{"token":{"access_token","refresh_token"}}`。
pub fn parse_keyring_blob(raw: &str) -> Option<LocalTokens> {
    let raw = raw.trim();
    let value: Value = if let Some(payload) = raw.strip_prefix("go-keyring-base64:") {
        let bytes = decode_base64(payload)?;
        serde_json::from_slice(&bytes).ok()?
    } else if raw.starts_with('{') {
        serde_json::from_str(raw).ok()?
    } else {
        let bytes = decode_base64(raw)?;
        serde_json::from_slice(&bytes).ok()?
    };
    let token = value.get("token").unwrap_or(&value);
    let access_token = token
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    let refresh_token = token
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    if access_token.is_none() && refresh_token.is_none() {
        return None;
    }
    Some(LocalTokens {
        access_token,
        refresh_token,
    })
}

#[cfg(target_os = "macos")]
fn macos_keychain_password() -> Option<String> {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = std::process::Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-a",
            KEYCHAIN_ACCOUNT,
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
            if !status.success() {
                return None;
            }
            String::from_utf8(stdout_bytes)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn macos_keychain_password() -> Option<String> {
    None
}

/// 探针：本机有没有 Antigravity 的登录态（access token 或 refresh token 任一即可）。
pub fn has_local_tokens() -> bool {
    load_local_tokens().is_ok()
}

pub fn read_local_tokens_at(global_storage: &Path) -> Result<LocalTokens, String> {
    let Some(conn) = vscode_state::open_read_only(global_storage)? else {
        return Ok(LocalTokens::default());
    };
    let access_token = vscode_state::read_item(&conn, AUTH_STATUS_KEY)
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| {
            value
                .get("apiKey")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_string)
        });
    let refresh_token = vscode_state::read_item(&conn, OAUTH_TOKEN_KEY)
        .as_deref()
        .and_then(extract_refresh_token);
    Ok(LocalTokens {
        access_token,
        refresh_token,
    })
}

/// 外层 base64 → protobuf，里面某个字符串字段又是一层 base64 → protobuf，
/// refresh token 是内层的一个 `1//` 开头的字符串。字段号不稳定，所以按形状找。
pub fn extract_refresh_token(encoded: &str) -> Option<String> {
    let blob = decode_base64(encoded.trim())?;
    proto_strings(&blob, 0)
        .into_iter()
        .filter(|value| value.len() > 40)
        .filter_map(|value| decode_base64(&value))
        .flat_map(|inner| proto_strings(&inner, 0))
        .find(|value| value.starts_with("1//"))
}

/// 内层那段 base64 是从 protobuf 里切出来的，padding 未必齐，按无 padding 解。
fn decode_base64(value: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(value.trim_end_matches('='))
        .ok()
}

/// 极简 protobuf 遍历：只收 wire type 2 里能当 UTF-8 打印的字段，其余递归下钻。
fn proto_strings(buf: &[u8], depth: u8) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < buf.len() {
        let Some((tag, next)) = read_varint(buf, i) else {
            break;
        };
        i = next;
        match tag & 7 {
            0 => match read_varint(buf, i) {
                Some((_, next)) => i = next,
                None => break,
            },
            1 => i += 8,
            5 => i += 4,
            2 => {
                let Some((len, next)) = read_varint(buf, i) else {
                    break;
                };
                let len = len as usize;
                if next + len > buf.len() {
                    break;
                }
                let value = &buf[next..next + len];
                i = next + len;
                match std::str::from_utf8(value) {
                    Ok(text) if text.chars().all(|c| c.is_ascii_graphic() || c == ' ') => {
                        out.push(text.to_string());
                    }
                    _ if depth < 5 => out.extend(proto_strings(value, depth + 1)),
                    _ => {}
                }
            }
            _ => break,
        }
    }
    out
}

fn read_varint(buf: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let mut result = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(i)?;
        i += 1;
        result |= u64::from(byte & 0x7f).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some((result, i));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// 从本机安装的 Antigravity 里找 OAuth 客户端。`main.js` / `language_server` 里
/// 同时有多个 id 和多个 secret，配对关系看不出来，所以全组合都留着，由令牌
/// 接口来筛（错配返回 `invalid_client`，很快失败）。
pub fn parse_oauth_clients(source: &str) -> Vec<(String, String)> {
    parse_oauth_clients_bytes(source.as_bytes())
}

/// 文本和二进制都能扫：macOS 2.7+ 的客户端在 `language_server` 里，不是 UTF-8。
pub fn parse_oauth_clients_bytes(source: &[u8]) -> Vec<(String, String)> {
    let ids = scan_bytes(source, |window| {
        window.ends_with(".apps.googleusercontent.com")
    });
    let secrets = scan_bytes(source, |window| window.starts_with("GOCSPX-"));
    let mut pairs = Vec::new();
    for id in &ids {
        for secret in &secrets {
            pairs.push((id.clone(), secret.clone()));
        }
    }
    pairs
}

/// 不引正则依赖：按「凭证允许出现的字符」切分，再筛出形状对的片段。
fn scan_bytes(source: &[u8], keep: impl Fn(&str) -> bool) -> Vec<String> {
    let mut found = Vec::new();
    let mut start = None;
    for (index, byte) in source.iter().copied().chain(std::iter::once(0)).enumerate() {
        let allowed = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.');
        if allowed {
            if start.is_none() {
                start = Some(index);
            }
            continue;
        }
        if let Some(from) = start.take() {
            if index - from >= 20 {
                if let Ok(token) = std::str::from_utf8(&source[from..index]) {
                    if keep(token) {
                        found.push(token.to_string());
                    }
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

fn local_oauth_clients() -> Result<Vec<(String, String)>, String> {
    for path in oauth_source_files() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let clients = parse_oauth_clients_bytes(&bytes);
        if !clients.is_empty() {
            return Ok(clients);
        }
    }
    Err("找不到本机 Antigravity 安装目录，无法取得刷新登录所需的客户端信息".to_string())
}

/// 按安装根目录优先：先当前 `Antigravity.app`（含 `language_server`），再旧的 IDE。
/// 同一根目录里先扫小 JS，没有再读大二进制。
pub(crate) fn oauth_source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in install_roots() {
        let js = [
            root.join("out").join("main.js"),
            root.join("resources")
                .join("app")
                .join("out")
                .join("main.js"),
            root.join("dist").join("main.js"),
        ];
        let binaries = [
            root.join("bin").join("language_server"),
            root.join("bin").join("language_server.exe"),
            root.join("resources").join("bin").join("language_server"),
            root.join("resources")
                .join("bin")
                .join("language_server.exe"),
        ];
        files.extend(js.into_iter().filter(|path| path.is_file()));
        files.extend(binaries.into_iter().filter(|path| path.is_file()));
    }
    for name in ["agy", "language_server", "language_server.exe"] {
        if let Some(bin) = which_named(name) {
            if bin.is_file() && !files.contains(&bin) {
                files.push(bin);
            }
        }
    }
    files
}

/// 默认安装位置优先于 PATH：macOS 上先认当前 `Antigravity.app`，避免扫到旧 IDE。
fn install_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/Applications/Antigravity.app/Contents/Resources"),
        PathBuf::from("/Applications/Antigravity.app/Contents/Resources/app"),
        PathBuf::from("/Applications/Antigravity IDE.app/Contents/Resources"),
        PathBuf::from("/Applications/Antigravity IDE.app/Contents/Resources/app"),
    ];
    if let Some(local) = dirs::data_local_dir() {
        roots.push(local.join("Programs").join("Antigravity"));
        roots.push(local.join("Programs").join("Antigravity IDE"));
    }
    roots.push(PathBuf::from("/usr/share/antigravity"));
    roots.push(PathBuf::from("/opt/Antigravity"));
    for name in [
        "agy",
        "antigravity",
        "antigravity-ide",
        "antigravity.cmd",
        "antigravity.exe",
    ] {
        if let Some(bin) = which_named(name) {
            if let Some(parent) = bin.parent() {
                roots.push(parent.to_path_buf());
                if let Some(grand) = parent.parent() {
                    roots.push(grand.to_path_buf());
                }
            }
        }
    }
    roots
}

fn which_named(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// 本机存的 access token 只活约 1 小时，过期了就用 refresh token 换一个。
fn refresh_access_token(refresh_token: &str) -> Result<String, String> {
    let clients = local_oauth_clients()?;
    let mut last = "Antigravity 登录已失效，请重新打开客户端登录".to_string();
    for (client_id, client_secret) in clients {
        let response = crate::net::agent_with_timeout(TIMEOUT)
            .post(TOKEN_URL)
            .send_form(&[
                ("client_id", client_id.as_str()),
                ("client_secret", client_secret.as_str()),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ]);
        match response {
            Ok(ok) => {
                let body = ok
                    .into_string()
                    .map_err(|e| format!("读取 Antigravity 令牌响应失败：{e}"))?;
                if let Some(token) = serde_json::from_str::<Value>(&body).ok().and_then(|value| {
                    value
                        .get("access_token")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                }) {
                    return Ok(token);
                }
                last = "Antigravity 令牌响应里没有 access_token".to_string();
            }
            // 配错客户端会是 400/401，换下一组再试。
            Err(ureq::Error::Status(400 | 401, response)) => {
                let _ = response.into_string();
            }
            Err(ureq::Error::Status(code, response)) => {
                let _ = response.into_string();
                last = format!("刷新 Antigravity 登录失败：HTTP {code}");
            }
            Err(_) => {
                return Err("无法连接 Google 令牌接口，请检查网络后重试".to_string());
            }
        }
    }
    Err(last)
}

fn request_summary(access_token: &str) -> Result<String, SummaryError> {
    let mut last = SummaryError::Other("无法连接 Antigravity 限额接口，请检查网络后重试".into());
    for url in SUMMARY_URLS {
        let result = crate::net::agent_with_timeout(TIMEOUT)
            .post(url)
            .set("Authorization", &format!("Bearer {access_token}"))
            .set("User-Agent", USER_AGENT)
            .set("Content-Type", "application/json")
            .send_string("{}");
        match result {
            Ok(response) => {
                return response.into_string().map_err(|e| {
                    SummaryError::Other(format!("读取 Antigravity 限额响应失败：{e}"))
                })
            }
            // 换环境也不会变，交给上层去刷新重试。
            Err(ureq::Error::Status(401 | 403, _)) => return Err(SummaryError::Unauthorized),
            Err(ureq::Error::Status(code, response)) => {
                let _ = response.into_string();
                last = SummaryError::Other(format!("拉取 Antigravity 限额失败：HTTP {code}"));
            }
            Err(_) => {}
        }
    }
    Err(last)
}

fn request_code_assist(access_token: &str) -> Option<String> {
    for url in SUMMARY_URLS {
        let url = url.replace("retrieveUserQuotaSummary", "loadCodeAssist");
        let result = crate::net::agent_with_timeout(TIMEOUT)
            .post(&url)
            .set("Authorization", &format!("Bearer {access_token}"))
            .set("User-Agent", USER_AGENT)
            .set("Content-Type", "application/json")
            .set(
                "Client-Metadata",
                r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#,
            )
            .send_string(CODE_ASSIST_BODY);
        match result {
            Ok(response) => return response.into_string().ok(),
            Err(ureq::Error::Status(401 | 403, _)) => return None,
            _ => {}
        }
    }
    None
}
