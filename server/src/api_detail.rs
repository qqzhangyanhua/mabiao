//! 会话详情与项目页的请求 / 组合响应形状（管理网页用）。与 `server/web/src/api/types.ts` 一一对应。

use push_protocol::{ContextManifestPayload, EventPayload};
use serde::{Deserialize, Serialize};

use crate::project_admin::ProjectRow;
use crate::sessions::{SessionDetailRow, SessionListRow};
use crate::summary::{BreakdownRow, Summary};
use crate::usage_query::Totals;

/// 本场会话自己的消耗：统一费用与客户端快照并列。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionUsageView {
    pub totals: Totals,
    /// 按统一费用从高到低。
    pub by_model: Vec<BreakdownRow>,
}

impl SessionUsageView {
    pub fn from_rows(rows: Vec<BreakdownRow>) -> Self {
        Self {
            totals: Totals::sum_of(&rows),
            by_model: rows,
        }
    }
}

/// 单场会话全文。`context_manifest` 里每个条目带证据层级，网页据此区分
/// 「已注入原文 / 磁盘可能生效 / 来自缓存无原文」，不得把后两者说成已注入。
/// 刻意不带内部列 `device_pk`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionDetailView {
    pub session: SessionListRow,
    pub source_files: Vec<String>,
    /// 本场正文与注入原文被打码的处数。
    pub redaction_count: i32,
    pub events: Vec<EventPayload>,
    pub context_manifest: Option<ContextManifestPayload>,
    pub usage: SessionUsageView,
}

impl SessionDetailView {
    pub fn new(row: SessionDetailRow, usage: Vec<BreakdownRow>) -> Self {
        Self {
            session: row.item,
            source_files: row.source_files,
            redaction_count: row.redaction_count,
            events: row.events,
            context_manifest: row.context_manifest,
            usage: SessionUsageView::from_rows(usage),
        }
    }
}

/// 项目页：项目本身加这个项目上的消耗拆分（按成员、模型、来源、按天）。
/// 成员只看到自己那部分，管理员看到全体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectDetailResponse {
    pub project: ProjectRow,
    pub summary: Summary,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RenameProjectRequest {
    pub name: String,
}

/// 把路径里的项目并进 `into_project_id`。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MergeProjectRequest {
    pub into_project_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeProjectResponse {
    pub project: ProjectRow,
    pub sessions_moved: u64,
    pub usage_records_moved: u64,
}
