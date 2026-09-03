use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionLoadStatus {
    Loaded,
    PresentUnloaded,
    LocallyInvisible,
    NotCreated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionEvidence {
    Verified,
    Inferred,
    NoMechanism,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionEntryKind {
    File,
    Directory,
}

/// 各 Source 的消耗摘要，扫描接缝预留给用量交叉洞察，本维度不写入 sqlite。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InstructionUsageSummary {
    pub sources: Vec<InstructionSourceUsage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstructionSourceUsage {
    pub source: String,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalInstructionFile {
    pub kind: InstructionEntryKind,
    pub display_path: String,
    pub abs_path: String,
    pub byte_size: u64,
    pub modified_at: Option<String>,
    pub load_status: InstructionLoadStatus,
    pub evidence: InstructionEvidence,
    pub content: String,
    pub error: Option<String>,
    pub note: Option<String>,
    pub action: Option<String>,
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalInstructionSourceRow {
    pub source: String,
    pub application: String,
    pub files: Vec<GlobalInstructionFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionCheckupKind {
    Empty,
    PresentUnloaded,
    OverrideShields,
    NearLimit,
    OverLimit,
    OrphanMemories,
    AutoMemory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionCheckupSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstructionCheckupFinding {
    pub kind: InstructionCheckupKind,
    pub severity: InstructionCheckupSeverity,
    pub source: String,
    pub application: String,
    pub display_path: String,
    pub problem: String,
    pub consequence: String,
}

/// 关键词共现提示：两侧原文片段交给用户判断，不表示已确认冲突。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstructionOverlapHint {
    pub keyword: String,
    pub global_application: String,
    pub global_display_path: String,
    pub global_snippet: String,
    pub project_display_path: String,
    pub project_snippet: String,
}

/// 某个 Source 的指令投入与本机用量对照，mtime 只作事实展示，不作健康指标。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstructionInvestment {
    pub source: String,
    pub application: String,
    pub loaded_bytes: u64,
    pub modified_at: Option<String>,
    pub total_tokens: i64,
}

/// 用量占比高而已加载指令明显偏少。不是过期告警。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstructionImbalance {
    pub source: String,
    pub application: String,
    pub note: String,
}

/// Claude 按仓库隔离的自动记忆，不进全局指令 sources。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaudeAutoMemoryFile {
    pub name: String,
    pub abs_path: String,
    pub byte_size: u64,
    pub modified_at: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaudeAutoMemoryRepo {
    pub repo: String,
    pub display_path: String,
    pub abs_path: String,
    pub byte_size: u64,
    pub modified_at: Option<String>,
    pub files: Vec<ClaudeAutoMemoryFile>,
}

/// 全局指令快照：不进消耗记录，不进 Token KPI，不写 sqlite。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalInstructionDto {
    pub sources: Vec<GlobalInstructionSourceRow>,
    pub findings: Vec<InstructionCheckupFinding>,
    pub selected_project: Option<String>,
    pub projects: Vec<String>,
    pub hints: Vec<InstructionOverlapHint>,
    pub investments: Vec<InstructionInvestment>,
    pub imbalances: Vec<InstructionImbalance>,
    /// 旁路只读，不进 sources/files。机器记忆不是手写全局指令。
    pub claude_memories: Vec<ClaudeAutoMemoryRepo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WriteUserFileRequest {
    pub abs_path: String,
    pub content: String,
    pub expected_mtime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WriteUserFileResult {
    pub modified_at: Option<String>,
    pub byte_size: u64,
}
