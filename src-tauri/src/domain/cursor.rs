use super::stats::{NamedAmount, SeriesPoint};
use serde::{Deserialize, Serialize};

/// 独立于 `UsageRecord`，不含 session_id / source_file。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorUsageEvent {
    pub occurred_at: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub is_headless: bool,
}

impl CursorUsageEvent {
    pub fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_creation_tokens
    }

    pub fn fingerprint(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.occurred_at,
            self.model,
            self.input_tokens,
            self.output_tokens,
            self.cache_read_tokens,
            self.cache_creation_tokens,
            self.is_headless
        )
    }
}

/// Cursor 账号用量聚合：独立维度，不并入本机 token 总量。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorAccountUsageDto {
    pub as_of: Option<String>,
    pub event_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
    pub daily: Vec<SeriesPoint>,
    pub by_model: Vec<NamedAmount>,
    pub headless_tokens: i64,
    pub interactive_tokens: i64,
    pub headless_share: Option<f64>,
}

impl CursorAccountUsageDto {
    pub fn empty() -> Self {
        Self {
            as_of: None,
            event_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_tokens: 0,
            daily: Vec::new(),
            by_model: Vec::new(),
            headless_tokens: 0,
            interactive_tokens: 0,
            headless_share: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeVolumeCommit {
    pub commit_hash: String,
    pub branch: String,
    pub scored_at: String,
    pub commit_message: String,
    pub lines_added: i64,
    pub lines_deleted: i64,
    pub composer_lines_added: i64,
    pub composer_lines_deleted: i64,
    pub human_lines_added: i64,
    pub human_lines_deleted: i64,
    pub tab_lines_added: i64,
    pub tab_lines_deleted: i64,
    pub ai_percentage: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeVolumeDailyPoint {
    pub bucket: String,
    pub lines_added: i64,
    pub lines_deleted: i64,
    pub composer_lines_added: i64,
    pub tab_lines_added: i64,
    pub human_lines_added: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeVolumeBranchRow {
    pub name: String,
    pub commit_count: i64,
    pub lines_added: i64,
    pub composer_lines_added: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeVolumeSummary {
    pub commit_count: i64,
    pub lines_added: i64,
    pub lines_deleted: i64,
    pub net_lines: i64,
    pub composer_lines_added: i64,
    pub composer_lines_deleted: i64,
    pub human_lines_added: i64,
    pub human_lines_deleted: i64,
    pub tab_lines_added: i64,
    pub tab_lines_deleted: i64,
    pub ai_percentage: Option<f64>,
    /// 全部时间、全部来源的消耗记录费用估算；与代码量一样按「至今累计」口径，不受总览筛选影响。
    /// 只用于下面的粗略 ROI 交叉指标，不代表 Cursor 单一来源的花费。
    pub total_cost: Option<f64>,
    pub cost_unpriced: bool,
    /// = total_cost ÷ (composer_lines_added / 1000)。跨来源粗略口径：分子是全部 AI CLI 的费用，
    /// 分母只是 Cursor 记录到的 AI 生成行数，两者不是同一统计边界，仅供参考，不做精确归因。
    pub cost_per_thousand_ai_lines: Option<f64>,
    pub daily: Vec<CodeVolumeDailyPoint>,
    pub by_branch: Vec<CodeVolumeBranchRow>,
    pub commits: Vec<CodeVolumeCommit>,
}

impl CodeVolumeSummary {
    pub fn empty() -> Self {
        Self {
            commit_count: 0,
            lines_added: 0,
            lines_deleted: 0,
            net_lines: 0,
            composer_lines_added: 0,
            composer_lines_deleted: 0,
            human_lines_added: 0,
            human_lines_deleted: 0,
            tab_lines_added: 0,
            tab_lines_deleted: 0,
            ai_percentage: None,
            total_cost: None,
            cost_unpriced: false,
            cost_per_thousand_ai_lines: None,
            daily: Vec::new(),
            by_branch: Vec::new(),
            commits: Vec::new(),
        }
    }
}

/// 单条 Cursor 会话聚合（本机 agent-transcripts，不含正文）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorSessionRecord {
    pub session_id: String,
    pub project: String,
    pub turn_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub aborted_count: i64,
    pub user_prompt_count: i64,
    pub subagent_count: i64,
    pub tool_calls_json: String,
    pub models_json: String,
    pub sources_json: String,
    pub extensions_json: String,
    pub first_seen_at: Option<String>,
    pub last_seen_at: Option<String>,
    pub files_touched: i64,
    pub source_file: String,
}

/// 单条 Cursor 会话的界面明细（已解析 models / 工具次数，不含正文）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorSessionListRow {
    pub session_id: String,
    pub project: String,
    pub turn_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub aborted_count: i64,
    pub user_prompt_count: i64,
    pub subagent_count: i64,
    pub models: Vec<String>,
    pub sources: Vec<String>,
    pub tool_call_count: i64,
    pub first_seen_at: Option<String>,
    pub last_seen_at: Option<String>,
    pub files_touched: i64,
    pub source_file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorSessionProjectRow {
    pub name: String,
    pub session_count: i64,
    pub turn_count: i64,
    pub error_count: i64,
    pub files_touched: i64,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorSessionDailyPoint {
    pub bucket: String,
    pub session_count: i64,
    pub turn_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorSessionModelRow {
    pub name: String,
    pub session_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorSessionToolRow {
    pub name: String,
    pub call_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorSessionSourceRow {
    pub name: String,
    pub session_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorSessionExtensionRow {
    pub name: String,
    pub file_count: i64,
}

/// Cursor 会话汇总：独立维度，不并入本机 token 总量。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorSessionSummaryDto {
    pub as_of: Option<String>,
    pub session_count: i64,
    pub turn_count: i64,
    pub aborted_count: i64,
    pub user_prompt_count: i64,
    pub subagent_count: i64,
    pub error_rate: Option<f64>,
    pub average_turns: Option<f64>,
    pub single_prompt_ratio: Option<f64>,
    pub average_tools_per_turn: Option<f64>,
    pub write_read_ratio: Option<f64>,
    pub active_project_count: i64,
    pub by_project: Vec<CursorSessionProjectRow>,
    pub by_model: Vec<CursorSessionModelRow>,
    pub by_source: Vec<CursorSessionSourceRow>,
    pub by_extension: Vec<CursorSessionExtensionRow>,
    pub top_tools: Vec<CursorSessionToolRow>,
    pub tool_groups: Vec<CursorSessionToolRow>,
    pub daily: Vec<CursorSessionDailyPoint>,
}

impl CursorSessionSummaryDto {
    pub fn empty() -> Self {
        Self {
            as_of: None,
            session_count: 0,
            turn_count: 0,
            aborted_count: 0,
            user_prompt_count: 0,
            subagent_count: 0,
            error_rate: None,
            average_turns: None,
            single_prompt_ratio: None,
            average_tools_per_turn: None,
            write_read_ratio: None,
            active_project_count: 0,
            by_project: Vec::new(),
            by_model: Vec::new(),
            by_source: Vec::new(),
            by_extension: Vec::new(),
            top_tools: Vec::new(),
            tool_groups: Vec::new(),
            daily: Vec::new(),
        }
    }
}

/// Cursor 会话列表明细：搜索/项目/排序/分页均下沉到 SQL。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorSessionQuery {
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub sort_by: Option<String>,
    #[serde(default)]
    pub sort_dir: Option<String>,
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorSessionPage {
    pub rows: Vec<CursorSessionListRow>,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorSessionHashFile {
    pub path: String,
    pub extension: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorSessionDetailDto {
    pub session: CursorSessionListRow,
    pub tools: Vec<CursorSessionToolRow>,
    pub hash_files: Vec<CursorSessionHashFile>,
    pub read_paths: Vec<String>,
    pub write_paths: Vec<String>,
    pub transcript_missing: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorAccountEventQuery {
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub page_size: Option<u32>,
    #[serde(default)]
    pub sort_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorAccountEventRow {
    pub occurred_at: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
    pub is_headless: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorAccountEventPage {
    pub rows: Vec<CursorAccountEventRow>,
    pub total: u32,
}
