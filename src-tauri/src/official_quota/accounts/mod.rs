//! 额度账号：把「当前本机登录」或应用内授权收成一条可开关、可改名的行。
//!
//! 配置（标识、名称、掩码、指纹、开关）和密钥分两个文件。密钥从创建起就是
//! 0600，也不进备份——备份白名单是另一件事。界面和命令只拿掩码，拿不到原文。

mod capture;
mod copilot_device;
mod fetch_row;
pub(crate) mod plan;

pub use copilot_device::{
    open_copilot_verification, poll_copilot_device_login, start_copilot_device_login,
    CopilotDeviceEndpoints, CopilotDeviceLogin, CopilotDevicePoll, CopilotDeviceStartDto,
};

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::backoff;
use super::custom;
use crate::domain::OfficialQuotaProvider;
use crate::official_quota::{self, read_identity, QuotaIdentity};

pub use fetch_row::{fetch_account_row, AccountTarget};
pub(crate) use fetch_row::{prepare_auto_target, prepare_manual_target};

pub const CONFIG_NAME: &str = "quota_accounts.json";
pub const SECRET_NAME: &str = "quota_account_secrets.json";
/// 密钥文件里没有能直接取数的 access token。这是待办，不是取数失败。
pub const MISSING_SECRET_TODO: &str = "登录凭证还没保存，请在设置页重新导入";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaAccountSource {
    LocalSnapshot,
    /// 应用内授权拿到的独立会话。首期是 Copilot 设备码，不监听本地端口。
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

/// 以后取数要用的密钥。刷新写回不在这条路径里，字段先存着。
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
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
}

impl ResolvedQuotaAccount {
    pub fn access_token_missing(&self) -> bool {
        self.secret
            .as_ref()
            .map(|secret| secret.access_token.trim().is_empty())
            .unwrap_or(true)
    }
}

/// 离线探到的当前本机登录。`root` 是读到这份身份的目录或凭证文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalLogin {
    pub provider: OfficialQuotaProvider,
    pub fingerprint: String,
    pub root: PathBuf,
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
    /// 有额度账号才探本机默认登录。没有账号时不读那些目录。
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
            ResolvedQuotaAccount { config, secret }
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
    let identity = read_identity(provider, root)?;
    let secret = capture::capture_secret(provider, root)?;
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

/// 按这个提供方的默认登录位置依次试，第一个读到身份的算数。
pub fn import_default(
    paths: &QuotaAccountPaths,
    provider: OfficialQuotaProvider,
    backoff_path: &Path,
) -> Result<SavedQuotaAccountDto, String> {
    let roots = default_roots(provider);
    if roots.is_empty() {
        return Err("未找到本机登录态，请先在客户端登录".to_string());
    }
    let mut last = "未找到本机登录态，请先在客户端登录".to_string();
    for root in roots {
        match import_local_login(paths, provider, &root, backoff_path) {
            Ok(saved) => return Ok(saved),
            Err(error) => last = error,
        }
    }
    Err(last)
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

pub fn delete_account(paths: &QuotaAccountPaths, id: &str) -> Result<QuotaAccountPanelDto, String> {
    let mut config = load_config(&paths.config);
    let before = config.accounts.len();
    config.accounts.retain(|account| account.id != id);
    if config.accounts.len() == before {
        return Err(format!("这条额度账号已经不在了：{id}"));
    }
    save_config(&paths.config, &config)?;
    let mut secrets = load_secrets(&paths.secrets);
    if secrets.secrets.remove(id).is_some() {
        save_secrets(&paths.secrets, &secrets)?;
    }
    Ok(list(paths))
}

/// 只探已经登记过的提供方。一个都没有就直接返回，不去碰默认目录。
pub fn probe_default_logins(accounts: &[ResolvedQuotaAccount]) -> Vec<LocalLogin> {
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
        for root in default_roots(provider) {
            if let Ok(identity) = read_identity(provider, &root) {
                logins.push(LocalLogin {
                    provider,
                    fingerprint: identity.fingerprint,
                    root,
                });
                break;
            }
        }
    }
    logins
}

pub(crate) fn default_roots(provider: OfficialQuotaProvider) -> Vec<PathBuf> {
    match provider {
        OfficialQuotaProvider::Claude => vec![official_quota::claude_usage::credentials_path()],
        OfficialQuotaProvider::Codex => vec![official_quota::codex_usage::auth_path()],
        OfficialQuotaProvider::Cursor => crate::cursor_credentials::global_storage_dir()
            .into_iter()
            .collect(),
        OfficialQuotaProvider::Grok => vec![official_quota::grok::auth_path()],
        OfficialQuotaProvider::Droid => vec![official_quota::droid::factory_home()],
        OfficialQuotaProvider::Antigravity => official_quota::antigravity::APP_DIRS
            .into_iter()
            .filter_map(crate::vscode_state::global_storage_dir)
            .collect(),
        OfficialQuotaProvider::OpenCode => vec![official_quota::opencode::auth_path()],
        OfficialQuotaProvider::Copilot => vec![official_quota::copilot::config_home()],
        OfficialQuotaProvider::Devin => official_quota::devin::storage_dirs(),
    }
}

fn to_dto(config: &QuotaAccountConfig) -> QuotaAccountDto {
    QuotaAccountDto {
        id: config.id.clone(),
        provider: config.provider.clone(),
        name: config.name.clone(),
        hint: config.hint.clone(),
        source: config.source,
        enabled: config.enabled,
    }
}

fn load_config(path: &Path) -> QuotaAccountFile {
    load_json(path)
}

fn load_secrets(path: &Path) -> SecretFile {
    load_json(path)
}

fn load_json<T: Default + serde::de::DeserializeOwned>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_config(path: &Path, config: &QuotaAccountFile) -> Result<(), String> {
    write_json(path, config, false)
}

fn save_secrets(path: &Path, secrets: &SecretFile) -> Result<(), String> {
    write_json(path, secrets, true)
}

fn write_json<T: Serialize>(path: &Path, value: &T, private: bool) -> Result<(), String> {
    let bytes = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    write_atomic(path, bytes.as_bytes(), private)
}

/// 同目录临时文件再 `rename`。`private` 时临时文件创建就是 0600。
fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = parent.join(format!(".{file_name}.tmp"));
    let _ = std::fs::remove_file(&temp);

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;

    let written = options.open(&temp).and_then(|mut file| {
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()
    });
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("写入 {file_name} 失败：{error}"));
    }
    std::fs::rename(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        format!("写入 {file_name} 失败：{error}")
    })
}

fn new_account_id(slug: &str, taken: &[String]) -> String {
    for _ in 0..64 {
        let candidate = format!("acct:{slug}:{:016x}", random_bits());
        if !taken.iter().any(|id| id == &candidate) {
            return candidate;
        }
    }
    format!("acct:{slug}:{:016x}", random_bits())
}

fn random_bits() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos() as u64),
    );
    hasher.finish()
}
