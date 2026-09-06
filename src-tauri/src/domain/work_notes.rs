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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNotesPreviewDto {
    pub range_kind: WorkNotesRangeKind,
    pub start_date: String,
    pub end_date: String,
    pub session_count: i64,
    pub skipped_sparse: i64,
    pub gate: WorkNotesGate,
    pub message: String,
}

/// 纪要引擎的静态描述。一个 CLI 一个 profile。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineProfile {
    pub id: String,
    pub program: String,
    pub writes_session_dir: bool,
}

/// 已经拼好、交给 runner 执行的一条 CLI 调用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineCommand {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: String,
    pub cwd: PathBuf,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNotesEntry {
    pub title: String,
    pub detail: String,
    pub project: String,
}
