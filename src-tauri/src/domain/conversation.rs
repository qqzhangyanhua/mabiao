use serde::{Deserialize, Serialize};

use super::cursor::CursorSessionDetailDto;
use super::usage::UsageRecord;

/// 与 Topbar 共用时间 / 模型 / provider / 项目；来源仍与用量筛选隔离。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConversationQuery {
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub page_size: Option<u32>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub providers: Vec<String>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub tool_failed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMatchField {
    Title,
    Body,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConversationSessionRow {
    pub source: String,
    pub session_id: String,
    pub title: String,
    pub project: String,
    pub model: String,
    pub started_at: String,
    pub ended_at: String,
    pub source_file: String,
    pub source_files: Vec<String>,
    pub capabilities: Vec<String>,
    pub support_status: String,
    pub file_available: bool,
    /// 用量侧按 `(source, session_id)` 聚合；无消耗记录时为 0。
    #[serde(default)]
    pub total_tokens: i64,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub unpriced: bool,
    /// 事件索引已发布当前适配器版本的代次。目录搜索在 false 时只保证标题可搜。
    #[serde(default)]
    pub event_index_ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_field: Option<ConversationMatchField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_snippet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_sequence: Option<u32>,
    /// 由工作纪要引擎写回的自造会话。只作展示与后续纪要输入剔除，不从 token KPI 扣除。
    #[serde(default)]
    pub generated_by_work_notes: bool,
    /// 工作纪要 map 阶段为该会话写下的一句话摘要。只挂当前指纹仍匹配的缓存。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub work_notes_summaries: Vec<WorkNotesSessionSummary>,
}

/// 某次纪要引擎给单会话写出的摘要。同一会话可有多引擎/多模型各一条。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNotesSessionSummary {
    pub engine: String,
    pub model: String,
    pub summary: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationPage {
    pub rows: Vec<ConversationSessionRow>,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationUsagePage {
    pub rows: Vec<UsageRecord>,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub occurred_at: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationEventKind {
    Message,
    Plan,
    ToolCall,
    ToolResult,
    ModelChange,
    Error,
    SystemStatus,
    Unadapted,
}

impl ConversationEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Plan => "plan",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::ModelChange => "model_change",
            Self::Error => "error",
            Self::SystemStatus => "system_status",
            Self::Unadapted => "unadapted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationEventActor {
    User,
    Assistant,
    Tool,
}

impl ConversationEventActor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationEventCapabilityStatus {
    Complete,
    MissingTimestamp,
    Unadapted,
    UnadaptedMissingTimestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationEventContentStatus {
    Complete,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationAttachmentKind {
    Image,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationAttachmentStatus {
    Available,
    Missing,
    Embedded,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationAttachment {
    pub id: String,
    pub kind: ConversationAttachmentKind,
    pub name: String,
    pub original_path: String,
    pub media_type: String,
    pub size_bytes: Option<u64>,
    pub status: ConversationAttachmentStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationEvent {
    pub event_id: String,
    pub sequence: u32,
    pub source_file: String,
    pub source_sequence: u32,
    pub kind: ConversationEventKind,
    pub occurred_at: Option<String>,
    pub actor: Option<ConversationEventActor>,
    pub name: Option<String>,
    pub text: Option<String>,
    pub details: serde_json::Value,
    pub attachments: Vec<ConversationAttachment>,
    pub capability_status: ConversationEventCapabilityStatus,
    pub content_status: ConversationEventContentStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationEventContentDto {
    pub event_id: String,
    pub text: Option<String>,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationAttachmentContentDto {
    pub attachment: ConversationAttachment,
    pub data_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationExportFormat {
    Markdown,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationExportDto {
    pub default_name: String,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationAgentLinkStatus {
    Linked,
    MissingSource,
    Unresolved,
    Conflict,
    Cycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationAgentCapabilityStatus {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationAgentLink {
    pub relationship_id: String,
    pub session_id: Option<String>,
    pub launch_event_id: Option<String>,
    pub status: ConversationAgentLinkStatus,
    pub session: Option<ConversationSessionRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationAgentRelations {
    pub capability_status: ConversationAgentCapabilityStatus,
    pub parent: Option<ConversationAgentLink>,
    pub children: Vec<ConversationAgentLink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationEventAnchor {
    First,
    Last,
    Before {
        sequence: u32,
    },
    After {
        sequence: u32,
    },
    /// 从该序号起向后取一页，用于正文搜索命中后落到第一条匹配事件。
    Around {
        sequence: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationEventPage {
    pub events: Vec<ConversationEvent>,
    pub has_more_before: bool,
    pub has_more_after: bool,
}

/// 整份解析结果，供测试与分页回退对照。不是 Tauri 详情 DTO。
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationParsedDetail {
    pub revision: String,
    pub session: ConversationSessionRow,
    pub events: Vec<ConversationEvent>,
    pub agent_relations: ConversationAgentRelations,
    pub cursor_behavior: Option<CursorSessionDetailDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationDetailDto {
    pub revision: String,
    pub session: ConversationSessionRow,
    pub event_count: u32,
    pub usage_record_count: u32,
    pub agent_relations: ConversationAgentRelations,
    /// Cursor 本机行为聚合；非 Cursor 或对不上 `cursor_sessions` 时为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_behavior: Option<CursorSessionDetailDto>,
    /// 会话上下文清单。Cursor / Grok 共用同一形状，其它来源为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_manifest: Option<ConversationContextManifest>,
}

/// 证据层级。`injected` 层可表述为已注入；`on_disk_possible` 不得。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationContextLayer {
    Injected,
    Observed,
    OnDiskPossible,
}

impl ConversationContextLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Injected => "injected",
            Self::Observed => "observed",
            Self::OnDiskPossible => "on_disk_possible",
        }
    }
}

/// 条目何时进入上下文。取代抽象置信度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationContextLoadMode {
    Always,
    OnMatch,
    OnDemand,
    Manual,
    Observed,
}

impl ConversationContextLoadMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::OnMatch => "on_match",
            Self::OnDemand => "on_demand",
            Self::Manual => "manual",
            Self::Observed => "observed",
        }
    }
}

/// MCP 连接结论。仅 MCP 条目使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationContextInjectionStatus {
    Connected,
    Failed,
    AuthRequired,
    Disabled,
}

impl ConversationContextInjectionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Failed => "failed",
            Self::AuthRequired => "auth_required",
            Self::Disabled => "disabled",
        }
    }
}

/// 清单条目种类。Cursor / Grok 共用，避免下一票另造一套。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationContextKind {
    Tool,
    SystemStatus,
    Error,
    Skill,
    Instruction,
    Rule,
    McpServer,
}

impl ConversationContextKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::SystemStatus => "system_status",
            Self::Error => "error",
            Self::Skill => "skill",
            Self::Instruction => "instruction",
            Self::Rule => "rule",
            Self::McpServer => "mcp_server",
        }
    }
}

/// 一条上下文痕迹。`id` 是稳定键；磁盘项另带 `path`。
/// 体积以 `char_count` 为权威值；token 只在展示层换算。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationContextItem {
    pub layer: ConversationContextLayer,
    pub kind: ConversationContextKind,
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_mode: Option<ConversationContextLoadMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub injection_status: Option<ConversationContextInjectionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_count: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_noise: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_unused_install: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

/// 三层证据的扁平清单，供对话详情复用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationContextManifest {
    pub items: Vec<ConversationContextItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub injected_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_disk_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_init_summary: Option<String>,
    /// Cursor 无注入快照时为 false，界面不得装成已注入。
    #[serde(default = "default_true")]
    pub has_injected_snapshot: bool,
    /// 源快照已不在，条目来自摄取时写入的度量缓存。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub metrics_from_cache: bool,
    /// Cursor 本机拿不到逐轮实测 token，体积只是估算。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub volume_is_estimate: bool,
    /// `requestContextCompleteness` 含 false 时点名；全 true 或缺失则不下发。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completeness_note: Option<String>,
    /// skill / MCP 工具 / 子代理第一次被调用的那一轮。不进事件表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub first_uses: Vec<ConversationContextFirstUse>,
}

/// 消息流里的单行「本轮引入 X」。`item_*` 指向顶部总账对应条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationContextFirstUse {
    pub event_id: String,
    pub sequence: u32,
    pub item_id: String,
    pub item_kind: ConversationContextKind,
    pub item_layer: ConversationContextLayer,
    pub label: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationDetailStateDto {
    pub revision: String,
    pub changed: bool,
    pub file_available: bool,
}

/// 对话事件索引补建进度：已就绪会话数 / 应索引会话数。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationIndexProgressDto {
    pub indexed: u32,
    pub total: u32,
    /// 事件表 + FTS 占用；dbstat 不可用时回落为正文字节数，补建中可能为 0。
    #[serde(default)]
    pub index_bytes: u64,
}
