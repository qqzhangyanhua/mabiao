//! 管理接口与账号接口的响应形状。只有服务端与它自带的网页用，不进 `push-protocol`。

use chrono::{DateTime, NaiveDate, Utc};
use push_protocol::{RemoteRole, UsageTokens};
use serde::{Deserialize, Serialize};

use crate::accounts::AccountRow;
use crate::coverage::MemberCoverage;
use crate::devices::DeviceRow;
use crate::sessions::SessionListRow;
use crate::summary::{BreakdownRow, Summary};
use crate::team_pricing::TeamPriceRow;
use crate::usage_query::{Totals, UsageRow};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountView {
    pub id: i64,
    pub account: String,
    pub role: RemoteRole,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub deactivated_at: Option<DateTime<Utc>>,
}

impl From<AccountRow> for AccountView {
    /// 不带 `password_hash`。
    fn from(row: AccountRow) -> Self {
        Self {
            role: row.role(),
            id: row.id,
            account: row.account,
            active: row.active,
            created_at: row.created_at,
            deactivated_at: row.deactivated_at,
        }
    }
}

/// 管理员只能建成员；管理员账号走服务端命令行。
#[derive(Clone, Deserialize)]
pub struct CreateMemberRequest {
    pub account: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceView {
    pub device_id: String,
    pub device_name: String,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

impl From<DeviceRow> for DeviceView {
    fn from(row: DeviceRow) -> Self {
        Self {
            device_id: row.device_id,
            device_name: row.device_name,
            first_seen_at: row.first_seen_at,
            last_seen_at: row.last_seen_at,
        }
    }
}

/// 管理员提交一条团队价目。单价是每 token 的价格，与桌面端价目一致。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SetTeamPriceRequest {
    pub model: String,
    /// 空或全空白表示按 model 兜底。
    #[serde(default)]
    pub provider: Option<String>,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_creation: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamPriceView {
    pub id: i64,
    pub model: String,
    pub provider: Option<String>,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_creation: f64,
    pub updated_at: DateTime<Utc>,
}

impl From<TeamPriceRow> for TeamPriceView {
    fn from(row: TeamPriceRow) -> Self {
        Self {
            id: row.id,
            model: row.model,
            provider: row.provider,
            input: row.input,
            output: row.output,
            cache_read: row.cache_read,
            cache_creation: row.cache_creation,
            updated_at: row.updated_at,
        }
    }
}

/// 内置 LiteLLM 快照的元信息，不带逐条单价。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotMetaView {
    pub as_of: String,
    pub source: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricingOverview {
    pub prices: Vec<TeamPriceView>,
    pub snapshot: SnapshotMetaView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetTeamPriceResponse {
    pub price: TeamPriceView,
    /// 因此变更重算了多少条已入库的消耗记录。
    pub recomputed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecomputeResponse {
    pub recomputed: u64,
}

/// `GET /api/v1/usage` 的查询串。`from` 含、`to` 不含，RFC 3339。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageQuery {
    pub account_id: Option<i64>,
    pub project_id: Option<i64>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// 一条消耗记录：客户端费用快照与服务端统一费用并列。跨人比较看 `unified_*`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageRecordView {
    pub id: i64,
    pub account_id: i64,
    pub account: String,
    pub occurred_at: DateTime<Utc>,
    pub source: String,
    pub model: String,
    pub provider: String,
    pub project: String,
    pub project_id: Option<i64>,
    pub session_id: String,
    pub tokens: UsageTokens,
    pub native_cost: Option<f64>,
    /// 推送当时客户端本机算的费用，只作对照。
    pub cost_snapshot: Option<f64>,
    pub pricing_source: String,
    /// 团队价目统一重算的费用；未定价为空。
    pub unified_cost: Option<f64>,
    /// 还没算过（旧数据）时为空。
    pub unified_pricing_source: Option<String>,
    /// `native` / `team` / `snapshot` / `none`。
    pub unified_cost_source: Option<String>,
}

impl From<UsageRow> for UsageRecordView {
    fn from(row: UsageRow) -> Self {
        Self {
            id: row.id,
            account_id: row.account_id,
            account: row.account,
            occurred_at: row.occurred_at,
            source: row.source,
            model: row.model,
            provider: row.provider,
            project: row.project_path,
            project_id: row.project_id,
            session_id: row.session_id,
            tokens: UsageTokens {
                input: row.input_tokens,
                output: row.output_tokens,
                cache_read: row.cache_read_tokens,
                cache_creation: row.cache_creation_tokens,
                reasoning: row.reasoning_tokens,
                total: row.total_tokens,
            },
            native_cost: row.native_cost,
            cost_snapshot: row.cost_snapshot,
            pricing_source: row.pricing_source,
            unified_cost: row.unified_cost,
            unified_pricing_source: row.unified_pricing_source,
            unified_cost_source: row.unified_cost_source,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageTotalsView {
    pub record_count: i64,
    pub total_tokens: i64,
    pub cost_snapshot_total: f64,
    pub unified_cost_total: f64,
    pub unified_unpriced_count: i64,
}

impl From<Totals> for UsageTotalsView {
    fn from(totals: Totals) -> Self {
        Self {
            record_count: totals.record_count,
            total_tokens: totals.total_tokens,
            cost_snapshot_total: totals.cost_snapshot_total,
            unified_cost_total: totals.unified_cost_total,
            unified_unpriced_count: totals.unified_unpriced_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageResponse {
    pub records: Vec<UsageRecordView>,
    /// 过滤条件下全部记录的合计，不受分页影响。
    pub totals: UsageTotalsView,
}

/// 管理员看的成员推送覆盖情况。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageView {
    pub account_id: i64,
    pub account: String,
    pub active: bool,
    pub device_count: i64,
    pub last_push_at: Option<DateTime<Utc>>,
    pub covered_through: Option<NaiveDate>,
}

impl From<MemberCoverage> for CoverageView {
    fn from(row: MemberCoverage) -> Self {
        Self {
            account_id: row.account_id,
            account: row.account,
            active: row.active,
            device_count: row.device_count,
            last_push_at: row.last_push_at,
            covered_through: row.covered_through,
        }
    }
}

/// `GET /api/v1/usage/summary` 的查询串。`from` 含、`to` 不含，RFC 3339。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SummaryQuery {
    pub account_id: Option<i64>,
    pub project_id: Option<i64>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// 按天切日界用的 UTC 偏移（分钟，东为正）。默认 0，即 UTC。
    pub tz_offset_minutes: Option<i32>,
}

/// 一个分组的合计：统一费用与客户端快照并列。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BreakdownView {
    /// 日期（`YYYY-MM-DD`）、账号名、来源、模型，或项目 key（没有项目为空串）。
    pub key: String,
    pub label: String,
    /// 账号或项目的 id，其它维度为空。
    pub id: Option<i64>,
    pub record_count: i64,
    pub total_tokens: i64,
    pub cost_snapshot: f64,
    pub unified_cost: f64,
    /// 统一价未定价的记录数；大于 0 时统一费用是下限。
    pub unpriced_count: i64,
}

impl From<BreakdownRow> for BreakdownView {
    fn from(row: BreakdownRow) -> Self {
        Self {
            key: row.key,
            label: row.label,
            id: row.id,
            record_count: row.record_count,
            total_tokens: row.total_tokens,
            cost_snapshot: row.cost_snapshot,
            unified_cost: row.unified_cost,
            unpriced_count: row.unpriced_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryResponse {
    pub totals: UsageTotalsView,
    /// 按日期升序。
    pub by_day: Vec<BreakdownView>,
    /// 以下四个维度按统一费用从高到低。
    pub by_account: Vec<BreakdownView>,
    pub by_source: Vec<BreakdownView>,
    pub by_model: Vec<BreakdownView>,
    pub by_project: Vec<BreakdownView>,
}

impl From<Summary> for SummaryResponse {
    fn from(summary: Summary) -> Self {
        fn views(rows: Vec<BreakdownRow>) -> Vec<BreakdownView> {
            rows.into_iter().map(Into::into).collect()
        }
        Self {
            totals: summary.totals.into(),
            by_day: views(summary.by_day),
            by_account: views(summary.by_account),
            by_source: views(summary.by_source),
            by_model: views(summary.by_model),
            by_project: views(summary.by_project),
        }
    }
}

/// `GET /api/v1/sessions` 的查询串。`from` 含、`to` 不含，按会话结束时间。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionListQuery {
    pub account_id: Option<i64>,
    pub project_id: Option<i64>,
    /// 按「码表生成」标记过滤：`true` 只看它们，`false` 排除它们，不传不过滤。
    pub generated_by_work_notes: Option<bool>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// 列表里的会话只有目录元数据；正文在会话详情里，不在这里。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionListItem {
    pub id: i64,
    pub account_id: i64,
    pub account: String,
    pub device_name: String,
    pub source: String,
    pub session_id: String,
    pub title: String,
    pub project: String,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub model: String,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub event_count: i32,
    pub generated_by_work_notes: bool,
    pub pushed_at: DateTime<Utc>,
}

impl From<SessionListRow> for SessionListItem {
    fn from(row: SessionListRow) -> Self {
        Self {
            id: row.id,
            account_id: row.account_id,
            account: row.account,
            device_name: row.device_name,
            source: row.source,
            session_id: row.session_id,
            title: row.title,
            project: row.project_path,
            project_id: row.project_id,
            project_name: row.project_name,
            model: row.model,
            started_at: row.started_at,
            ended_at: row.ended_at,
            event_count: row.event_count,
            generated_by_work_notes: row.generated_by_work_notes,
            pushed_at: row.pushed_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionListItem>,
    /// 过滤条件下的会话总数，不受分页影响。
    pub total: i64,
}
