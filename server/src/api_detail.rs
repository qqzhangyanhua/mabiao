//! 会话详情与项目页的响应形状（管理网页用）。与 `server/web/src/api/types.ts` 一一对应。

use chrono::{DateTime, Utc};
use push_protocol::{ContextManifestPayload, EventPayload};
use serde::{Deserialize, Serialize};

use crate::api::{BreakdownView, SessionListItem, SummaryResponse, UsageTotalsView};
use crate::project_admin::{ProjectListRow, ProjectRow};
use crate::sessions::SessionDetailRow;
use crate::summary::BreakdownRow;
use crate::usage_query::Totals;

/// 本场会话自己的消耗：统一费用与客户端快照并列。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionUsageView {
    pub totals: UsageTotalsView,
    /// 按统一费用从高到低。
    pub by_model: Vec<BreakdownView>,
}

impl SessionUsageView {
    pub fn from_rows(rows: Vec<BreakdownRow>) -> Self {
        Self {
            totals: Totals::sum_of(&rows).into(),
            by_model: rows.into_iter().map(Into::into).collect(),
        }
    }
}

/// 单场会话全文。`context_manifest` 里每个条目带证据层级，网页据此区分
/// 「已注入原文 / 磁盘可能生效 / 来自缓存无原文」，不得把后两者说成已注入。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionDetailView {
    pub session: SessionListItem,
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
            session: row.item.into(),
            source_files: row.source_files,
            redaction_count: row.redaction_count,
            events: row.events,
            context_manifest: row.context_manifest,
            usage: SessionUsageView::from_rows(usage),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectView {
    pub id: i64,
    pub name: String,
    pub key: String,
    /// 归一后的 remote（`host/owner/repo`）；目录兜底的项目为空。
    pub git_remote: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<ProjectRow> for ProjectView {
    fn from(row: ProjectRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            key: row.key,
            git_remote: row.git_remote,
            created_at: row.created_at,
        }
    }
}

/// 项目页：项目本身加这个项目上的消耗拆分（按成员、模型、来源、按天）。
/// 成员只看到自己那部分，管理员看到全体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectDetailResponse {
    pub project: ProjectView,
    pub summary: SummaryResponse,
}

/// 管理员的项目清单行：附数据量，挑合并目标用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminProjectView {
    #[serde(flatten)]
    pub project: ProjectView,
    pub session_count: i64,
    pub usage_record_count: i64,
    /// 已经合并进它的项目个数。
    pub merged_count: i64,
}

impl From<ProjectListRow> for AdminProjectView {
    fn from(row: ProjectListRow) -> Self {
        Self {
            project: ProjectView {
                id: row.id,
                name: row.name,
                key: row.key,
                git_remote: row.git_remote,
                created_at: row.created_at,
            },
            session_count: row.session_count,
            usage_record_count: row.usage_record_count,
            merged_count: row.merged_count,
        }
    }
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
    pub project: ProjectView,
    pub sessions_moved: u64,
    pub usage_records_moved: u64,
}
