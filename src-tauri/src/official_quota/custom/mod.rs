//! 自定义提供商：用户在设置页自行登记的、按内置预设类型取数的账号级额度来源。
//!
//! 与内置 9 家是**平行通道**：`OfficialQuotaProvider` 枚举及其穷尽匹配一行不改，
//! 这里自成一路，两边各自产出额度行，在 `official_quota::load_dto` 处合流。
//! 合流点是整个应用取用额度数据的唯一出口，因此首页、托盘、告警全部零改动。
//!
//! 边界见 `docs/adr/0012-custom-quota-providers.md`：只允许内置预设类型、
//! 只打计费 / 余额接口、不进消耗记录、不进本机 token KPI、凭证不进备份。

pub mod litellm_proxy;
pub mod openai_compatible;
pub mod panel;
pub mod store;

use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::domain::OfficialQuotaWindow;

pub use store::{CustomQuotaConfig, CustomQuotaCredentials, CustomQuotaProvider, ResolvedProvider};

/// 标识前缀。两个职责：与内置 9 家永不冲突、界面上一眼分辨自定义与内置。
/// 托盘「最紧一档」按窗口有无重置时间分流，见 `tightest_window`。
pub const ID_PREFIX: &str = "custom:";
const TIMEOUT: Duration = Duration::from_secs(15);
pub const MISSING_SECRET: &str = "未配置密钥，请在设置页重新填写";
/// 「暂未支持」错误的识别标记，`is_precheck_error` 靠它认。
const UNSUPPORTED_MARK: &str = "暂未支持";

pub fn is_custom_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

/// 预设类型。本版实现「OpenAI 兼容计费」及其别名「NewAPI / OneAPI」，
/// 以及「LiteLLM Proxy」；其余走 `unsupported`。
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
    #[serde(rename = "litellm_proxy")]
    LiteLlmProxy,
}

impl CustomQuotaPreset {
    pub const ALL: [Self; 7] = [
        Self::OpenAiCompatible,
        Self::NewApi,
        Self::OpenRouter,
        Self::DeepSeek,
        Self::SiliconFlow,
        Self::Moonshot,
        Self::LiteLlmProxy,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai_compatible",
            Self::NewApi => "newapi",
            Self::OpenRouter => "openrouter",
            Self::DeepSeek => "deepseek",
            Self::SiliconFlow => "siliconflow",
            Self::Moonshot => "moonshot",
            Self::LiteLlmProxy => "litellm_proxy",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "OpenAI 兼容计费",
            Self::NewApi => "NewAPI / OneAPI",
            Self::OpenRouter => "OpenRouter",
            Self::DeepSeek => "DeepSeek",
            Self::SiliconFlow => "硅基流动",
            Self::Moonshot => "Moonshot",
            Self::LiteLlmProxy => "LiteLLM Proxy",
        }
    }

    /// 本版是否已有解析器。界面据此把没实现的那几档标灰。
    ///
    /// NewAPI / OneAPI 是 OpenAI 兼容计费的别名：站点自身实现了同一套
    /// `/v1/dashboard/billing/*`，不另开解析器。LiteLLM Proxy 打 `/key/info`。
    pub fn implemented(self) -> bool {
        matches!(
            self,
            Self::OpenAiCompatible | Self::NewApi | Self::LiteLlmProxy
        )
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.as_str() == value)
    }
}

fn unsupported(preset: CustomQuotaPreset) -> String {
    let names: Vec<&str> = CustomQuotaPreset::ALL
        .into_iter()
        .filter(|item| item.implemented())
        .map(CustomQuotaPreset::display_name)
        .collect();
    let listed = quote_display_names(&names);
    if listed.is_empty() {
        format!("「{}」{UNSUPPORTED_MARK}", preset.display_name())
    } else {
        format!(
            "「{}」{UNSUPPORTED_MARK}，当前只实现了{listed}",
            preset.display_name()
        )
    }
}

/// 把显示名列成「甲」、「乙」。空列表与一档都不能冒出多余顿号或空书名号。
pub(crate) fn quote_display_names(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("「{name}」"))
        .collect::<Vec<_>>()
        .join("、")
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

/// 一个要打的地址，以及它拿不到时算不算致命。
///
/// 分这个级别是因为「上限」和「已用」不是一回事：只实现了用量接口的中转站
/// 照样该显示金额，不该因为上限接口 404 就整行取不到数。
///
/// 这个形状同时是设置页那行回显的载体：界面显示的就是这里的 `url`，
/// 因此回显与取数不可能漂移。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaRequest {
    pub url: String,
    /// 为 false 时，这一条请求失败按「没有这个口径」处理，而不是让整次取数失败。
    pub required: bool,
}

/// 取数时**真正会请求**的地址，按预设类型分派。未实现的类型在这里就拦下来。
pub fn request_urls(
    preset: CustomQuotaPreset,
    base_url: &str,
    today: chrono::NaiveDate,
) -> Result<Vec<QuotaRequest>, String> {
    let base = normalize_base_url(base_url)?;
    match preset {
        CustomQuotaPreset::OpenAiCompatible | CustomQuotaPreset::NewApi => {
            Ok(openai_compatible::urls(&base, today))
        }
        CustomQuotaPreset::LiteLlmProxy => Ok(litellm_proxy::urls(&base)),
        other => Err(unsupported(other)),
    }
}

/// 原始响应体 → 额度窗口。**按预设类型分派、只有一个入口**：后续补齐未实现的
/// 预设时接缝数不增长，新解析器直接复用同一个测试入口。
///
/// `bodies` 与 `request_urls` 的返回一一对应。
pub fn parse_quota(
    preset: CustomQuotaPreset,
    bodies: &[&str],
) -> Result<Vec<OfficialQuotaWindow>, String> {
    match preset {
        CustomQuotaPreset::OpenAiCompatible | CustomQuotaPreset::NewApi => {
            openai_compatible::parse(bodies)
        }
        CustomQuotaPreset::LiteLlmProxy => litellm_proxy::parse(bodies),
        other => Err(unsupported(other)),
    }
}

/// 取数只需要三样：预设类型、地址、密钥。标识、名称、开关都不参与——
/// 因此设置页「测试连接」能拿一份还没保存、还没有标识的草稿直接打，
/// 不必先捏一条假的提供商出来。
pub fn fetch_quota(
    preset: CustomQuotaPreset,
    base_url: &str,
    secret: Option<&str>,
) -> super::ProviderFetch {
    let secret = ready(preset, secret)?;
    let requests = request_urls(preset, base_url, Utc::now().date_naive())?;
    // 可选接口拿不到就当没有这个口径：只实现了用量接口的中转站仍然显示金额。
    // 必需的那条失败才让整次取数失败，错误照旧是人话。
    let bodies = requests
        .iter()
        .map(|entry| match request(&entry.url, secret) {
            Ok(body) => Ok(body),
            Err(error) if entry.required => Err(error),
            Err(_) => Ok(String::new()),
        })
        .collect::<Result<Vec<String>, String>>()?;
    let borrowed: Vec<&str> = bodies.iter().map(String::as_str).collect();
    let windows = parse_quota(preset, &borrowed)?;
    Ok(super::QuotaSnapshot::new(windows, Utc::now().to_rfc3339()))
}

pub fn fetch(provider: &ResolvedProvider) -> super::ProviderFetch {
    fetch_quota(
        provider.config.preset,
        &provider.config.base_url,
        provider.secret.as_deref(),
    )
}

/// 不用打网就能判定的失败，顺带交出可用的密钥。
///
/// 单独拎出来是给退避看的——退避存在的理由是「别把对方打挂」，而这两种
/// 压根没碰到对方。记进退避的话，恢复备份后刚填完密钥、或刚存下一个未实现的
/// 预设，再点刷新只会看到「刚取数失败，N 分钟后自动重试」，把真正的原因盖掉。
fn ready(preset: CustomQuotaPreset, secret: Option<&str>) -> Result<&str, String> {
    let secret = secret.ok_or_else(|| MISSING_SECRET.to_string())?;
    if !preset.implemented() {
        return Err(unsupported(preset));
    }
    Ok(secret)
}

/// `ready` 的判定结果，只要那句话。取数入口自己走 `ready`，因此两边不会各判一次。
pub fn precheck(provider: &ResolvedProvider) -> Option<String> {
    ready(provider.config.preset, provider.secret.as_deref()).err()
}

/// 这条错误是不是「压根没打网」。`backoff::is_rate_limited` 也是按标记认的，
/// 沿用同一套办法：错误目前就是纯字符串。
pub fn is_precheck_error(error: &str) -> bool {
    error == MISSING_SECRET || error.contains(UNSUPPORTED_MARK)
}

/// 错误一律翻成人话：用户要判断的是「去充值 / 换密钥 / 等网络」，
/// 一个裸的 HTTP 码回答不了这个问题。
fn request(url: &str, secret: &str) -> Result<String, String> {
    // 认证走 Bearer。NewAPI / OneAPI 是别名，LiteLLM Proxy 也是 Bearer，不另开一套头。
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
