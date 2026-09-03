use serde::{Deserialize, Serialize};

/// 用户配置的月度预算（美元），持久化在独立文件里，与单价表分开管理。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub monthly_usd: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfficialQuotaProvider {
    Claude,
    Codex,
    Cursor,
    Grok,
    Droid,
    Antigravity,
    OpenCode,
    Copilot,
    Devin,
}

impl OfficialQuotaProvider {
    pub const ALL: [Self; 9] = [
        Self::Claude,
        Self::Codex,
        Self::Cursor,
        Self::Grok,
        Self::Droid,
        Self::Antigravity,
        Self::OpenCode,
        Self::Copilot,
        Self::Devin,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Grok => "grok",
            Self::Droid => "droid",
            Self::Antigravity => "antigravity",
            Self::OpenCode => "opencode",
            Self::Copilot => "copilot",
            Self::Devin => "devin",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
            Self::Grok => "Grok",
            Self::Droid => "Droid",
            Self::Antigravity => "Antigravity",
            Self::OpenCode => "OpenCode",
            Self::Copilot => "Copilot",
            Self::Devin => "Devin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "cursor" => Some(Self::Cursor),
            "grok" => Some(Self::Grok),
            "droid" => Some(Self::Droid),
            "antigravity" => Some(Self::Antigravity),
            "opencode" => Some(Self::OpenCode),
            "copilot" => Some(Self::Copilot),
            "devin" => Some(Self::Devin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfficialQuotaFreshness {
    Official,
    Stale,
    Unavailable,
}

/// 按连续两次官方快照估计何时打满。不是官方给出的时间。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaExhaustKind {
    /// 按当前速度会在 `at` 打满。
    Hits,
    /// 有速率，但打满时刻晚于本窗重置。
    WillNotHit,
    /// 快照已经 ≥ 100%。
    Exhausted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaExhaustDto {
    pub kind: QuotaExhaustKind,
    /// 预计打满时刻（RFC3339）。`hits` 才有。
    pub at: Option<String>,
}

/// 「官方额度」里的一条窗口。
///
/// 约定：所有构造点以 `..Default::default()` 收尾，这样 #81 新增金额口径字段时
/// 改动收敛到这里，不必同时改散在各 provider 解析器里的二十多处字面量。
///
/// `non_exhaustive` 只是让 clippy 的 `needless_update` 接受「字段已列全仍带
/// `..Default::default()`」这种写法，并对外声明本结构体会长字段。它**不**强制
/// 上面那条约定——构造点全在本 crate 内，crate 内该属性无效力，约定靠 review 守。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OfficialQuotaWindow {
    pub kind: String,
    pub label: String,
    pub used_percent: Option<f64>,
    pub resets_at: Option<String>,
    /// 金额口径。充值制的自定义提供商给的是钱不是百分比，两种口径要能并存：
    /// 拿得到上限就同时有百分比和金额，拿不到上限就只有金额。
    ///
    /// 三个字段全是 `Option`，serde 对缺失的 `Option` 按 `None` 处理，
    /// 因此 sqlite 里旧格式的窗口 JSON 不需要迁移（有测试盯着这条）。
    #[serde(default)]
    pub used_amount: Option<f64>,
    #[serde(default)]
    pub limit_amount: Option<f64>,
    /// ISO 4217 代码，例如 `USD`。取不到时留空，界面只显示数字。
    #[serde(default)]
    pub currency: Option<String>,
    /// 按最近两次官方快照估计的撞线。读 DTO 时现算，不作为官方快照的一部分写入。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exhaust: Option<QuotaExhaustDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfficialQuotaRow {
    pub provider: String,
    pub application: String,
    pub windows: Vec<OfficialQuotaWindow>,
    pub freshness: OfficialQuotaFreshness,
    pub captured_at: Option<String>,
    pub error: Option<String>,
    /// 待办提示，不是取数失败。恢复备份后缺密钥走这里。
    #[serde(default)]
    pub todo: Option<String>,
    /// 账号套餐展示名。Cursor / Grok 能拿到；其余账号和自定义提供商留空。
    #[serde(default)]
    pub plan: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfficialQuotaConfig {
    #[serde(default = "default_alerts_enabled")]
    pub alerts_enabled: bool,
    /// 主窗口「配置显示」里关掉的账号（provider id）。托盘额度区块复用这份配置，
    /// 一处关掉两边都不再展示；不影响告警和本机采集。
    #[serde(default)]
    pub hidden_providers: Vec<String>,
}

fn default_alerts_enabled() -> bool {
    true
}

impl Default for OfficialQuotaConfig {
    fn default() -> Self {
        Self {
            alerts_enabled: true,
            hidden_providers: Vec::new(),
        }
    }
}

/// 账号级官方额度快照：不进消耗记录，不与本机 5 小时/7 天估计窗混条。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfficialQuotaDto {
    pub rows: Vec<OfficialQuotaRow>,
    pub alerts_enabled: bool,
    pub stale_after_minutes: i64,
    /// 本机没检测到登录态、因而没出现在 `rows` 里的账号（provider id）。
    /// 隐藏可以少一堆红字，但不能让用户不知道我们支持它。
    pub undetected: Vec<String>,
    /// 与 `OfficialQuotaConfig::hidden_providers` 原样对照，前端用它跟本地
    /// 「配置显示」状态对齐，不参与 `rows` 的过滤（各家状态仍要能在设置页看到）。
    #[serde(default)]
    pub hidden_providers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfficialQuotaHookDto {
    pub settings_path: String,
    pub command: String,
    pub snippet: String,
    pub already_configured: bool,
    pub conflict: bool,
    pub conflict_command: Option<String>,
}
