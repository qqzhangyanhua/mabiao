//! 从显式配置目录或凭证文件提取额度账号身份。
//!
//! 只读、离线。不读家目录，不读系统钥匙串，不写任何第三方文件，也不把明文令牌放进
//! 指纹或展示标签。有稳定标识（账号 id、用户 id、邮箱、组织 id、JWT `sub`）就用它；
//! 只有不透明密钥时，指纹是 SHA-256，标签是尾号掩码。

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::cursor_credentials;
use crate::domain::OfficialQuotaProvider;
use crate::official_quota::{antigravity, codex_usage, copilot, devin, droid, grok, opencode};

/// 防重和本机同身份比对用的指纹，加上给界面看的脱敏标签。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaIdentity {
    pub fingerprint: String,
    pub label: String,
}

/// 从 `root` 指向的配置目录或凭证文件提取身份。
pub fn read_identity(
    provider: OfficialQuotaProvider,
    root: &Path,
) -> Result<QuotaIdentity, String> {
    match provider {
        OfficialQuotaProvider::Claude => claude(root),
        OfficialQuotaProvider::Codex => codex(root),
        OfficialQuotaProvider::Cursor => cursor(root),
        OfficialQuotaProvider::Grok => grok_identity(root),
        OfficialQuotaProvider::Droid => droid_identity(root),
        OfficialQuotaProvider::Antigravity => antigravity_identity(root),
        OfficialQuotaProvider::OpenCode => opencode_identity(root),
        OfficialQuotaProvider::Copilot => copilot_identity(root),
        OfficialQuotaProvider::Devin => devin_identity(root),
    }
}

fn claude(root: &Path) -> Result<QuotaIdentity, String> {
    let path = file_or_join(root, ".credentials.json");
    let raw = read_text(&path, "未找到 Claude Code 登录态，请先运行 claude 并登录")?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|_| "Claude 登录态文件结构变了，读不到 claudeAiOauth".to_string())?;
    let token = value
        .pointer("/claudeAiOauth/accessToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "Claude 登录态里没有 accessToken".to_string())?;
    Ok(opaque_identity(token))
}

fn codex(root: &Path) -> Result<QuotaIdentity, String> {
    let auth = codex_usage::load_auth(&file_or_join(root, "auth.json"))?;
    if let Some(account_id) = nonempty(auth.account_id.as_deref()) {
        return Ok(stable_identity(
            account_id,
            Some(account_id),
            &auth.access_token,
        ));
    }
    Ok(opaque_identity(&auth.access_token))
}

fn cursor(root: &Path) -> Result<QuotaIdentity, String> {
    let credential = cursor_credentials::read_credential_at(vscode_storage_dir(root))?
        .ok_or_else(|| "未找到 Cursor 登录态".to_string())?;
    let user_id = credential
        .session_token
        .split_once("%3A%3A")
        .map(|(user_id, _)| user_id)
        .filter(|user_id| !user_id.is_empty())
        .ok_or_else(|| "本机 Cursor 登录态的 sub 里没有用户 ID".to_string())?;
    Ok(stable_identity(
        user_id,
        credential.email.as_deref(),
        &credential.session_token,
    ))
}

fn grok_identity(root: &Path) -> Result<QuotaIdentity, String> {
    // 身份只认文件里已经写着的 user_id。缺了就摘要 token，不去打 `/v1/user`。
    let session = grok::load_session_at(root, chrono::Utc::now())?;
    if let Some(user_id) = nonempty(session.user_id.as_deref()) {
        return Ok(stable_identity(user_id, Some(user_id), &session.token));
    }
    Ok(opaque_identity(&session.token))
}

fn droid_identity(root: &Path) -> Result<QuotaIdentity, String> {
    let plain = droid::read_auth_plaintext_at(root)?
        .ok_or_else(|| "尚未登录 Droid，请先运行 droid 并登录 app.factory.ai".to_string())?;
    let value: Value =
        serde_json::from_str(&plain).map_err(|_| "Droid 登录态不是合法 JSON".to_string())?;
    let token = value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "Droid 登录态里没有 access_token".to_string())?;
    if let Some(org_id) = value
        .get("active_organization_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|org_id| !org_id.is_empty())
        .map(str::to_string)
        .or_else(|| droid::parse_jwt_external_org_id(token))
    {
        return Ok(stable_identity(&org_id, Some(&org_id), token));
    }
    Ok(opaque_identity(token))
}

fn antigravity_identity(root: &Path) -> Result<QuotaIdentity, String> {
    let tokens = antigravity::read_local_tokens_at(vscode_storage_dir(root))?;
    let secret = nonempty(tokens.access_token.as_deref())
        .or_else(|| nonempty(tokens.refresh_token.as_deref()))
        .ok_or_else(|| {
            "尚未登录 Antigravity，请先打开 Antigravity 客户端并登录 Google 账号".to_string()
        })?;
    Ok(opaque_identity(secret))
}

fn opencode_identity(root: &Path) -> Result<QuotaIdentity, String> {
    let key = opencode::load_api_key(&file_or_join(root, "auth.json"))?
        .ok_or_else(|| "尚未登录 OpenCode Zen，请先运行 opencode 并登录".to_string())?;
    Ok(opaque_identity(&key))
}

fn copilot_identity(root: &Path) -> Result<QuotaIdentity, String> {
    let login = copilot::load_login_at(root).ok_or_else(|| {
        "未找到 GitHub Copilot 登录态，请先在编辑器里登录 Copilot 或运行 gh auth login".to_string()
    })?;
    if let Some(user) = nonempty(login.user.as_deref()) {
        return Ok(stable_identity(user, Some(user), &login.token));
    }
    Ok(opaque_identity(&login.token))
}

fn devin_identity(root: &Path) -> Result<QuotaIdentity, String> {
    let key = devin::read_api_key_at(vscode_storage_dir(root))?
        .ok_or_else(|| "尚未登录 Devin / Windsurf，请先打开客户端并登录".to_string())?;
    Ok(opaque_identity(&key))
}

fn file_or_join(root: &Path, name: &str) -> PathBuf {
    if root.is_file() {
        root.to_path_buf()
    } else {
        root.join(name)
    }
}

fn vscode_storage_dir(root: &Path) -> &Path {
    if root.is_file() {
        root.parent().unwrap_or(root)
    } else {
        root
    }
}

fn read_text(path: &Path, missing: &str) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(missing.to_string()),
        Err(error) => Err(format!("{missing}：{error}")),
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// 稳定标识不是密钥。标识本身如果就是那串密钥，退回摘要，避免明文漏出去。
fn stable_identity(id: &str, label: Option<&str>, secret: &str) -> QuotaIdentity {
    if id == secret {
        return opaque_identity(secret);
    }
    let label = match nonempty(label) {
        Some(label) if label != secret => label.to_string(),
        _ => id.to_string(),
    };
    QuotaIdentity {
        fingerprint: id.to_string(),
        label,
    }
}

/// 不透明密钥：JWT 里有 `sub` 就用声明，否则 SHA-256 加尾号掩码。
fn opaque_identity(secret: &str) -> QuotaIdentity {
    if let Some(sub) = jwt_claim(secret, "sub") {
        let label = jwt_claim(secret, "email");
        return stable_identity(&sub, label.as_deref(), secret);
    }
    QuotaIdentity {
        fingerprint: sha256_hex(secret.as_bytes()),
        label: masked_label(secret),
    }
}

fn jwt_claim(token: &str, claim: &str) -> Option<String> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let signature = parts.next()?;
    if parts.next().is_some() || payload.is_empty() || signature.is_empty() {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    claims
        .get(claim)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn masked_label(secret: &str) -> String {
    super::custom::store::mask_secret(Some(secret)).unwrap_or_else(|| "••••••".to_string())
}
