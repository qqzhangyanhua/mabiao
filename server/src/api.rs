//! 管理接口与账号接口的请求 / 响应形状。只有服务端与它自带的网页用，不进 `push-protocol`。
//!
//! 不含敏感列、且字段形状与线上一致的只读查询结构（`DeviceRow`、`TeamPriceRow`、`Totals`、
//! `MemberCoverage`、`BreakdownRow`、`Summary`、`SessionListRow`、`Composition`、`Behavior` 等）
//! 直接序列化返回；这里只保留需要脱敏（如 `AccountView` 剔除 `password_hash`）、需要结构转换
//! （如 `UsageRecordView` 聚合 `tokens`）或组合封包的类型。

use chrono::{DateTime, Utc};
use push_protocol::{RemoteRole, UsageTokens};
use serde::{Deserialize, Serialize};

use crate::accounts::AccountRow;
use crate::scope::ScopeInput;
use crate::sessions::SessionListRow;
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

/// 内置 LiteLLM 快照的元信息，不带逐条单价。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotMetaView {
    pub as_of: String,
    pub source: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricingOverview {
    pub prices: Vec<TeamPriceRow>,
    pub snapshot: SnapshotMetaView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetTeamPriceResponse {
    pub price: TeamPriceRow,
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
    /// 精确匹配来源。空表示不过滤。
    pub source: Option<String>,
    /// 精确匹配模型。空表示不过滤。
    pub model: Option<String>,
    /// `native` / `team` / `snapshot` / `none`。
    pub unified_cost_source: Option<String>,
}

impl UsageQuery {
    pub fn scope_input(&self) -> ScopeInput<'_> {
        ScopeInput {
            account_id: self.account_id,
            project_id: self.project_id,
            from: self.from.as_deref(),
            to: self.to.as_deref(),
        }
    }
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
    /// 对上已推送会话时是会话主键，网页用它打开详情。
    pub session_pk: Option<i64>,
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
            session_pk: row.session_pk,
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
pub struct UsageResponse {
    pub records: Vec<UsageRecordView>,
    /// 过滤条件下全部记录的合计，不受分页影响。
    pub totals: Totals,
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

impl SummaryQuery {
    pub fn scope_input(&self) -> ScopeInput<'_> {
        ScopeInput {
            account_id: self.account_id,
            project_id: self.project_id,
            from: self.from.as_deref(),
            to: self.to.as_deref(),
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

impl SessionListQuery {
    pub fn scope_input(&self) -> ScopeInput<'_> {
        ScopeInput {
            account_id: self.account_id,
            project_id: self.project_id,
            from: self.from.as_deref(),
            to: self.to.as_deref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionListRow>,
    /// 过滤条件下的会话总数，不受分页影响。
    pub total: i64,
}

/// 账号、项目、时间。构成与行为指标共用，不含分页。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScopeQuery {
    pub account_id: Option<i64>,
    pub project_id: Option<i64>,
    pub from: Option<String>,
    pub to: Option<String>,
}

impl ScopeQuery {
    pub fn scope_input(&self) -> ScopeInput<'_> {
        ScopeInput {
            account_id: self.account_id,
            project_id: self.project_id,
            from: self.from.as_deref(),
            to: self.to.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::Value;

    use super::*;
    use crate::api_detail::SessionDetailView;
    use crate::project_admin::{ProjectListRow, ProjectRow};
    use crate::sessions::SessionDetailRow;
    use crate::usage_query::{KeyedTokens, TokenParts};

    #[test]
    fn direct_query_projections_preserve_exact_wire_shapes() {
        let ts = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();

        let bucket = serde_json::to_value(KeyedTokens {
            key: "codex".into(),
            tokens: TokenParts {
                input: 10,
                output: 4,
                cache_read: 6,
                cache_creation: 1,
                reasoning: 2,
            },
        })
        .unwrap();
        assert_eq!(bucket["key"], "codex");
        assert_eq!(bucket["input"], 10);
        assert_eq!(bucket["reasoning"], 2);
        assert!(
            bucket.get("tokens").is_none(),
            "composition bucket must stay flat: {bucket}"
        );

        let project = serde_json::to_value(ProjectRow {
            id: 7,
            key: "git:github.com/team/alpha".into(),
            name: "alpha".into(),
            git_remote: Some("github.com/team/alpha".into()),
            merged_into: Some(99),
            created_at: ts,
        })
        .unwrap();
        assert_eq!(project["id"], 7);
        assert_eq!(project["name"], "alpha");
        assert!(
            project.get("merged_into").is_none(),
            "merged_into must not leak: {project}"
        );

        let admin_project = serde_json::to_value(ProjectListRow {
            id: 7,
            key: "git:github.com/team/alpha".into(),
            name: "alpha".into(),
            git_remote: None,
            created_at: ts,
            session_count: 2,
            usage_record_count: 3,
            merged_count: 1,
        })
        .unwrap();
        assert_eq!(admin_project["session_count"], 2);
        assert_eq!(admin_project["merged_count"], 1);
        assert!(
            admin_project.get("project").is_none(),
            "admin project must stay flat"
        );

        let session = SessionListRow {
            id: 1,
            account_id: 2,
            account: "alice".into(),
            device_name: "laptop".into(),
            source: "codex".into(),
            session_id: "s-1".into(),
            title: "t".into(),
            project_path: "/w/alpha".into(),
            project_id: Some(7),
            project_name: Some("alpha".into()),
            model: "gpt-5".into(),
            started_at: Some(ts),
            ended_at: Some(ts),
            event_count: 3,
            generated_by_work_notes: false,
            pushed_at: ts,
        };
        let session_json = serde_json::to_value(&session).unwrap();
        assert_eq!(session_json["project"], "/w/alpha");
        assert!(session_json.get("project_path").is_none());

        let detail = serde_json::to_value(SessionDetailView::new(
            SessionDetailRow {
                item: session,
                device_pk: 42,
                source_files: vec!["/a.jsonl".into()],
                redaction_count: 0,
                events: vec![],
                context_manifest: None,
            },
            vec![],
        ))
        .unwrap();
        assert!(
            detail.get("device_pk").is_none(),
            "device_pk must not leak: {detail}"
        );
        assert_eq!(detail["session"]["project"], "/w/alpha");

        let account = serde_json::to_value(AccountView {
            id: 2,
            account: "alice".into(),
            role: RemoteRole::Member,
            active: true,
            created_at: ts,
            deactivated_at: None,
        })
        .unwrap();
        assert!(account.get("password_hash").is_none());
        let obj: &serde_json::Map<String, Value> = account.as_object().unwrap();
        assert_eq!(obj.len(), 6);
    }
}
