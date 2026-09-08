use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 工作纪要要取的区间。语义一律「至今」，与报告的已结束自然周期相反。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNotesRange {
    pub kind: WorkNotesRangeKind,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkNotesRangeKind {
    ThisWeek,
    ThisMonth,
    Custom,
}

impl WorkNotesRange {
    pub fn this_week() -> Self {
        Self {
            kind: WorkNotesRangeKind::ThisWeek,
            from: None,
            to: None,
        }
    }

    pub fn this_month() -> Self {
        Self {
            kind: WorkNotesRangeKind::ThisMonth,
            from: None,
            to: None,
        }
    }

    pub fn custom(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            kind: WorkNotesRangeKind::Custom,
            from: Some(from.into()),
            to: Some(to.into()),
        }
    }
}

/// 规模闸门判定。阈值只在 Rust 计算，webview 只呈现。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkNotesGate {
    Ok,
    Confirm,
    Rejected,
}

/// 选完区间立刻返回的规模预览，不调引擎。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkNotesPreviewDto {
    pub range_kind: WorkNotesRangeKind,
    pub start_date: String,
    pub end_date: String,
    pub session_count: i64,
    pub skipped_sparse: i64,
    pub gate: WorkNotesGate,
    pub message: String,
    pub estimated_calls: i64,
    pub estimated_secs: i64,
    pub estimated_input_tokens: i64,
    pub estimated_cost: Option<f64>,
    pub estimated_unpriced: bool,
    /// 该区间最近一次已生成的纪要。关掉 App 再打开时直接展示，不调引擎。
    #[serde(default)]
    pub cached: Option<WorkNotesDto>,
}

/// `preview` / `generate` 共用的请求参数。引擎、模型与补充指令计入缓存键。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNotesParams {
    pub range: WorkNotesRange,
    #[serde(default)]
    pub extra_instructions: String,
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub confirmed: bool,
}

impl WorkNotesParams {
    pub fn engine_id(&self) -> &str {
        let trimmed = self.engine.trim();
        if trimmed.is_empty() {
            "codex"
        } else {
            trimmed
        }
    }

    pub fn model_id(&self) -> &str {
        self.model.trim()
    }

    pub fn extra(&self) -> &str {
        self.extra_instructions.trim()
    }
}

/// `summarize_session` 的请求参数：哪条对话记录、用哪个引擎。引擎与模型同样计入缓存键。
/// 由 command 就地拼出来，不过 IPC，因而没有 `types.ts` 对应物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkNotesSessionParams {
    pub source: String,
    pub session_id: String,
    pub engine: String,
    pub model: String,
}

impl WorkNotesSessionParams {
    pub fn engine_id(&self) -> &str {
        let trimmed = self.engine.trim();
        if trimmed.is_empty() {
            "codex"
        } else {
            trimmed
        }
    }

    pub fn model_id(&self) -> &str {
        self.model.trim()
    }
}

/// 纪要引擎的静态描述。一个 CLI 一个 profile。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineProfile {
    pub id: String,
    pub program: String,
    pub writes_session_dir: bool,
    /// 编排读这个数，不在调用循环里写死。
    pub concurrency: u32,
    pub secs_per_call: u32,
    pub model: String,
    pub provider: String,
}

/// 设置页手动探测的结果。未安装的项 `installed = false`，不进生成选项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedEngine {
    pub id: String,
    pub program: String,
    pub writes_session_dir: bool,
    pub installed: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkNotesJobStatus {
    Idle,
    Running,
    Done,
    Cancelled,
    Error,
}

/// 后台任务进度。前端轮询，不另开 Tauri event。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkNotesProgressDto {
    pub status: WorkNotesJobStatus,
    pub done: u32,
    pub total: u32,
    pub current_title: String,
    pub error: String,
    pub result: Option<WorkNotesDto>,
}

impl Default for WorkNotesProgressDto {
    fn default() -> Self {
        Self {
            status: WorkNotesJobStatus::Idle,
            done: 0,
            total: 0,
            current_title: String::new(),
            error: String::new(),
            result: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNotesFailure {
    pub title: String,
    pub error: String,
}

/// 应用数据目录下纪要引擎的专用空目录名。会落盘的引擎把它当作项目路径，用来识别自造会话。
pub const WORK_NOTES_ENGINE_DIR: &str = "work-notes-engine";

/// 已经拼好、交给 runner 执行的一条 CLI 调用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineCommand {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: String,
    pub cwd: PathBuf,
    /// grok 钉死的新会话 UUID。其它引擎为 None。
    pub session_id: Option<String>,
}

/// 工作纪要入口 DTO。叙事来自对话正文；硬数字只复用消耗记录查询。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkNotesDto {
    pub range_kind: WorkNotesRangeKind,
    pub start_date: String,
    pub end_date: String,
    pub has_data: bool,
    pub skipped_sparse: i64,
    pub session_count: i64,
    pub project_count: i64,
    pub active_days: i64,
    pub total_tokens: i64,
    pub headline: String,
    pub entries: Vec<WorkNotesEntry>,
    pub closing: String,
    #[serde(default)]
    pub extra_instructions: String,
    pub failed_count: i64,
    pub failures: Vec<WorkNotesFailure>,
    pub actual_input_tokens: i64,
    pub actual_output_tokens: i64,
    pub actual_cost: Option<f64>,
    pub actual_unpriced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNotesEntry {
    pub title: String,
    pub detail: String,
    pub project: String,
}

/// 历史纪要列表的查询参数。`engine` 留空表示不筛引擎，看全部。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNotesHistoryQuery {
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub page_size: Option<u32>,
}

/// 历史列表每一行只带列表要展示的字段；entries/closing 等大字段留到点开详情时再取。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNotesHistoryRow {
    pub id: i64,
    pub created_at: String,
    pub range_kind: WorkNotesRangeKind,
    pub start_date: String,
    pub end_date: String,
    pub engine: String,
    pub model: String,
    pub extra_instructions: String,
    pub session_count: i64,
    pub headline: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNotesHistoryPage {
    pub rows: Vec<WorkNotesHistoryRow>,
    pub total: u32,
}
