//! 设置页「自定义提供商」面板的命令契约。
//!
//! 三个动作：列出、保存（标识为空则新建）、删除。密钥在出口处一律换成掩码——
//! 界面永远拿不到明文，截图和投屏就不可能把它带出去。

use serde::{Deserialize, Serialize};

use super::store::{self, CustomQuotaPaths, CustomQuotaProvider};
use super::CustomQuotaPreset;

/// 列表里的一条：密钥只给掩码，不明文回显。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomQuotaProviderDto {
    pub id: String,
    pub name: String,
    pub preset: CustomQuotaPreset,
    pub base_url: String,
    pub enabled: bool,
    /// 已配置密钥时是掩码串；没配（多半是恢复备份后）为 null，界面据此给待办提示。
    pub secret_mask: Option<String>,
}

/// 预设类型选项。从枚举生成而不是前端写死，避免两边各列一份、补齐时漏改一处。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomQuotaPresetDto {
    pub value: String,
    pub label: String,
    /// 本版是否已有解析器。为 false 时界面给「暂未支持」提示。
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomQuotaPanelDto {
    pub providers: Vec<CustomQuotaProviderDto>,
    pub presets: Vec<CustomQuotaPresetDto>,
}

/// 保存的结果。带上刚存下的标识，好让界面立刻去取一次这一条的额度——
/// 否则首页那一行要挂着「暂无」直到用户自己点刷新，而「存完就看到那一行」
/// 正是这个功能的全部意义。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedCustomQuotaDto {
    pub saved_id: String,
    pub panel: CustomQuotaPanelDto,
}

/// 保存请求。标识为空 = 新建。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveCustomQuotaProvider {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub preset: CustomQuotaPreset,
    pub base_url: String,
    /// 留空 = 沿用现在的开关状态（新建时默认打开）。设置页目前没有这个开关，
    /// 不留空就会在改名时把用户手动关掉的那条悄悄打开。
    #[serde(default)]
    pub enabled: Option<bool>,
    /// 留空 = 沿用已存的密钥。编辑名称或地址时不必把密钥重打一遍，
    /// 而界面本来就只有掩码、也重打不出来。
    #[serde(default)]
    pub secret: Option<String>,
}

pub fn presets() -> Vec<CustomQuotaPresetDto> {
    CustomQuotaPreset::ALL
        .into_iter()
        .map(|preset| CustomQuotaPresetDto {
            value: preset.as_str().to_string(),
            label: preset.display_name().to_string(),
            supported: preset.implemented(),
        })
        .collect()
}

pub fn list(paths: &CustomQuotaPaths) -> CustomQuotaPanelDto {
    CustomQuotaPanelDto {
        providers: store::load_providers(paths)
            .into_iter()
            .map(|provider| CustomQuotaProviderDto {
                id: provider.config.id,
                name: provider.config.name,
                preset: provider.config.preset,
                base_url: provider.config.base_url,
                enabled: provider.config.enabled,
                secret_mask: store::mask_secret(provider.secret.as_deref()),
            })
            .collect(),
        presets: presets(),
    }
}

/// 保存**不做取数拦截**：断网、中转站临时抽风、或用户在飞机上先把配置填好，
/// 都不该变成「存不进去」。这里只挡填不动的错——名字空着、地址不成形。
///
/// 没实现的预设类型也照存不误，取数时给「暂未支持」的人话错误。
pub fn save(
    paths: &CustomQuotaPaths,
    request: SaveCustomQuotaProvider,
) -> Result<SavedCustomQuotaDto, String> {
    let name = request.name.trim().to_string();
    if name.is_empty() {
        return Err("请填写名称".to_string());
    }
    let base_url = request.base_url.trim().to_string();
    // 归一化只用来校验形状，存的仍是用户打的那串：重新打开表单时
    // 看到的应该是自己填的东西，而不是被应用悄悄改写过的版本。
    super::normalize_base_url(&base_url)?;

    let mut config = store::load_config(&paths.config);
    let secret = request
        .secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let id = match request
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => {
            let entry = config
                .providers
                .iter_mut()
                .find(|provider| provider.id == id)
                .ok_or_else(|| format!("这条自定义提供商已经不在了：{id}"))?;
            // 标识不动——额度缓存、退避状态、告警去重记录全部跟着它走，
            // 改名不该让首页那行短暂变空、也不该重复告警。
            entry.name = name;
            entry.preset = request.preset;
            entry.base_url = base_url;
            entry.enabled = request.enabled.unwrap_or(entry.enabled);
            id.to_string()
        }
        None => {
            if secret.is_none() {
                return Err("请填写密钥".to_string());
            }
            let taken: Vec<String> = config
                .providers
                .iter()
                .map(|provider| provider.id.clone())
                .collect();
            let id = store::new_provider_id(&taken);
            config.providers.push(CustomQuotaProvider {
                id: id.clone(),
                name,
                preset: request.preset,
                base_url,
                enabled: request.enabled.unwrap_or(true),
            });
            id
        }
    };

    // 配置先落盘、密钥后落盘。反过来的话，配置写失败会在磁盘上留下一把
    // 没有任何配置引用得到的密钥；这个顺序下最坏也只是「配置在、密钥没存上」，
    // 界面会提示重新填一次，是个能自己走出来的状态。
    store::save_config(&paths.config, &config)?;
    if let Some(secret) = secret {
        let mut credentials = store::load_credentials(&paths.credentials);
        credentials.secrets.insert(id.clone(), secret.to_string());
        store::save_credentials(&paths.credentials, &credentials)?;
    }
    Ok(SavedCustomQuotaDto {
        saved_id: id,
        panel: list(paths),
    })
}

/// 删除配置与密钥。sqlite 里那条额度缓存留着不管：`load_dto` 只认配置里列出的
/// 标识，删掉之后没有任何地方会再读它。它只在标识被重新摇到时才会重见天日，
/// 而 `new_provider_id` 只避开「当前配置里还在的」标识——概率是二十四位里撞一次，
/// 代价是新条目短暂显示上一条的余额，直到第一次取数覆盖它。
pub fn delete(paths: &CustomQuotaPaths, id: &str) -> Result<CustomQuotaPanelDto, String> {
    let mut config = store::load_config(&paths.config);
    let before = config.providers.len();
    config.providers.retain(|provider| provider.id != id);
    if config.providers.len() == before {
        return Err(format!("这条自定义提供商已经不在了：{id}"));
    }
    store::save_config(&paths.config, &config)?;

    let mut credentials = store::load_credentials(&paths.credentials);
    if credentials.secrets.remove(id).is_some() {
        store::save_credentials(&paths.credentials, &credentials)?;
    }
    Ok(list(paths))
}
