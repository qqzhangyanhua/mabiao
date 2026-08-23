//! 自定义提供商的两份文件：配置一份、凭证一份。
//!
//! 分开存的唯一目的是把密钥挡在备份之外——备份目录是设计成给人整个拷走的，
//! 密钥进去等于每次备份都在扩散凭证。因此配置文件（名称 / 类型 / 地址 / 开关）
//! 可以进备份，凭证文件不进。
//!
//! 换机器恢复备份后，配置还在、密钥为空。这时该提供商取数会报「未配置密钥」，
//! 是一个待办提示，不是坏掉了。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{CustomQuotaPreset, ID_PREFIX};

pub const CONFIG_NAME: &str = "custom_quota_providers.json";
pub const CREDENTIAL_NAME: &str = "custom_quota_credentials.json";
/// 掩码只留尾部这几位：够用户认出是哪把钥匙，又不足以拼回原文。
const TAIL_KEPT: usize = 4;

/// 一条自定义提供商的配置。**不含密钥**——密钥在凭证文件里按标识索引。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomQuotaProvider {
    /// 形如 `custom:a3f9c1`，随机生成，与内置 9 家的标识永不冲突。
    pub id: String,
    /// 纯展示标签，可改可重复。改名不改标识，额度缓存与告警去重记录跟着标识走。
    pub name: String,
    pub preset: CustomQuotaPreset,
    pub base_url: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomQuotaConfig {
    #[serde(default)]
    pub providers: Vec<CustomQuotaProvider>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomQuotaCredentials {
    /// 标识 → 密钥。
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
}

/// 配置与凭证合起来的一条：取数需要两边都有。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedProvider {
    pub config: CustomQuotaProvider,
    /// 凭证文件里没有这条时为 `None`（多半是恢复备份后密钥没跟过来）。
    pub secret: Option<String>,
}

/// 两份文件的路径。它们从 `AppState` 一路传到这里，永远成对出现，
/// 因此绑成一个类型——省得每一层都并排摆两个 `&Path`，也就不可能传反。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomQuotaPaths {
    pub config: PathBuf,
    pub credentials: PathBuf,
}

impl CustomQuotaPaths {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            config: dir.join(CONFIG_NAME),
            credentials: dir.join(CREDENTIAL_NAME),
        }
    }

    /// 应用数据目录下的那一份。命令层走 `AppState`，托盘与 CLI 走这条。
    pub fn app_data() -> Self {
        Self::in_dir(&crate::paths::app_data_dir())
    }
}

/// 读不动或解析不了都按「空」处理：这两份文件坏掉不该让整个额度区块打不开，
/// 用户在设置页重新填一遍即可。
fn load_json<T: Default + serde::de::DeserializeOwned>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn load_config(path: &Path) -> CustomQuotaConfig {
    load_json(path)
}

pub fn load_credentials(path: &Path) -> CustomQuotaCredentials {
    load_json(path)
}

/// 合流用的配置列表。`load_dto` 只画行、不取数，因此只要配置，不碰凭证文件——
/// 四个 DTO 装配点都走这条，省得各自去走一遍 `load_config(...).providers`。
pub fn load_configs(paths: &CustomQuotaPaths) -> Vec<CustomQuotaProvider> {
    load_config(&paths.config).providers
}

/// 取数用的合流入口：配置在、密钥缺，也要能表达出来。
pub fn load_providers(paths: &CustomQuotaPaths) -> Vec<ResolvedProvider> {
    let credentials = load_credentials(&paths.credentials);
    load_config(&paths.config)
        .providers
        .into_iter()
        .map(|config| {
            let secret = credentials
                .secrets
                .get(&config.id)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            ResolvedProvider { config, secret }
        })
        .collect()
}

pub fn save_config(path: &Path, config: &CustomQuotaConfig) -> Result<(), String> {
    write_json(path, config)
}

/// 凭证文件在 Unix 下收成 0600：这份文件里是明文密钥，同机器的其它用户不该读到。
pub fn save_credentials(path: &Path, credentials: &CustomQuotaCredentials) -> Result<(), String> {
    write_json(path, credentials)?;
    restrict_permissions(path);
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    // 收权限失败不该让保存失败——文件已经写好了，权限只是加固。
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

/// 随机标识，带 `custom:` 前缀。与已有标识撞了就重摇——本地条目个位数，
/// 撞了直接换一个比证明「不会撞」便宜得多。
pub fn new_provider_id(taken: &[String]) -> String {
    for _ in 0..64 {
        let candidate = format!("{ID_PREFIX}{:06x}", random_bits() & 0xff_ffff);
        if !taken.iter().any(|id| id == &candidate) {
            return candidate;
        }
    }
    format!("{ID_PREFIX}{:012x}", random_bits())
}

/// `RandomState` 的种子来自操作系统，每次 `new()` 都不同；标识只需要「不撞」，
/// 不需要密码学强度，因此不为它引一个随机数依赖。
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

/// 界面上永远只显示掩码：截图、投屏、录屏都不该把密钥带出去。
/// 没配密钥返回 `None`，让界面能区分「没填」和「填了但看不见」。
pub fn mask_secret(secret: Option<&str>) -> Option<String> {
    let secret = secret.map(str::trim).filter(|value| !value.is_empty())?;
    // 太短的密钥连尾巴都不给，否则掩码等于原文。
    if secret.chars().count() <= TAIL_KEPT {
        return Some("••••••".to_string());
    }
    let tail: String = {
        let mut last: Vec<char> = secret.chars().rev().take(TAIL_KEPT).collect();
        last.reverse();
        last.into_iter().collect()
    };
    Some(format!("••••••{tail}"))
}
