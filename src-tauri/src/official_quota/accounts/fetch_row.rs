//! 用已保存的 access token 取一条额度账号。不刷新、不把新 token 写回去。
//!
//! 当前本机那一条改读现场登录态，避免花掉文件里存着的 refresh token。

use std::path::{Path, PathBuf};

use super::capture;
use super::plan::Placement;
use super::{LocalLogin, ResolvedQuotaAccount, MISSING_SECRET_TODO};
use crate::domain::OfficialQuotaProvider;
use crate::official_quota::{
    antigravity, claude_usage, codex_usage, copilot, cursor, devin, droid, grok, opencode,
    QuotaSnapshot,
};

/// 一次账号取数要带上的材料。不含 refresh token：这条路径不会用它。
#[derive(Debug, Clone)]
pub struct AccountTarget {
    pub id: String,
    pub name: String,
    pub provider: OfficialQuotaProvider,
    pub access_token: String,
    pub account_id: Option<String>,
    /// 有值表示这是当前本机登录，取数时从这里重读 access token。
    pub live_root: Option<PathBuf>,
}

/// 自动刷新名单用。现场凭证读不到，或者没有 access token，就不放进名单。
pub(crate) fn prepare_auto_target(
    placement: &Placement<'_>,
    logins: &[LocalLogin],
) -> Option<AccountTarget> {
    let account = placement.account;
    let provider = OfficialQuotaProvider::parse(&account.config.provider)?;
    if placement.active_local {
        let login = matching_login(account, logins)?;
        let secret = capture::capture_secret(provider, &login.root).ok()?;
        if secret.access_token.trim().is_empty() {
            return None;
        }
        return Some(AccountTarget {
            id: account.config.id.clone(),
            name: account.config.name.clone(),
            provider,
            access_token: secret.access_token,
            account_id: secret.account_id,
            live_root: Some(login.root.clone()),
        });
    }
    let secret = account.secret.as_ref()?;
    if secret.access_token.trim().is_empty() {
        return None;
    }
    Some(AccountTarget {
        id: account.config.id.clone(),
        name: account.config.name.clone(),
        provider,
        access_token: secret.access_token.clone(),
        account_id: secret.account_id.clone(),
        live_root: None,
    })
}

pub(crate) fn prepare_manual_target(
    account: &ResolvedQuotaAccount,
    logins: &[LocalLogin],
) -> Result<AccountTarget, String> {
    let provider = OfficialQuotaProvider::parse(&account.config.provider)
        .ok_or_else(|| format!("未知的官方额度账号：{}", account.config.id))?;
    if let Some(login) = matching_login(account, logins) {
        return Ok(AccountTarget {
            id: account.config.id.clone(),
            name: account.config.name.clone(),
            provider,
            access_token: String::new(),
            account_id: None,
            live_root: Some(login.root.clone()),
        });
    }
    let Some(secret) = account
        .secret
        .as_ref()
        .filter(|secret| !secret.access_token.trim().is_empty())
    else {
        return Err(MISSING_SECRET_TODO.to_string());
    };
    Ok(AccountTarget {
        id: account.config.id.clone(),
        name: account.config.name.clone(),
        provider,
        access_token: secret.access_token.clone(),
        account_id: secret.account_id.clone(),
        live_root: None,
    })
}

fn matching_login<'a>(
    account: &ResolvedQuotaAccount,
    logins: &'a [LocalLogin],
) -> Option<&'a LocalLogin> {
    let provider = OfficialQuotaProvider::parse(&account.config.provider)?;
    logins
        .iter()
        .find(|login| login.provider == provider && login.fingerprint == account.config.fingerprint)
}

/// `live_root` 有值时只认现场 access token。`quota_base` 只改写 Grok 的额度地址，给本地桩用。
pub fn fetch_account_row(
    provider: OfficialQuotaProvider,
    stored_access_token: &str,
    stored_account_id: Option<&str>,
    live_root: Option<&Path>,
    quota_base: Option<&str>,
) -> Result<QuotaSnapshot, String> {
    let (token, account_id) = if let Some(root) = live_root {
        let secret = capture::capture_secret(provider, root)?;
        if secret.access_token.trim().is_empty() {
            return Err(MISSING_SECRET_TODO.to_string());
        }
        (secret.access_token, secret.account_id)
    } else {
        let token = stored_access_token.trim();
        if token.is_empty() {
            return Err(MISSING_SECRET_TODO.to_string());
        }
        (
            token.to_string(),
            stored_account_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        )
    };
    dispatch(provider, &token, account_id.as_deref(), quota_base)
}

fn dispatch(
    provider: OfficialQuotaProvider,
    token: &str,
    account_id: Option<&str>,
    quota_base: Option<&str>,
) -> Result<QuotaSnapshot, String> {
    match provider {
        OfficialQuotaProvider::Claude => {
            claude_usage::fetch_with_access_token(token).map(Into::into)
        }
        OfficialQuotaProvider::Codex => {
            let auth = codex_usage::CodexAuth {
                access_token: token.to_string(),
                account_id: account_id.map(str::to_string),
            };
            codex_usage::fetch_with_access_token(&auth)
        }
        OfficialQuotaProvider::Cursor => cursor::fetch_with_session_token(token),
        OfficialQuotaProvider::Grok => grok::fetch_with_access_token(token, account_id, quota_base),
        OfficialQuotaProvider::Droid => droid::fetch_with_access_token(token),
        OfficialQuotaProvider::Antigravity => antigravity::fetch_with_access_token(token),
        OfficialQuotaProvider::OpenCode => opencode::fetch_with_access_token(token).map(Into::into),
        OfficialQuotaProvider::Copilot => copilot::fetch_with_access_token(token),
        OfficialQuotaProvider::Devin => devin::fetch_with_access_token(token),
    }
}
