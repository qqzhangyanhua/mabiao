use serde::{Deserialize, Serialize};

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
}

impl Source {
    pub const ALL: [Source; 14] = [
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
