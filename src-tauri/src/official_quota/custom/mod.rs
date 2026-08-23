//! 自定义提供商：用户在设置页自行登记的、按内置预设类型取数的账号级额度来源。
//!
//! 与内置 9 家是**平行通道**：`OfficialQuotaProvider` 枚举及其穷尽匹配一行不改，
//! 这里自成一路，两边各自产出额度行，在 `official_quota::load_dto` 处合流。
//! 合流点是整个应用取用额度数据的唯一出口，因此首页、托盘、告警全部零改动。
//!
//! 边界见 `docs/adr/0012-custom-quota-providers.md`：只允许内置预设类型、
//! 只打计费 / 余额接口、不进消耗记录、不进本机 token KPI、凭证不进备份。

pub mod openai_compatible;
pub mod panel;
pub mod store;

use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::domain::OfficialQuotaWindow;

pub use store::{CustomQuotaConfig, CustomQuotaCredentials, CustomQuotaProvider, ResolvedProvider};

/// 标识前缀。三个职责：与内置 9 家永不冲突、作为托盘「最紧一档」的跳过判据、
/// 界面上一眼分辨自定义与内置。
pub const ID_PREFIX: &str = "custom:";
const TIMEOUT: Duration = Duration::from_secs(15);
const MISSING_SECRET: &str = "未配置密钥，请在设置页重新填写";
/// 「暂未支持」错误的识别标记，`is_precheck_error` 靠它认。
const UNSUPPORTED_MARK: &str = "暂未支持";

pub fn is_custom_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

/// 预设类型。**一次性定义齐 6 种**，避免后续补齐解析器时再动枚举；
/// 本版只实现「OpenAI 兼容计费」，其余走 `unsupported`，给明确的暂未支持提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CustomQuotaPreset {
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
    #[serde(rename = "newapi")]
    NewApi,
    #[serde(rename = "openrouter")]
    OpenRouter,
    #[serde(rename = "deepseek")]
    DeepSeek,
    #[serde(rename = "siliconflow")]
    SiliconFlow,
    #[serde(rename = "moonshot")]
    Moonshot,
}

impl CustomQuotaPreset {
    pub const ALL: [Self; 6] = [
        Self::OpenAiCompatible,
        Self::NewApi,
        Self::OpenRouter,
        Self::DeepSeek,
        Self::SiliconFlow,
        Self::Moonshot,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai_compatible",
            Self::NewApi => "newapi",
            Self::OpenRouter => "openrouter",
            Self::DeepSeek => "deepseek",
            Self::SiliconFlow => "siliconflow",
            Self::Moonshot => "moonshot",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "OpenAI 兼容计费",
            Self::NewApi => "NewAPI / OneAPI 用户接口",
            Self::OpenRouter => "OpenRouter",
            Self::DeepSeek => "DeepSeek",
            Self::SiliconFlow => "硅基流动",
            Self::Moonshot => "Moonshot",
        }
    }

    /// 本版是否已有解析器。界面据此把没实现的那几档标灰。
    pub fn implemented(self) -> bool {
        matches!(self, Self::OpenAiCompatible)
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.as_str() == value)
    }
}

fn unsupported(preset: CustomQuotaPreset) -> String {
    format!(
        "「{}」{UNSUPPORTED_MARK}，当前只实现了「{}」",
        preset.display_name(),
        CustomQuotaPreset::OpenAiCompatible.display_name()
    )
}

/// base URL 归一化：剥掉结尾斜杠、剥掉结尾的 `/v1`。
///
/// 只在 Rust 存在这一份，前端不重写——否则界面上写的和真正请求的会各自漂移。
/// 「根地址 / 带 `/v1` / 带结尾斜杠 / 两者都带」四种写法归到同一个地址。
pub fn normalize_base_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("请填写 base URL".to_string());
    }
    // 协议头先认、再剥尾巴。反过来的话 `https://` 被剥成 `https:`，
    // 报的会是「要以 https:// 开头」——用户明明写了，只会更糊涂。
    let scheme = ["https://", "http://"]
        .into_iter()
        .find(|scheme| trimmed.starts_with(scheme))
        .ok_or_else(|| "base URL 需要以 http:// 或 https:// 开头".to_string())?;
    let rest = trimmed[scheme.len()..].trim_end_matches('/');
    let rest = rest
        .strip_suffix("/v1")
        .unwrap_or(rest)
        .trim_end_matches('/');
    if rest.is_empty() {
        return Err("base URL 只有协议头，缺少域名".to_string());
    }
    Ok(format!("{scheme}{rest}"))
}

/// 取数时**真正会请求**的地址，按预设类型分派。未实现的类型在这里就拦下来。
pub fn request_urls(
    preset: CustomQuotaPreset,
    base_url: &str,
    today: chrono::NaiveDate,
) -> Result<Vec<String>, String> {
    let base = normalize_base_url(base_url)?;
    match preset {
        CustomQuotaPreset::OpenAiCompatible => Ok(openai_compatible::urls(&base, today)),
        other => Err(unsupported(other)),
    }
}

/// 原始响应体 → 额度窗口。**按预设类型分派、只有一个入口**：后续补齐其余 5 种
/// 预设时接缝数不增长，新解析器直接复用同一个测试入口。
///
/// `bodies` 与 `request_urls` 的返回一一对应。
pub fn parse_quota(
    preset: CustomQuotaPreset,
    bodies: &[&str],
) -> Result<Vec<OfficialQuotaWindow>, String> {
    match preset {
        CustomQuotaPreset::OpenAiCompatible => openai_compatible::parse(bodies),
        other => Err(unsupported(other)),
    }
}

pub fn fetch(provider: &ResolvedProvider) -> super::ProviderFetch {
    if let Some(blocked) = precheck(provider) {
        return Err(blocked);
    }
    let secret = provider
        .secret
        .as_deref()
        .ok_or_else(|| MISSING_SECRET.to_string())?;
    let urls = request_urls(
        provider.config.preset,
        &provider.config.base_url,
        Utc::now().date_naive(),
    )?;
    let bodies = urls
        .iter()
        .map(|url| request(url, secret))
        .collect::<Result<Vec<String>, String>>()?;
    let borrowed: Vec<&str> = bodies.iter().map(String::as_str).collect();
    let windows = parse_quota(provider.config.preset, &borrowed)?;
    Ok((windows, Utc::now().to_rfc3339()))
}

/// 不用打网就能判定的失败：密钥没配、预设还没实现。
///
/// 单独拎出来是给退避看的——退避存在的理由是「别把对方打挂」，而这两种
/// 压根没碰到对方。记进退避的话，恢复备份后刚填完密钥、或刚存下一个未实现的
/// 预设，再点刷新只会看到「刚取数失败，N 分钟后自动重试」，把真正的原因盖掉。
pub fn precheck(provider: &ResolvedProvider) -> Option<String> {
    if provider.secret.is_none() {
        return Some(MISSING_SECRET.to_string());
    }
    if !provider.config.preset.implemented() {
        return Some(unsupported(provider.config.preset));
    }
    None
}

/// 这条错误是不是「压根没打网」。`backoff::is_rate_limited` 也是按标记认的，
/// 沿用同一套办法：错误目前就是纯字符串。
pub fn is_precheck_error(error: &str) -> bool {
    error == MISSING_SECRET || error.contains(UNSUPPORTED_MARK)
}

/// 错误一律翻成人话：用户要判断的是「去充值 / 换密钥 / 等网络」，
/// 一个裸的 HTTP 码回答不了这个问题。
fn request(url: &str, secret: &str) -> Result<String, String> {
    // 目前唯一实现的预设走 Bearer；后续 NewAPI 那档需要另一套头，
    // 到时按预设分派，不要在这里堆 if。
    let request = crate::net::agent_with_timeout(TIMEOUT)
        .get(url)
        .set("Authorization", &format!("Bearer {secret}"))
        .set("Accept", "application/json");
    match request.call() {
        Ok(response) => response
            .into_string()
            .map_err(|_| "读取响应失败，接口返回的内容不是文本".to_string()),
        Err(ureq::Error::Status(401 | 403, _)) => {
            Err("密钥无效或已失效，请在设置页更新密钥".to_string())
        }
        Err(ureq::Error::Status(404, _)) => {
            Err("地址不对：接口不存在，请检查 base URL 与预设类型是否匹配".to_string())
        }
        Err(ureq::Error::Status(429, _)) => Err("对方限流了，稍后会自动重试".to_string()),
        Err(ureq::Error::Status(code, _)) => Err(format!(
            "接口返回异常（HTTP {code}），请确认 base URL 与预设类型是否匹配"
        )),
        Err(_) => Err("网络不通，连不上这个地址，请检查网络或代理设置".to_string()),
    }
}
