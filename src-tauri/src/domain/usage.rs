use serde::{Deserialize, Serialize};

use super::brand::Brand;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Codex,
    Claude,
    Pi,
    Omp,
    Opencode,
    Kimi,
    Dsh,
    Gemini,
    Grok,
    Qwen,
    Factory,
    CursorAgent,
    Copilot,
    Hermes,
    Agy,
    Qoder,
    QoderCn,
    Cline,
    WorkBuddy,
    Zcode,
    Alma,
    CodeBuddy,
    Trae,
    Goose,
    Roo,
    Kilo,
}

impl Source {
    pub const ALL: [Source; 26] = [
        Source::Codex,
        Source::Claude,
        Source::Pi,
        Source::Omp,
        Source::Opencode,
        Source::Kimi,
        Source::Dsh,
        Source::Gemini,
        Source::Grok,
        Source::Qwen,
        Source::Factory,
        Source::CursorAgent,
        Source::Copilot,
        Source::Hermes,
        Source::Agy,
        Source::Qoder,
        Source::QoderCn,
        Source::Cline,
        Source::WorkBuddy,
        Source::Zcode,
        Source::Alma,
        Source::CodeBuddy,
        Source::Trae,
        Source::Goose,
        Source::Roo,
        Source::Kilo,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Source::Codex => "codex",
            Source::Claude => "claude",
            Source::Pi => "pi",
            Source::Omp => "omp",
            Source::Opencode => "opencode",
            Source::Kimi => "kimi",
            Source::Dsh => "dsh",
            Source::Gemini => "gemini",
            Source::Grok => "grok",
            Source::Qwen => "qwen",
            Source::Factory => "factory",
            Source::CursorAgent => "cursor_agent",
            Source::Copilot => "copilot",
            Source::Hermes => "hermes",
            Source::Agy => "agy",
            Source::Qoder => "qoder",
            Source::QoderCn => "qoder_cn",
            Source::Cline => "cline",
            Source::WorkBuddy => "workbuddy",
            Source::Zcode => "zcode",
            Source::Alma => "alma",
            Source::CodeBuddy => "codebuddy",
            Source::Trae => "trae",
            Source::Goose => "goose",
            Source::Roo => "roo",
            Source::Kilo => "kilo",
        }
    }

    pub fn application_name(self) -> &'static str {
        match self {
            Source::Codex => "Codex",
            Source::Claude => "Claude Code",
            Source::Pi => "Pi",
            Source::Omp => "OMP",
            Source::Opencode => "OpenCode",
            Source::Kimi => "Kimi CLI",
            Source::Dsh => "DeepSeek Harness",
            Source::Gemini => "Gemini CLI",
            Source::Grok => "Grok CLI",
            Source::Qwen => "Qwen Code",
            Source::Factory => "Droid",
            Source::CursorAgent => "Cursor Agent",
            Source::Copilot => "GitHub Copilot CLI",
            Source::Hermes => "Hermes",
            Source::Agy => "Antigravity",
            Source::Qoder => "Qoder",
            Source::QoderCn => "Qoder CN",
            Source::Cline => "Cline",
            Source::WorkBuddy => "WorkBuddy",
            Source::Zcode => "ZCode",
            Source::Alma => "Alma",
            Source::CodeBuddy => "CodeBuddy",
            Source::Trae => "Trae CLI",
            Source::Goose => "Goose",
            Source::Roo => "Roo Code",
            Source::Kilo => "Kilo Code",
        }
    }

    /// 这个来源所属的品牌。`qoder_cn` 属于 Qoder，`cursor_agent` 属于 Cursor。
    pub fn brand(self) -> Brand {
        match self {
            Source::Codex => Brand::Codex,
            Source::Claude => Brand::Claude,
            Source::Pi => Brand::Pi,
            Source::Omp => Brand::Omp,
            Source::Opencode => Brand::Opencode,
            Source::Kimi => Brand::Kimi,
            Source::Dsh => Brand::Dsh,
            Source::Gemini => Brand::Gemini,
            Source::Grok => Brand::Grok,
            Source::Qwen => Brand::Qwen,
            Source::Factory => Brand::Factory,
            Source::CursorAgent => Brand::Cursor,
            Source::Copilot => Brand::Copilot,
            Source::Hermes => Brand::Hermes,
            Source::Agy => Brand::Agy,
            Source::Qoder => Brand::Qoder,
            Source::QoderCn => Brand::Qoder,
            Source::Cline => Brand::Cline,
            Source::WorkBuddy => Brand::WorkBuddy,
            Source::Zcode => Brand::Zcode,
            Source::Alma => Brand::Alma,
            Source::CodeBuddy => Brand::CodeBuddy,
            Source::Trae => Brand::Trae,
            Source::Goose => Brand::Goose,
            Source::Roo => Brand::Roo,
            Source::Kilo => Brand::Kilo,
        }
    }

    /// 这个来源的 CLI 恢复命令模板，`{id}` 占位会话 ID；没有公开恢复命令时为 `None`。
    pub fn resume_template(self) -> Option<&'static str> {
        match self {
            Source::Codex => Some("codex resume {id}"),
            Source::Claude => Some("claude --resume {id}"),
            Source::Pi => Some("pi --session {id}"),
            Source::Omp => Some("omp --session {id}"),
            Source::Opencode => Some("opencode --session {id}"),
            Source::Kimi => Some("kimi --session {id}"),
            Source::Dsh => None,
            Source::Gemini => Some("gemini --resume {id}"),
            Source::Grok => Some("grok --resume {id}"),
            Source::Qwen => Some("qwen --resume {id}"),
            Source::Factory => Some("droid --resume {id}"),
            Source::CursorAgent => Some("cursor-agent --resume {id}"),
            Source::Copilot => Some("copilot --resume={id}"),
            Source::Hermes => None,
            Source::Agy => None,
            Source::Qoder => Some("qodercli --resume {id}"),
            Source::QoderCn => Some("qoderclicn --resume {id}"),
            Source::Cline => Some("cline --id {id}"),
            Source::WorkBuddy => None,
            Source::Zcode => None,
            Source::Alma => None,
            Source::CodeBuddy => Some("codebuddy --resume {id}"),
            Source::Trae => Some("traex resume {id}"),
            Source::Goose => Some("goose session --resume --session-id {id}"),
            Source::Roo => None,
            Source::Kilo => None,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "codex" => Some(Source::Codex),
            "claude" => Some(Source::Claude),
            "pi" => Some(Source::Pi),
            "omp" => Some(Source::Omp),
            "opencode" => Some(Source::Opencode),
            "kimi" => Some(Source::Kimi),
            "dsh" => Some(Source::Dsh),
            "gemini" => Some(Source::Gemini),
            "grok" => Some(Source::Grok),
            "qwen" => Some(Source::Qwen),
            "factory" => Some(Source::Factory),
            "cursor_agent" => Some(Source::CursorAgent),
            "copilot" => Some(Source::Copilot),
            "hermes" => Some(Source::Hermes),
            "agy" => Some(Source::Agy),
            "qoder" => Some(Source::Qoder),
            "qoder_cn" => Some(Source::QoderCn),
            "cline" => Some(Source::Cline),
            "workbuddy" => Some(Source::WorkBuddy),
            "zcode" => Some(Source::Zcode),
            "alma" => Some(Source::Alma),
            "codebuddy" => Some(Source::CodeBuddy),
            "trae" => Some(Source::Trae),
            "goose" => Some(Source::Goose),
            "roo" => Some(Source::Roo),
            "kilo" => Some(Source::Kilo),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageRecord {
    pub occurred_at: String,
    pub source: Source,
    pub model: String,
    pub provider: String,
    pub project: String,
    pub session_id: String,
    pub source_file: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    /// 1h 缓存创建，已含在 `cache_creation_tokens` 里（KPI 仍是 5m+1h）。
    #[serde(default)]
    pub cache_creation_1h_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
    pub native_cost: Option<f64>,
}

impl UsageRecord {
    pub fn with_total(mut self) -> Self {
        if self.total_tokens <= 0 {
            self.total_tokens = self.input_tokens
                + self.output_tokens
                + self.cache_read_tokens
                + self.cache_creation_tokens
                + self.reasoning_tokens;
        }
        self
    }
}
