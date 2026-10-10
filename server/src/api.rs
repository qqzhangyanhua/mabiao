//! 管理接口与账号接口的响应形状。只有服务端与它自带的网页用，不进 `push-protocol`。

use chrono::{DateTime, NaiveDate, Utc};
use push_protocol::{RemoteRole, UsageTokens};
use serde::{Deserialize, Serialize};

use crate::accounts::AccountRow;
use crate::coverage::MemberCoverage;
use crate::devices::DeviceRow;
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
