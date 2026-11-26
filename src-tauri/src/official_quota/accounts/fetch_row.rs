//! 用已保存的 access token 取一条额度账号。
//!
//! `fetch_account_row` 不刷新、不把新 token 写回去。已分离的 Codex / Grok / Antigravity
//! 走 `fetch_managed_account`：过期或上游拒绝时在内存里续期，只回写码表自己的凭证文件。
//! 当前本机和隔离目录只读现场登录态，不花 refresh token。

use std::path::{Path, PathBuf};

use base64::Engine;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::capture;
use super::plan::Placement;
use super::refresh::{self, RefreshFail};
use super::{
    fingerprints_match, LocalLogin, QuotaAccountSource, ResolvedQuotaAccount, LOGIN_EXPIRED,
    MISSING_SECRET_TODO,
};
use crate::domain::OfficialQuotaProvider;
use crate::official_quota::{
    antigravity, claude_usage, codex_usage, copilot, cursor, devin, droid, grok, opencode,
    QuotaSnapshot,
};

/// 一次账号取数要带上的材料。`refresh` 只在已分离快照上有值。
#[derive(Debug, Clone)]
pub struct AccountTarget {
    pub id: String,
    pub name: String,
    pub provider: OfficialQuotaProvider,
    pub access_token: String,
    pub account_id: Option<String>,
    /// 有值表示取数时从这里重读 access token：显式目录上的当前本机登录，或隔离目录。
    pub live_root: Option<PathBuf>,
    /// 默认发现：取数时走和零配置相同的读者，不指向某个文件，也不续期。
    pub live_default: bool,
    /// 已分离快照的续期材料。当前本机和隔离目录保持空。
    pub refresh: Option<super::AccountSecret>,
    pub secrets_path: Option<PathBuf>,
}

/// 自动刷新名单用。现场凭证读不到，或者没有能取数的材料，就不放进名单。
pub(crate) fn prepare_auto_target(
    placement: &Placement<'_>,
    logins: &[LocalLogin],
) -> Option<AccountTarget> {
    let account = placement.account;
    let provider = OfficialQuotaProvider::parse(&account.config.provider)?;
    if placement.active_local {
        let login = matching_login(account, logins)?;
        return live_from_login(account, provider, login, true);
    }
    if account.config.source == QuotaAccountSource::ProfileDir {
        let dir = profile_dir(account)?;
        return Some(live_target(
            account,
            provider,
            String::new(),
            None,
            PathBuf::from(dir),
        ));
    }
    separated_target(account, provider)
}

pub(crate) fn prepare_manual_target(
    account: &ResolvedQuotaAccount,
    logins: &[LocalLogin],
) -> Result<AccountTarget, String> {
    let provider = OfficialQuotaProvider::parse(&account.config.provider)
        .ok_or_else(|| format!("未知的官方额度账号：{}", account.config.id))?;
    if let Some(login) = matching_login(account, logins) {
        return live_from_login(account, provider, login, false)
            .ok_or_else(|| MISSING_SECRET_TODO.to_string());
    }
    if account.config.source == QuotaAccountSource::ProfileDir {
        let dir = profile_dir(account).ok_or_else(|| "配置目录不存在".to_string())?;
        return Ok(live_target(
            account,
            provider,
            String::new(),
            None,
            PathBuf::from(dir),
        ));
    }
    separated_target(account, provider).ok_or_else(|| MISSING_SECRET_TODO.to_string())
}

fn live_from_login(
    account: &ResolvedQuotaAccount,
    provider: OfficialQuotaProvider,
    login: &LocalLogin,
    require_access: bool,
) -> Option<AccountTarget> {
    let Some(root) = login.root.clone() else {
        if !require_access {
            return Some(default_live_target(account, provider, String::new(), None));
        }
        let secret = capture::capture_default(provider).ok()?;
        if secret.access_token.trim().is_empty() {
            return None;
        }
        return Some(default_live_target(
            account,
            provider,
            secret.access_token,
            secret.account_id,
        ));
    };
    if !require_access {
        return Some(live_target(account, provider, String::new(), None, root));
    }
    let secret = capture::capture_secret(provider, &root).ok()?;
    if secret.access_token.trim().is_empty() {
        return None;
    }
    Some(live_target(
        account,
        provider,
        secret.access_token,
        secret.account_id,
        root,
    ))
}

fn default_live_target(
    account: &ResolvedQuotaAccount,
    provider: OfficialQuotaProvider,
    access_token: String,
    account_id: Option<String>,
) -> AccountTarget {
    AccountTarget {
        id: account.config.id.clone(),
        name: account.config.name.clone(),
        provider,
        access_token,
        account_id,
        live_root: None,
        live_default: true,
        refresh: None,
        secrets_path: None,
    }
}

fn live_target(
    account: &ResolvedQuotaAccount,
    provider: OfficialQuotaProvider,
    access_token: String,
    account_id: Option<String>,
    root: PathBuf,
) -> AccountTarget {
    AccountTarget {
        id: account.config.id.clone(),
        name: account.config.name.clone(),
        provider,
        access_token,
        account_id,
        live_root: Some(root),
        live_default: false,
        refresh: None,
        secrets_path: None,
    }
}

fn separated_target(
    account: &ResolvedQuotaAccount,
    provider: OfficialQuotaProvider,
) -> Option<AccountTarget> {
    let secret = account.secret.clone();
    let refreshable = secret.as_ref().is_some_and(|secret| {
        super::refreshable_secret(provider, secret) && account.secrets_path.is_some()
    });
    let access_token = secret
        .as_ref()
        .map(|secret| secret.access_token.clone())
        .unwrap_or_default();
    if access_token.trim().is_empty() && !refreshable {
        return None;
    }
    Some(AccountTarget {
        id: account.config.id.clone(),
        name: account.config.name.clone(),
        provider,
        access_token,
        account_id: secret.and_then(|secret| secret.account_id),
        live_root: None,
        live_default: false,
        refresh: if refreshable {
            secret_of(account)
        } else {
            None
        },
        secrets_path: account.secrets_path.clone(),
    })
}

fn secret_of(account: &ResolvedQuotaAccount) -> Option<super::AccountSecret> {
    account.secret.clone()
}

fn profile_dir(account: &ResolvedQuotaAccount) -> Option<&str> {
    account
        .config
        .profile_dir
        .as_deref()
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
}

fn matching_login<'a>(
    account: &ResolvedQuotaAccount,
    logins: &'a [LocalLogin],
) -> Option<&'a LocalLogin> {
    let provider = OfficialQuotaProvider::parse(&account.config.provider)?;
    logins.iter().find(|login| {
        login.provider == provider
            && fingerprints_match(&account.config.fingerprint, &login.fingerprint)
    })
}

/// `live_root` 有值时只认现场 access token。`quota_base` 把额度地址换到本地桩。
/// 不刷新。
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

/// 已分离快照会在 access token 过期或上游拒绝登录时续期一次。
/// 当前本机和隔离目录（`live_root`）不续期。`quota_base` 只给测试桩。
pub fn fetch_managed_account(
    target: &AccountTarget,
    quota_base: Option<&str>,
) -> Result<QuotaSnapshot, String> {
    if target.live_default {
        let secret = capture::capture_default(target.provider)?;
        if secret.access_token.trim().is_empty() {
            return Err(MISSING_SECRET_TODO.to_string());
        }
        return dispatch(
            target.provider,
            &secret.access_token,
            secret.account_id.as_deref(),
            quota_base,
        );
    }
    if target.live_root.is_some() || target.refresh.is_none() {
        return fetch_account_row(
            target.provider,
            &target.access_token,
            target.account_id.as_deref(),
            target.live_root.as_deref(),
            quota_base,
        );
    }
    let mut token = target.access_token.clone();
    let mut refreshed = false;
    if access_needs_refresh(&token, target.refresh.as_ref(), Utc::now()) {
        token = apply_refresh(target)?;
        refreshed = true;
    }
    match dispatch(
        target.provider,
        &token,
        target.account_id.as_deref(),
        quota_base,
    ) {
        Ok(snapshot) => Ok(snapshot),
        Err(error) if !refreshed && is_auth_failure(&error) => {
            let token = apply_refresh(target)?;
            dispatch(
                target.provider,
                &token,
                target.account_id.as_deref(),
                quota_base,
            )
        }
        Err(error) => Err(error),
    }
}

fn apply_refresh(target: &AccountTarget) -> Result<String, String> {
    let Some(secret) = target.refresh.as_ref() else {
        return Err(MISSING_SECRET_TODO.to_string());
    };
    let Some(path) = target.secrets_path.as_deref() else {
        return Err(MISSING_SECRET_TODO.to_string());
    };
    let refreshed = refresh::refresh_separated(&refresh::RefreshRequest {
        provider: target.provider,
        secret,
    })
    .map_err(|fail| match fail {
        RefreshFail::Rejected => LOGIN_EXPIRED.to_string(),
        RefreshFail::Other(message) => message,
    })?;
    super::store_refreshed_tokens(
        path,
        &target.id,
        &refreshed.access_token,
        refreshed.refresh_token.as_deref(),
    )?;
    Ok(refreshed.access_token)
}

fn access_needs_refresh(
    token: &str,
    secret: Option<&super::AccountSecret>,
    now: DateTime<Utc>,
) -> bool {
    if token.trim().is_empty() {
        return true;
    }
    if let Some(secret) = secret {
        if stored_expiry_passed(secret.access_expires_at.as_deref(), now) {
            return true;
        }
    }
    jwt_expired(token, now.timestamp())
}

fn stored_expiry_passed(raw: Option<&str>, now: DateTime<Utc>) -> bool {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    DateTime::parse_from_rfc3339(raw)
        .map(|time| time.with_timezone(&Utc) <= now)
        .unwrap_or(false)
}

fn jwt_expired(token: &str, now_secs: i64) -> bool {
    let Some(payload) = token.split('.').nth(1).filter(|part| !part.is_empty()) else {
        return false;
    };
    let Ok(bytes) =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('='))
    else {
        return false;
    };
    let Ok(claims) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    claims
        .get("exp")
        .and_then(Value::as_i64)
        .is_some_and(|exp| exp <= now_secs)
}

fn is_auth_failure(error: &str) -> bool {
    error.contains("登录已失效")
        || error.contains("登录已过期")
        || error.contains("HTTP 401")
        || error.contains("HTTP 403")
        || error.contains("Unauthorized")
}

fn dispatch(
    provider: OfficialQuotaProvider,
    token: &str,
    account_id: Option<&str>,
    quota_base: Option<&str>,
) -> Result<QuotaSnapshot, String> {
    match provider {
        OfficialQuotaProvider::Claude => {
            claude_usage::fetch_with_access_token_at(token, quota_base).map(Into::into)
        }
        OfficialQuotaProvider::Codex => {
            let auth = codex_usage::CodexAuth {
                access_token: token.to_string(),
                account_id: account_id.map(str::to_string),
            };
            codex_usage::fetch_with_access_token_at(&auth, quota_base)
        }
        OfficialQuotaProvider::Cursor => cursor::fetch_with_session_token_at(token, quota_base),
        OfficialQuotaProvider::Grok => grok::fetch_with_access_token(token, account_id, quota_base),
        OfficialQuotaProvider::Droid => droid::fetch_with_access_token_at(token, quota_base),
        OfficialQuotaProvider::Antigravity => {
            antigravity::fetch_with_access_token_at(token, quota_base)
        }
        OfficialQuotaProvider::OpenCode => {
            opencode::fetch_with_access_token_at(token, quota_base).map(Into::into)
        }
        OfficialQuotaProvider::Copilot => copilot::fetch_with_access_token_at(token, quota_base),
        OfficialQuotaProvider::Devin => devin::fetch_with_access_token_at(token, quota_base),
    }
}
