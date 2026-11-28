//! 从登录态抄一份密钥。显式目录只读文件，不写第三方客户端文件，也不走钥匙串。
//! 没指定目录时走和零配置取数相同的发现顺序；Antigravity 与 Droid 会先读钥匙串。

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::AccountSecret;
use crate::domain::OfficialQuotaProvider;
use crate::official_quota::{
    antigravity, claude_usage, codex_usage, copilot, devin, droid, grok, opencode, QuotaEnv,
};

pub fn capture_secret_at(
    provider: OfficialQuotaProvider,
    root: &Path,
    now: DateTime<Utc>,
) -> Result<AccountSecret, String> {
    match provider {
        OfficialQuotaProvider::Claude => claude(root, now),
        OfficialQuotaProvider::Codex => codex(root),
        OfficialQuotaProvider::Cursor => cursor(root),
        OfficialQuotaProvider::Grok => grok_secret(root, now),
        OfficialQuotaProvider::Droid => droid_secret(root),
        OfficialQuotaProvider::Antigravity => antigravity_secret(root),
        OfficialQuotaProvider::OpenCode => opencode_secret(root),
        OfficialQuotaProvider::Copilot => copilot_secret(root),
        OfficialQuotaProvider::Devin => devin_secret(root),
    }
}

fn claude(root: &Path, now: DateTime<Utc>) -> Result<AccountSecret, String> {
    let path = file_or_join(root, ".credentials.json");
    let access_token = claude_usage::load_access_token_at(&path, now)?;
    let refresh_token = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| {
            value
                .pointer("/claudeAiOauth/refreshToken")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_string)
        });
    Ok(AccountSecret {
        access_token,
        refresh_token,
        ..AccountSecret::default()
    })
}

fn codex(root: &Path) -> Result<AccountSecret, String> {
    let path = file_or_join(root, "auth.json");
    let auth = codex_usage::load_auth(&path)?;
    let refresh_token = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| {
            value
                .pointer("/tokens/refresh_token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_string)
        });
    Ok(AccountSecret {
        access_token: auth.access_token,
        refresh_token,
        account_id: auth.account_id,
        ..AccountSecret::default()
    })
}

fn cursor(root: &Path) -> Result<AccountSecret, String> {
    let credential = crate::cursor_credentials::read_credential_at(vscode_storage_dir(root))?
        .ok_or_else(|| "未找到 Cursor 登录态".to_string())?;
    Ok(AccountSecret {
        access_token: credential.session_token,
        ..AccountSecret::default()
    })
}

fn grok_secret(root: &Path, now: DateTime<Utc>) -> Result<AccountSecret, String> {
    let session = grok::load_session_at(root, now)?;
    let refresh = session.refresh;
    Ok(AccountSecret {
        access_token: session.token,
        refresh_token: refresh.as_ref().map(|item| item.refresh_token.clone()),
        refresh_issuer: refresh.as_ref().map(|item| item.oidc_issuer.clone()),
        refresh_client_id: refresh.as_ref().map(|item| item.client_id.clone()),
        account_id: session.user_id,
        access_expires_at: session.expires_at,
        ..AccountSecret::default()
    })
}

fn droid_secret(root: &Path) -> Result<AccountSecret, String> {
    let plain = droid::read_auth_plaintext_at(root)?
        .ok_or_else(|| "尚未登录 Droid，请先运行 droid 并登录 app.factory.ai".to_string())?;
    droid_secret_from_plaintext(&plain)
}

pub(crate) fn droid_secret_from_plaintext(plain: &str) -> Result<AccountSecret, String> {
    let value: Value =
        serde_json::from_str(plain).map_err(|_| "Droid 登录态不是合法 JSON".to_string())?;
    let access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "Droid 登录态里没有 access_token".to_string())?
        .to_string();
    let refresh_token = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    Ok(AccountSecret {
        access_token,
        refresh_token,
        ..AccountSecret::default()
    })
}

fn antigravity_secret(root: &Path) -> Result<AccountSecret, String> {
    antigravity_secret_from_tokens(&antigravity::read_local_tokens_at(vscode_storage_dir(
        root,
    ))?)
}

pub(crate) fn antigravity_secret_from_tokens(
    tokens: &antigravity::LocalTokens,
) -> Result<AccountSecret, String> {
    let access_token = tokens.access_token.clone().unwrap_or_default();
    if access_token.trim().is_empty() && tokens.refresh_token.is_none() {
        return Err(
            "尚未登录 Antigravity，请先打开 Antigravity 客户端并登录 Google 账号".to_string(),
        );
    }
    Ok(AccountSecret {
        access_token,
        refresh_token: tokens.refresh_token.clone(),
        ..AccountSecret::default()
    })
}

/// 没传目录时和零配置取数读同一份登录。显式目录仍走 `capture_secret_at`，不进这里。
pub(crate) fn capture_default_in(
    env: &QuotaEnv,
    provider: OfficialQuotaProvider,
) -> Result<AccountSecret, String> {
    match provider {
        OfficialQuotaProvider::Antigravity => {
            antigravity_secret_from_tokens(&antigravity::load_local_tokens_in(env)?)
        }
        OfficialQuotaProvider::Droid => {
            let plain = droid::load_default_auth_plaintext_in(env)?.ok_or_else(|| {
                "尚未登录 Droid，请先运行 droid 并登录 app.factory.ai".to_string()
            })?;
            droid_secret_from_plaintext(&plain)
        }
        other => {
            let mut last = "未找到本机登录态，请先在客户端登录".to_string();
            for root in env.default_roots(other) {
                match capture_secret_at(other, &root, env.now()) {
                    Ok(secret) => return Ok(secret),
                    Err(error) => last = error,
                }
            }
            Err(last)
        }
    }
}

fn opencode_secret(root: &Path) -> Result<AccountSecret, String> {
    let key = opencode::load_api_key(&file_or_join(root, "auth.json"))?
        .ok_or_else(|| "尚未登录 OpenCode Zen，请先运行 opencode 并登录".to_string())?;
    Ok(AccountSecret {
        access_token: key,
        ..AccountSecret::default()
    })
}

fn copilot_secret(root: &Path) -> Result<AccountSecret, String> {
    let login = copilot::load_login_at(root).ok_or_else(|| {
        "未找到 GitHub Copilot 登录态，请先在编辑器里登录 Copilot 或运行 gh auth login".to_string()
    })?;
    Ok(AccountSecret {
        access_token: login.token,
        account_id: login.user,
        ..AccountSecret::default()
    })
}

fn devin_secret(root: &Path) -> Result<AccountSecret, String> {
    let key = devin::read_api_key_at(vscode_storage_dir(root))?
        .ok_or_else(|| "尚未登录 Devin / Windsurf，请先打开客户端并登录".to_string())?;
    Ok(AccountSecret {
        access_token: key,
        ..AccountSecret::default()
    })
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
