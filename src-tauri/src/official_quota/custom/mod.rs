//! 自定义提供商：用户在设置页自行登记的、按内置预设类型取数的账号级额度来源。
//!
//! 与内置 9 家是**平行通道**：`OfficialQuotaProvider` 枚举及其穷尽匹配一行不改，
//! 这里自成一路，两边各自产出额度行，在 `official_quota::load_dto` 处合流。
//! 合流点是整个应用取用额度数据的唯一出口，因此首页、托盘、告警全部零改动。
//!
//! 边界见 `docs/adr/0012-custom-quota-providers.md`：只允许内置预设类型、
//! 只打计费 / 余额接口、不进消耗记录、不进本机 token KPI、凭证不进备份。

pub mod command_code;
pub mod kimi_code;
pub mod litellm_proxy;
pub mod minimax_coding;
pub mod openai_compatible;
pub mod panel;
pub mod store;
pub mod volcengine_ark;
pub mod zhipu_coding;

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
/// 换了 host / origin 之后不得沿用已存密钥，否则密钥会被打到新地址。
pub const HOST_CHANGED_SECRET: &str = "更换了提供商地址，请重新填写密钥";
/// 「暂未支持」错误的识别标记，`is_precheck_error` 靠它认。
const UNSUPPORTED_MARK: &str = "暂未支持";
const REMOTE_HTTP_DENIED: &str =
    "非本机地址必须使用 https://，http:// 仅允许 localhost / 127.0.0.1 / ::1";

pub fn is_custom_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

/// 预设类型。本版实现「OpenAI 兼容计费」及其别名「NewAPI / OneAPI」、
/// 「LiteLLM Proxy」，四档 API-key 套餐，以及火山方舟（AccessKey ID +
/// Secret Access Key）；其余走 `unsupported`。
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
    #[serde(rename = "kimi_code")]
    KimiCode,
    #[serde(rename = "minimax_coding")]
    MiniMaxCoding,
    #[serde(rename = "zhipu_coding")]
    ZhipuCoding,
    #[serde(rename = "command_code")]
    CommandCode,
    #[serde(rename = "volcengine_ark")]
    VolcengineArk,
}

impl CustomQuotaPreset {
    pub const ALL: [Self; 12] = [
        Self::OpenAiCompatible,
        Self::NewApi,
        Self::OpenRouter,
        Self::DeepSeek,
        Self::SiliconFlow,
        Self::Moonshot,
        Self::LiteLlmProxy,
        Self::KimiCode,
        Self::MiniMaxCoding,
        Self::ZhipuCoding,
        Self::CommandCode,
        Self::VolcengineArk,
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
            Self::KimiCode => "kimi_code",
            Self::MiniMaxCoding => "minimax_coding",
            Self::ZhipuCoding => "zhipu_coding",
            Self::CommandCode => "command_code",
            Self::VolcengineArk => "volcengine_ark",
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
            Self::KimiCode => "Kimi Code",
            Self::MiniMaxCoding => "MiniMax Coding Plan",
            Self::ZhipuCoding => "GLM / Z.ai Coding Plan",
            Self::CommandCode => "Command Code",
            Self::VolcengineArk => "火山方舟",
        }
    }

    /// 本版是否已有解析器。界面据此把没实现的那几档标灰。
    ///
    /// NewAPI / OneAPI 是 OpenAI 兼容计费的别名：站点自身实现了同一套
    /// `/v1/dashboard/billing/*`，不另开解析器。LiteLLM Proxy 打 `/key/info`。
    pub fn implemented(self) -> bool {
        matches!(
            self,
            Self::OpenAiCompatible
                | Self::NewApi
                | Self::LiteLlmProxy
                | Self::KimiCode
                | Self::MiniMaxCoding
                | Self::ZhipuCoding
                | Self::CommandCode
                | Self::VolcengineArk
        )
    }

    /// 火山方舟要账号 AccessKey ID + Secret，其余预设只要一把密钥。
    pub fn needs_access_key_id(self) -> bool {
        matches!(self, Self::VolcengineArk)
    }

    /// 智谱 / Z.ai 的额度接口要裸密钥，其余预设走 Bearer。
    pub fn bearer_authorization(self) -> bool {
        !matches!(self, Self::ZhipuCoding)
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
    let authority = rest.split('/').next().unwrap_or("");
    let (host, _port) = parse_authority(authority)?;
    if scheme == "http://" && !is_loopback_host(&host) {
        return Err(REMOTE_HTTP_DENIED.to_string());
    }
    Ok(format!("{scheme}{rest}"))
}

fn parse_authority(authority: &str) -> Result<(String, Option<u16>), String> {
    if authority.is_empty() {
        return Err("base URL 只有协议头，缺少域名".to_string());
    }
    if authority.contains('@') {
        return Err("base URL 不能包含用户名或密码".to_string());
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest
            .split_once(']')
            .ok_or_else(|| "IPv6 地址缺少 ]".to_string())?;
        if host.is_empty() {
            return Err("IPv6 地址为空".to_string());
        }
        let port = match after {
            "" => None,
            s => match s.strip_prefix(':') {
                Some(port) => Some(parse_port(port)?),
                None => return Err("IPv6 地址格式不正确".to_string()),
            },
        };
        return Ok((host.to_ascii_lowercase(), port));
    }
    let colon_count = authority.bytes().filter(|byte| *byte == b':').count();
    if colon_count == 0 {
        return Ok((authority.to_ascii_lowercase(), None));
    }
    if colon_count == 1 {
        let Some((host, port)) = authority.split_once(':') else {
            return Err("base URL 的端口号不正确".to_string());
        };
        if host.is_empty() {
            return Err("base URL 只有协议头，缺少域名".to_string());
        }
        return Ok((host.to_ascii_lowercase(), Some(parse_port(port)?)));
    }
    Ok((authority.to_ascii_lowercase(), None))
}

fn parse_port(raw: &str) -> Result<u16, String> {
    raw.parse()
        .map_err(|_| "base URL 的端口号不正确".to_string())
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// 归一化后的 base 去掉路径，只留 scheme + host[:port]。
pub(crate) fn origin_of_normalized(base: &str) -> String {
    let scheme = if base.starts_with("https://") {
        "https://"
    } else if base.starts_with("http://") {
        "http://"
    } else {
        return base.to_string();
    };
    let rest = &base[scheme.len()..];
    let authority = rest.split('/').next().unwrap_or("");
    format!("{scheme}{authority}")
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "https://" => Some(443),
        "http://" => Some(80),
        _ => None,
    }
}

/// 去掉路径与默认端口后的 origin，用来判断「换地址」是不是换了主机。
pub fn origin_of(raw: &str) -> Result<String, String> {
    let normalized = normalize_base_url(raw)?;
    let scheme = ["https://", "http://"]
        .into_iter()
        .find(|item| normalized.starts_with(item))
        .ok_or_else(|| "base URL 需要以 http:// 或 https:// 开头".to_string())?;
    let rest = &normalized[scheme.len()..];
    let authority = rest.split('/').next().unwrap_or("");
    let (host, port) = parse_authority(authority)?;
    let port = port.filter(|value| default_port(scheme) != Some(*value));
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host
    };
    match port {
        Some(port) => Ok(format!("{scheme}{host}:{port}")),
        None => Ok(format!("{scheme}{host}")),
    }
}

/// 已存密钥只能打回同一个 origin。解析失败或换了主机都当作不能沿用。
pub fn can_reuse_stored_secret(saved_base_url: &str, request_base_url: &str) -> bool {
    match (origin_of(saved_base_url), origin_of(request_base_url)) {
        (Ok(saved), Ok(request)) => saved == request,
        _ => false,
    }
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
        CustomQuotaPreset::KimiCode => Ok(kimi_code::urls(&base)),
        CustomQuotaPreset::MiniMaxCoding => Ok(minimax_coding::urls(&base)),
        CustomQuotaPreset::ZhipuCoding => Ok(zhipu_coding::urls(&base)),
        CustomQuotaPreset::CommandCode => Ok(command_code::urls(&base)),
        CustomQuotaPreset::VolcengineArk => volcengine_ark::urls(&base),
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
        CustomQuotaPreset::KimiCode => kimi_code::parse(bodies),
        CustomQuotaPreset::MiniMaxCoding => minimax_coding::parse(bodies),
        CustomQuotaPreset::ZhipuCoding => zhipu_coding::parse(bodies),
        CustomQuotaPreset::CommandCode => command_code::parse(bodies),
        CustomQuotaPreset::VolcengineArk => volcengine_ark::parse(bodies),
        other => Err(unsupported(other)),
    }
}

/// 取数认预设类型、地址、密钥；火山方舟再加一把 AccessKey ID。
/// 标识、名称、开关都不参与——设置页「测试连接」能拿还没保存的草稿直接打。
pub fn fetch_quota(
    preset: CustomQuotaPreset,
    base_url: &str,
    secret: Option<&str>,
    access_key_id: Option<&str>,
) -> super::ProviderFetch {
    let secret = ready(preset, secret, access_key_id)?;
    if preset == CustomQuotaPreset::VolcengineArk {
        return volcengine_ark::fetch(base_url, access_key_id, Some(secret));
    }
    let requests = request_urls(preset, base_url, Utc::now().date_naive())?;
    // 可选接口拿不到就当没有这个口径：只实现了用量接口的中转站仍然显示金额。
    // 必需的那条失败才让整次取数失败，错误照旧是人话。
    let bodies = requests
        .iter()
        .map(|entry| match request(preset, &entry.url, secret) {
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
        provider.access_key_id.as_deref(),
    )
}

/// 不用打网就能判定的失败，顺带交出可用的密钥。
///
/// 单独拎出来是给退避看的——退避存在的理由是「别把对方打挂」，而这两种
/// 压根没碰到对方。记进退避的话，恢复备份后刚填完密钥、或刚存下一个未实现的
/// 预设，再点刷新只会看到「刚取数失败，N 分钟后自动重试」，把真正的原因盖掉。
fn ready<'a>(
    preset: CustomQuotaPreset,
    secret: Option<&'a str>,
    access_key_id: Option<&str>,
) -> Result<&'a str, String> {
    let secret = secret.ok_or_else(|| MISSING_SECRET.to_string())?;
    if !preset.implemented() {
        return Err(unsupported(preset));
    }
    if preset.needs_access_key_id()
        && access_key_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(volcengine_ark::MISSING_ACCESS_KEY.to_string());
    }
    Ok(secret)
}

/// `ready` 的判定结果，只要那句话。取数入口自己走 `ready`，因此两边不会各判一次。
pub fn precheck(provider: &ResolvedProvider) -> Option<String> {
    ready(
        provider.config.preset,
        provider.secret.as_deref(),
        provider.access_key_id.as_deref(),
    )
    .err()
}

/// 这条错误是不是「压根没打网」。`backoff::is_rate_limited` 也是按标记认的，
/// 沿用同一套办法：错误目前就是纯字符串。
pub fn is_precheck_error(error: &str) -> bool {
    error == MISSING_SECRET
        || error == volcengine_ark::MISSING_ACCESS_KEY
        || error.contains(UNSUPPORTED_MARK)
}

/// 错误一律翻成人话：用户要判断的是「去充值 / 换密钥 / 等网络」，
/// 一个裸的 HTTP 码回答不了这个问题。
fn request(preset: CustomQuotaPreset, url: &str, secret: &str) -> Result<String, String> {
    // 默认 Bearer。智谱 / Z.ai Coding Plan 要裸密钥，Authorization 不加 Bearer。
    let authorization = authorization_value(preset, secret);
    let request = crate::net::agent_with_timeout(TIMEOUT)
        .get(url)
        .set("Authorization", &authorization)
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

pub(crate) fn authorization_value(preset: CustomQuotaPreset, secret: &str) -> String {
    if preset.bearer_authorization() {
        format!("Bearer {secret}")
    } else {
        secret.to_string()
    }
}
