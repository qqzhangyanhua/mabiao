//! 额度账号：把「当前本机登录」、隔离配置目录或应用内授权收成一条可开关、可改名的行。
//!
//! 配置（标识、名称、掩码、指纹、开关、隔离目录）和密钥分两个文件。密钥从创建起就是
//! 0600，也不进备份——备份白名单是另一件事。界面和命令只拿掩码，拿不到原文。
//! 隔离目录不把长期密钥复制进凭证文件。已分离快照的续期只写这份凭证文件。

mod capture;
mod codex_pkce;
mod copilot_device;
mod fetch_row;
pub(crate) mod plan;
mod refresh;
mod storage;

pub use codex_pkce::{
    open_codex_authorize, poll_codex_pkce_login, start_codex_pkce_login,
    submit_codex_pkce_callback, CodexPkceEndpoints, CodexPkceLogin, CodexPkcePoll,
    CodexPkceStartDto, CODEX_PKCE_REDIRECT_PORT,
};
pub use copilot_device::{
    open_copilot_verification, poll_copilot_device_login, start_copilot_device_login,
    CopilotDeviceEndpoints, CopilotDeviceLogin, CopilotDevicePoll, CopilotDeviceStartDto,
};

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::backoff;
use super::custom;
use crate::domain::OfficialQuotaProvider;
use crate::official_quota::{read_identity, read_identity_at, QuotaEnv, QuotaIdentity, QuotaPaths};

#[cfg(test)]
pub(crate) use fetch_row::prepare_manual_target;
pub use fetch_row::{
    fetch_account_row, fetch_managed_account, fetch_managed_account_in, AccountTarget,
};
pub(crate) use fetch_row::{prepare_auto_target_in, prepare_manual_target_in};
pub(crate) use storage::store_refreshed_tokens;
use storage::{
    forget_secret, load_config, load_secrets, new_account_id, random_bits, save_config,
    save_secrets, write_atomic,
};

pub const CONFIG_NAME: &str = "quota_accounts.json";
pub const SECRET_NAME: &str = "quota_account_secrets.json";
/// 密钥文件里没有能直接取数的 access token。这是待办，不是取数失败。
pub const MISSING_SECRET_TODO: &str = "登录凭证还没保存，请在设置页重新导入";
/// 刷新令牌被上游拒绝。保留上次的窗口，让用户自己去重新授权。
pub const LOGIN_EXPIRED: &str = "登录已失效，请在设置页重新授权或重新导入";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaAccountSource {
    LocalSnapshot,
    ProfileDir,
    /// 应用内授权拿到的独立会话。Codex 走 PKCE，Copilot 走设备码。
    Oauth,
}

/// 非敏感配置。指纹只用于防重和本机比对，不是令牌。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaAccountConfig {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub hint: String,
    pub fingerprint: String,
    pub source: QuotaAccountSource,
    /// 隔离目录的绝对路径。只有 `profile_dir` 会写它。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_dir: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct QuotaAccountFile {
    #[serde(default)]
    accounts: Vec<QuotaAccountConfig>,
}

/// 已分离快照的密钥。续期成功只改这份凭证，不改客户端目录。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSecret {
    #[serde(default)]
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_client_id: Option<String>,
    /// Antigravity 安装里扫到的客户端密钥。只在已分离快照续期时用，不展示。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// access token 的过期时刻（RFC3339）。没有就不靠它判断过期。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_expires_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SecretFile {
    #[serde(default)]
    secrets: std::collections::BTreeMap<String, AccountSecret>,
}

/// 配置和密钥合在一起。密钥文件缺这条时 `secret` 为 `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedQuotaAccount {
    pub config: QuotaAccountConfig,
    pub secret: Option<AccountSecret>,
    /// 续期回写用。不是配置的一部分，加载时填上。
    pub secrets_path: Option<PathBuf>,
}

impl ResolvedQuotaAccount {
    pub fn access_token_missing(&self) -> bool {
        if self.config.source == QuotaAccountSource::ProfileDir {
            return false;
        }
        match &self.secret {
            Some(secret) if !secret.access_token.trim().is_empty() => false,
            Some(secret) if refreshable(&self.config.provider) && has_refresh(secret) => false,
            _ => true,
        }
    }
}

pub(crate) fn refreshable_secret(provider: OfficialQuotaProvider, secret: &AccountSecret) -> bool {
    refreshable(provider.as_str()) && has_refresh(secret)
}

fn refreshable(provider: &str) -> bool {
    matches!(
        OfficialQuotaProvider::parse(provider),
        Some(
            OfficialQuotaProvider::Codex
                | OfficialQuotaProvider::Grok
                | OfficialQuotaProvider::Antigravity
        )
    )
}

fn has_refresh(secret: &AccountSecret) -> bool {
    secret
        .refresh_token
        .as_deref()
        .is_some_and(|token| !token.trim().is_empty())
}

pub(crate) fn fingerprints_match(stored: &str, live: &str) -> bool {
    let stored = stored.trim();
    let live = live.trim();
    !stored.is_empty() && stored == live
}

/// 离线探到的当前本机登录。
/// `root` 有值时只读那个目录或凭证文件；`None` 表示默认发现，取数时走和零配置相同的读者。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalLogin {
    pub provider: OfficialQuotaProvider,
    pub fingerprint: String,
    pub root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaAccountPaths {
    pub config: PathBuf,
    pub secrets: PathBuf,
}

impl QuotaAccountPaths {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            config: dir.join(CONFIG_NAME),
            secrets: dir.join(SECRET_NAME),
        }
    }

    pub fn app_data() -> Self {
        Self::in_dir(&crate::paths::app_data_dir())
    }
}

/// 桌面、托盘、CLI 共用的一份输入：自定义提供商、额度账号、本机登录。
#[derive(Debug, Clone)]
pub struct QuotaSnapshotInputs {
    pub custom: Vec<custom::ResolvedProvider>,
    pub accounts: Vec<ResolvedQuotaAccount>,
    pub logins: Vec<LocalLogin>,
}

impl QuotaSnapshotInputs {
    /// 有额度账号才探本机默认登录。没有额度账号时不读那些目录。
    pub fn from_app_data() -> Self {
        let custom = custom::store::load_providers(&custom::store::CustomQuotaPaths::app_data());
        let accounts = load_resolved(&QuotaAccountPaths::app_data());
        let logins = probe_default_logins(&accounts);
        Self {
            custom,
            accounts,
            logins,
        }
    }

    /// 测试和显式根目录用。`logins` 由调用方给，不探家目录。
    pub fn from_stored(app_data: &Path, logins: Vec<LocalLogin>) -> Self {
        Self {
            custom: custom::store::load_providers(&custom::store::CustomQuotaPaths::in_dir(
                app_data,
            )),
            accounts: load_resolved(&QuotaAccountPaths::in_dir(app_data)),
            logins,
        }
    }
}

/// 设置页看到的一条。没有指纹，没有令牌。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaAccountDto {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub hint: String,
    pub source: QuotaAccountSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_dir: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaAccountPanelDto {
    pub accounts: Vec<QuotaAccountDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedQuotaAccountDto {
    pub saved_id: String,
    pub enabled: bool,
    pub panel: QuotaAccountPanelDto,
}

/// 改名或开关。`None` 表示这一项不动。
#[derive(Debug, Clone, Deserialize)]
pub struct SaveQuotaAccount {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// 新增或编辑一条隔离目录账号。`id` 为空是新增。
#[derive(Debug, Clone, Deserialize)]
pub struct SaveProfileDirAccount {
    #[serde(default)]
    pub id: Option<String>,
    pub provider: String,
    pub name: String,
    pub profile_dir: String,
}

pub fn load_resolved(paths: &QuotaAccountPaths) -> Vec<ResolvedQuotaAccount> {
    let secrets = load_secrets(&paths.secrets);
    load_config(&paths.config)
        .accounts
        .into_iter()
        .map(|config| {
            let secret = secrets.secrets.get(&config.id).cloned().map(|mut secret| {
                secret.access_token = secret.access_token.trim().to_string();
                secret
            });
            ResolvedQuotaAccount {
                config,
                secret,
                secrets_path: Some(paths.secrets.clone()),
            }
        })
        .collect()
}

pub fn list(paths: &QuotaAccountPaths) -> QuotaAccountPanelDto {
    QuotaAccountPanelDto {
        accounts: load_config(&paths.config)
            .accounts
            .iter()
            .map(to_dto)
            .collect(),
    }
}

/// 从显式目录导入当前登录。同一提供方、同一指纹再导一次，沿用原来的 id。
pub fn import_local_login(
    paths: &QuotaAccountPaths,
    provider: OfficialQuotaProvider,
    root: &Path,
    backoff_path: &Path,
) -> Result<SavedQuotaAccountDto, String> {
    import_local_login_at(paths, provider, root, backoff_path, Utc::now())
}

pub fn import_local_login_at(
    paths: &QuotaAccountPaths,
    provider: OfficialQuotaProvider,
    root: &Path,
    backoff_path: &Path,
    now: DateTime<Utc>,
) -> Result<SavedQuotaAccountDto, String> {
    let identity = read_identity_at(provider, root, now)?;
    let secret = capture::capture_secret_at(provider, root, now)?;
    upsert_account(
        paths,
        provider,
        &identity,
        secret,
        QuotaAccountSource::LocalSnapshot,
        backoff_path,
    )
}

/// 同一提供方、同一指纹沿用原来的 id，并清掉这一条的退避。
/// 自定义过的名称（和当时的标签不同）保留。
fn upsert_account(
    paths: &QuotaAccountPaths,
    provider: OfficialQuotaProvider,
    identity: &QuotaIdentity,
    secret: AccountSecret,
    source: QuotaAccountSource,
    backoff_path: &Path,
) -> Result<SavedQuotaAccountDto, String> {
    let mut config = load_config(&paths.config);
    let mut secrets = load_secrets(&paths.secrets);
    let slug = provider.as_str();
    let id = if let Some(existing) = config
        .accounts
        .iter_mut()
        .find(|account| account.provider == slug && account.fingerprint == identity.fingerprint)
    {
        if existing.name == existing.hint {
            existing.name = identity.label.clone();
        }
        existing.hint = identity.label.clone();
        existing.source = source;
        if source != QuotaAccountSource::ProfileDir {
            existing.profile_dir = None;
        }
        existing.id.clone()
    } else {
        let id = new_account_id(
            slug,
            &config
                .accounts
                .iter()
                .map(|account| account.id.clone())
                .collect::<Vec<_>>(),
        );
        config.accounts.push(QuotaAccountConfig {
            id: id.clone(),
            provider: slug.to_string(),
            name: identity.label.clone(),
            hint: identity.label.clone(),
            fingerprint: identity.fingerprint.clone(),
            source,
            profile_dir: None,
            enabled: true,
        });
        id
    };
    secrets.secrets.insert(id.clone(), secret);
    save_config(&paths.config, &config)?;
    save_secrets(&paths.secrets, &secrets)?;
    backoff::clear(backoff_path, &id);
    let enabled = config
        .accounts
        .iter()
        .find(|account| account.id == id)
        .is_some_and(|account| account.enabled);
    Ok(SavedQuotaAccountDto {
        saved_id: id,
        enabled,
        panel: list(paths),
    })
}

/// 按这个提供方的默认登录位置导入。Antigravity / Droid 走和零配置取数相同的发现顺序。
pub fn import_default(
    paths: &QuotaAccountPaths,
    provider: OfficialQuotaProvider,
    backoff_path: &Path,
) -> Result<SavedQuotaAccountDto, String> {
    let mut env = QuotaEnv::live(QuotaPaths::app_data());
    env.paths.accounts = paths.clone();
    env.paths.backoff = backoff_path.to_path_buf();
    import_default_in(&env, provider)
}

pub fn import_default_in(
    env: &QuotaEnv,
    provider: OfficialQuotaProvider,
) -> Result<SavedQuotaAccountDto, String> {
    let paths = &env.paths.accounts;
    let backoff_path = &env.paths.backoff;
    match provider {
        OfficialQuotaProvider::Antigravity => {
            let tokens = crate::official_quota::antigravity::load_local_tokens_in(env)?;
            let identity =
                crate::official_quota::identity::identity_from_antigravity_tokens(&tokens)?;
            let secret = capture::antigravity_secret_from_tokens(&tokens)?;
            upsert_account(
                paths,
                provider,
                &identity,
                secret,
                QuotaAccountSource::LocalSnapshot,
                backoff_path,
            )
        }
        OfficialQuotaProvider::Droid => {
            let plain = crate::official_quota::droid::load_default_auth_plaintext_in(env)?
                .ok_or_else(|| {
                    "尚未登录 Droid，请先运行 droid 并登录 app.factory.ai".to_string()
                })?;
            let identity = crate::official_quota::identity::identity_from_droid_plaintext(&plain)?;
            let secret = capture::droid_secret_from_plaintext(&plain)?;
            upsert_account(
                paths,
                provider,
                &identity,
                secret,
                QuotaAccountSource::LocalSnapshot,
                backoff_path,
            )
        }
        other => {
            let roots = env.default_roots(other);
            if roots.is_empty() {
                return Err("未找到本机登录态，请先在客户端登录".to_string());
            }
            let mut last = "未找到本机登录态，请先在客户端登录".to_string();
            for root in roots {
                match import_local_login_at(paths, other, &root, backoff_path, env.now()) {
                    Ok(saved) => return Ok(saved),
                    Err(error) => last = error,
                }
            }
            Err(last)
        }
    }
}

pub fn save(
    paths: &QuotaAccountPaths,
    request: SaveQuotaAccount,
    backoff_path: &Path,
) -> Result<SavedQuotaAccountDto, String> {
    let mut config = load_config(&paths.config);
    let account = config
        .accounts
        .iter_mut()
        .find(|account| account.id == request.id)
        .ok_or_else(|| format!("未知的官方额度账号：{}", request.id))?;
    if let Some(name) = request.name {
        let name = name.trim();
        if name.is_empty() {
            return Err("请填写名称".to_string());
        }
        account.name = name.to_string();
    }
    if let Some(enabled) = request.enabled {
        account.enabled = enabled;
    }
    let id = account.id.clone();
    let enabled = account.enabled;
    save_config(&paths.config, &config)?;
    backoff::clear(backoff_path, &id);
    Ok(SavedQuotaAccountDto {
        saved_id: id,
        enabled,
        panel: list(paths),
    })
}

/// 登记或改一条隔离目录。目录必须已经存在。不把目录里的长期密钥复制进凭证文件。
pub fn save_profile_dir(
    paths: &QuotaAccountPaths,
    request: SaveProfileDirAccount,
    backoff_path: &Path,
) -> Result<SavedQuotaAccountDto, String> {
    let provider = OfficialQuotaProvider::parse(request.provider.trim())
        .ok_or_else(|| format!("未知的官方额度账号：{}", request.provider.trim()))?;
    let name = request.name.trim();
    if name.is_empty() {
        return Err("请填写名称".to_string());
    }
    let directory = validated_profile_dir(&request.profile_dir)?;
    let directory_text = directory.to_string_lossy().into_owned();
    let identity = read_identity(provider, &directory).ok();
    let mut config = load_config(&paths.config);
    let slug = provider.as_str();
    let requested_id = request
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let id = if let Some(id) = requested_id {
        let account = config
            .accounts
            .iter_mut()
            .find(|account| account.id == id)
            .ok_or_else(|| format!("未知的官方额度账号：{id}"))?;
        account.provider = slug.to_string();
        account.name = name.to_string();
        account.source = QuotaAccountSource::ProfileDir;
        account.profile_dir = Some(directory_text);
        if let Some(identity) = identity {
            account.fingerprint = identity.fingerprint;
            account.hint = identity.label;
        }
        account.id.clone()
    } else if let Some(account) = config.accounts.iter_mut().find(|account| {
        account.source == QuotaAccountSource::ProfileDir
            && account.provider == slug
            && account.profile_dir.as_deref() == Some(directory_text.as_str())
    }) {
        account.name = name.to_string();
        if let Some(identity) = identity {
            account.fingerprint = identity.fingerprint;
            account.hint = identity.label;
        }
        account.id.clone()
    } else {
        let id = new_account_id(
            slug,
            &config
                .accounts
                .iter()
                .map(|account| account.id.clone())
                .collect::<Vec<_>>(),
        );
        let (fingerprint, hint) = match identity {
            Some(identity) => (identity.fingerprint, identity.label),
            None => (String::new(), name.to_string()),
        };
        config.accounts.push(QuotaAccountConfig {
            id: id.clone(),
            provider: slug.to_string(),
            name: name.to_string(),
            hint,
            fingerprint,
            source: QuotaAccountSource::ProfileDir,
            profile_dir: Some(directory_text),
            enabled: true,
        });
        id
    };
    save_config(&paths.config, &config)?;
    forget_secret(&paths.secrets, &id)?;
    backoff::clear(backoff_path, &id);
    let enabled = config
        .accounts
        .iter()
        .find(|account| account.id == id)
        .is_some_and(|account| account.enabled);
    Ok(SavedQuotaAccountDto {
        saved_id: id,
        enabled,
        panel: list(paths),
    })
}

fn validated_profile_dir(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("请填写配置目录".to_string());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err("配置目录要用绝对路径".to_string());
    }
    if !path.exists() {
        return Err("配置目录不存在".to_string());
    }
    if !path.is_dir() {
        return Err("配置目录必须是文件夹".to_string());
    }
    Ok(std::fs::canonicalize(&path).unwrap_or(path))
}

pub fn delete_account(paths: &QuotaAccountPaths, id: &str) -> Result<QuotaAccountPanelDto, String> {
    let mut config = load_config(&paths.config);
    let before = config.accounts.len();
    config.accounts.retain(|account| account.id != id);
    if config.accounts.len() == before {
        return Err(format!("这条额度账号已经不在了：{id}"));
    }
    save_config(&paths.config, &config)?;
    forget_secret(&paths.secrets, id)?;
    Ok(list(paths))
}

/// 只探已经登记过的提供方。一个都没有就直接返回，不去碰默认目录。
pub fn probe_default_logins(accounts: &[ResolvedQuotaAccount]) -> Vec<LocalLogin> {
    probe_default_logins_in(&QuotaEnv::live(QuotaPaths::app_data()), accounts)
}

pub fn probe_default_logins_in(
    env: &QuotaEnv,
    accounts: &[ResolvedQuotaAccount],
) -> Vec<LocalLogin> {
    if accounts.is_empty() {
        return Vec::new();
    }
    let mut logins = Vec::new();
    for provider in OfficialQuotaProvider::ALL {
        if !accounts
            .iter()
            .any(|account| account.config.provider == provider.as_str())
        {
            continue;
        }
        if let Some(login) = probe_provider_in(env, provider) {
            logins.push(login);
        }
    }
    logins
}

fn probe_provider_in(env: &QuotaEnv, provider: OfficialQuotaProvider) -> Option<LocalLogin> {
    match provider {
        OfficialQuotaProvider::Antigravity => {
            let tokens = crate::official_quota::antigravity::load_local_tokens_in(env).ok()?;
            let identity =
                crate::official_quota::identity::identity_from_antigravity_tokens(&tokens).ok()?;
            Some(LocalLogin {
                provider,
                fingerprint: identity.fingerprint,
                root: None,
            })
        }
        OfficialQuotaProvider::Droid => {
            let plain =
                crate::official_quota::droid::load_default_auth_plaintext_in(env).ok()??;
            let identity =
                crate::official_quota::identity::identity_from_droid_plaintext(&plain).ok()?;
            Some(LocalLogin {
                provider,
                fingerprint: identity.fingerprint,
                root: None,
            })
        }
        other => {
            for root in env.default_roots(other) {
                if let Ok(identity) = read_identity_at(other, &root, env.now()) {
                    return Some(LocalLogin {
                        provider,
                        fingerprint: identity.fingerprint,
                        root: Some(root),
                    });
                }
            }
            None
        }
    }
}

fn to_dto(config: &QuotaAccountConfig) -> QuotaAccountDto {
    QuotaAccountDto {
        id: config.id.clone(),
        provider: config.provider.clone(),
        name: config.name.clone(),
        hint: config.hint.clone(),
        source: config.source,
        profile_dir: config.profile_dir.clone(),
        enabled: config.enabled,
    }
}

/// 探测和刷新前，隔离目录的身份以目录里当前读到的为准。读不到就留着保存时的指纹。
pub(crate) fn with_live_profiles_at(
    accounts: &[ResolvedQuotaAccount],
    now: DateTime<Utc>,
) -> Vec<ResolvedQuotaAccount> {
    accounts
        .iter()
        .map(|account| live_profile(account, now))
        .collect()
}

fn live_profile(account: &ResolvedQuotaAccount, now: DateTime<Utc>) -> ResolvedQuotaAccount {
    let mut account = account.clone();
    if account.config.source != QuotaAccountSource::ProfileDir {
        return account;
    }
    let Some(dir) = account
        .config
        .profile_dir
        .as_deref()
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
    else {
        return account;
    };
    let Some(provider) = OfficialQuotaProvider::parse(&account.config.provider) else {
        return account;
    };
    if let Ok(identity) = read_identity_at(provider, Path::new(dir), now) {
        account.config.fingerprint = identity.fingerprint;
        account.config.hint = identity.label;
    }
    account
}
