//! 团队价目与统一费用（ADR 0026「统一价目」）。
//!
//! 每个成员本机价目不同，同样的 token 会算出不同的钱。服务端用团队价目 + 内置 LiteLLM 快照，
//! 按与桌面端相同的优先级重算：来源自带 `native_cost` > 团队价目精确匹配 > 按 model 兜底（团队价目
//! 或快照）> 未定价。规则本体在 `crates/pricing/`，这里只负责拼价表、读写库、批量套用。
//! 统一费用另存三列，不覆盖客户端的 `cost_snapshot` / `pricing_source`，两者并列返回。

use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use pricing::{
    price_usage_cached, CostSource, PriceCache, PriceEntry, PriceOrigin, PriceTable, PricedUsage,
    PricingBasis,
};
use push_protocol::PricingSource;
use serde::Deserialize;
use sqlx::{PgConnection, PgExecutor, PgPool};

use crate::api::SetTeamPriceRequest;
use crate::error::AppError;
use crate::usage::pricing_source_str;

/// 与桌面端共用同一份内置快照文件，避免两边各带一份而漂移。Docker 构建要把它带进上下文。
const BUNDLED_JSON: &str = include_str!("../../src-tauri/assets/litellm_prices.json");

/// 每 token 单价的上限，挡掉把「每百万 token」误填成「每 token」之类的量级错误。
pub const MAX_PRICE_PER_TOKEN: f64 = 1000.0;
const MAX_NAME_CHARS: usize = 512;
/// 一次重算读多少行；一批一条 UPDATE。
const RECOMPUTE_BATCH: i64 = 5000;
/// 咨询锁的键。推送消耗记录取共享锁，改价目 / 重算取排他锁：
/// 重算期间的推送要等它提交，不会带着旧价目的结果落库后无人再算。
const PRICING_LOCK_KEY: i64 = 0x6d61_6269_616f;

#[derive(Debug, Deserialize)]
pub struct Snapshot {
    pub as_of: String,
    pub source: String,
    pub entries: Vec<PriceEntry>,
}

pub fn builtin_snapshot() -> &'static Snapshot {
    static SNAPSHOT: OnceLock<Snapshot> = OnceLock::new();
    SNAPSHOT.get_or_init(|| {
        serde_json::from_str(BUNDLED_JSON).expect("内置 LiteLLM 快照应该能解析（由单测守住）")
    })
}

/// 合并出生效价表。与桌面端 `litellm::merge` 同一规则：团队价目始终优先，快照只补齐团队
/// 完全没配过的模型；团队只要给某模型配了任意单价（精确或按模型），该模型就不再引入快照兜底。
pub fn effective_table(team: &[PriceEntry], snapshot: &[PriceEntry]) -> PriceTable {
    let priced: std::collections::HashSet<&str> = team.iter().map(|p| p.model.as_str()).collect();
    let mut prices: Vec<PriceEntry> = team
        .iter()
        .cloned()
        .map(|mut entry| {
            entry.origin = PriceOrigin::User;
            entry
        })
        .collect();
    for entry in snapshot {
        if !priced.contains(entry.model.as_str()) {
            let mut fallback = entry.clone();
            fallback.origin = PriceOrigin::Snapshot;
            prices.push(fallback);
        }
    }
    PriceTable { prices }
}

/// 一条消耗记录的统一费用。
#[derive(Debug, Clone, PartialEq)]
pub struct UnifiedCost {
    pub cost: Option<f64>,
    pub pricing_source: PricingSource,
    /// 钱来自谁：`native` / `team` / `snapshot` / `none`。
    pub cost_source: &'static str,
}

fn cost_source_str(source: CostSource) -> &'static str {
    match source {
        CostSource::Native => "native",
        CostSource::User => "team",
        CostSource::Snapshot => "snapshot",
        CostSource::None => "none",
    }
}

/// 套用价表。不开签名模糊匹配：那是 Cursor 账号事件专用，推送不含这类记录。
pub fn unify<'p, 'r>(cache: &mut PriceCache<'p, 'r>, usage: PricedUsage<'r>) -> UnifiedCost {
    let priced = price_usage_cached(cache, usage, false);
    UnifiedCost {
        cost: priced.derived.amount,
        pricing_source: match priced.basis {
            PricingBasis::Native => PricingSource::Native,
            PricingBasis::Exact => PricingSource::Exact,
            PricingBasis::Fallback => PricingSource::Fallback,
            PricingBasis::Unpriced => PricingSource::Unpriced,
        },
        cost_source: cost_source_str(priced.derived.cost_source),
    }
}

// ---------- 价目维护 ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TeamPriceRow {
    pub id: i64,
    pub model: String,
    pub provider: Option<String>,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_creation: f64,
    pub updated_at: DateTime<Utc>,
}

/// 管理员提交的一条价目，已校验、已规整。
#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedPrice {
    pub model: String,
    pub provider: Option<String>,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_creation: f64,
}

pub fn normalize_price(request: &SetTeamPriceRequest) -> Result<NormalizedPrice, AppError> {
    let model = request.model.trim();
    if model.is_empty() {
        return Err(AppError::invalid("model 不能为空"));
    }
    let provider = request
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    for (name, value) in [("model", Some(model)), ("provider", provider)] {
        if value.is_some_and(|v| v.chars().count() > MAX_NAME_CHARS) {
            return Err(AppError::invalid(format!(
                "{name} 最多 {MAX_NAME_CHARS} 个字符"
            )));
        }
    }
    for (name, value) in [
        ("input", request.input),
        ("output", request.output),
        ("cache_read", request.cache_read),
        ("cache_creation", request.cache_creation),
    ] {
        if !value.is_finite() || !(0.0..=MAX_PRICE_PER_TOKEN).contains(&value) {
            return Err(AppError::invalid(format!(
                "{name} 单价要在 0 到 {MAX_PRICE_PER_TOKEN} 之间（每 token 的价格）"
            )));
        }
    }
    Ok(NormalizedPrice {
        model: model.to_owned(),
        provider: provider.map(str::to_owned),
        input: request.input,
        output: request.output,
        cache_read: request.cache_read,
        cache_creation: request.cache_creation,
    })
}

pub async fn list(executor: impl PgExecutor<'_>) -> Result<Vec<TeamPriceRow>, AppError> {
    Ok(sqlx::query_as(
        "SELECT id, model, provider, input, output, cache_read, cache_creation, updated_at
         FROM team_prices ORDER BY id",
    )
    .fetch_all(executor)
    .await?)
}

/// 同（model, provider）（都不分大小写）已有就原地改，没有就新增。
pub async fn upsert(
    conn: &mut PgConnection,
    price: &NormalizedPrice,
    updated_by: i64,
) -> Result<TeamPriceRow, AppError> {
    Ok(sqlx::query_as(
        "INSERT INTO team_prices (model, provider, input, output, cache_read, cache_creation, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (lower(model), lower(coalesce(provider, ''))) DO UPDATE SET
             model = EXCLUDED.model,
             provider = EXCLUDED.provider,
             input = EXCLUDED.input,
             output = EXCLUDED.output,
             cache_read = EXCLUDED.cache_read,
             cache_creation = EXCLUDED.cache_creation,
             updated_by = EXCLUDED.updated_by,
             updated_at = now()
         RETURNING id, model, provider, input, output, cache_read, cache_creation, updated_at",
    )
    .bind(&price.model)
    .bind(&price.provider)
    .bind(price.input)
    .bind(price.output)
    .bind(price.cache_read)
    .bind(price.cache_creation)
    .bind(updated_by)
    .fetch_one(conn)
    .await?)
}

/// 返回被删价目的 model，不存在返回 `None`。
pub async fn delete(conn: &mut PgConnection, id: i64) -> Result<Option<String>, AppError> {
    Ok(
        sqlx::query_scalar("DELETE FROM team_prices WHERE id = $1 RETURNING model")
            .bind(id)
            .fetch_optional(conn)
            .await?,
    )
}

/// 推送消耗记录时取共享锁：与改价目 / 重算互斥，但推送之间互不阻塞。
pub async fn lock_shared(conn: &mut PgConnection) -> Result<(), AppError> {
    sqlx::query("SELECT pg_advisory_xact_lock_shared($1)")
        .bind(PRICING_LOCK_KEY)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn lock_exclusive(conn: &mut PgConnection) -> Result<(), AppError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(PRICING_LOCK_KEY)
        .execute(conn)
        .await?;
    Ok(())
}

/// 当前生效价表：库里的团队价目 + 内置快照。
pub async fn load_table(conn: &mut PgConnection) -> Result<PriceTable, AppError> {
    let team: Vec<PriceEntry> = list(conn)
        .await?
        .into_iter()
        .map(|row| PriceEntry {
            model: row.model,
            provider: row.provider,
            input: row.input,
            output: row.output,
            cache_read: row.cache_read,
            cache_creation: row.cache_creation,
            origin: PriceOrigin::User,
        })
        .collect();
    Ok(effective_table(&team, &builtin_snapshot().entries))
}

// ---------- 重算 ----------

/// 重算哪些记录。
#[derive(Debug, Clone, Copy)]
pub enum Scope<'a> {
    All,
    /// 某个 model 的全部记录（不分大小写）：团队价目只会影响同名模型。
    Model(&'a str),
    /// 从没算过统一费用的记录（旧数据）。
    Missing,
}

#[derive(sqlx::FromRow)]
struct UsageForPricing {
    id: i64,
    model: String,
    provider: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    native_cost: Option<f64>,
}

/// 按 `table` 重算范围内的记录，返回更新的行数。调用方负责持有排他锁并放进事务。
pub async fn recompute(
    conn: &mut PgConnection,
    table: &PriceTable,
    scope: Scope<'_>,
) -> Result<u64, AppError> {
    let (model, missing_only) = match scope {
        Scope::All => (None, false),
        Scope::Model(model) => (Some(model), false),
        Scope::Missing => (None, true),
    };
    let mut after = 0_i64;
    let mut updated = 0_u64;
    loop {
        let rows: Vec<UsageForPricing> = sqlx::query_as(
            "SELECT id, model, provider, input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens, native_cost
             FROM usage_records
             WHERE id > $1
               AND ($2::text IS NULL OR lower(model) = lower($2))
               AND (NOT $3 OR unified_pricing_source IS NULL)
             ORDER BY id LIMIT $4",
        )
        .bind(after)
        .bind(model)
        .bind(missing_only)
        .bind(RECOMPUTE_BATCH)
        .fetch_all(&mut *conn)
        .await?;
        let Some(last) = rows.last() else { break };
        after = last.id;

        let mut cache = PriceCache::new(table);
        let mut ids = Vec::with_capacity(rows.len());
        let mut costs = Vec::with_capacity(rows.len());
        let mut sources = Vec::with_capacity(rows.len());
        let mut origins = Vec::with_capacity(rows.len());
        for row in &rows {
            let unified = unify(
                &mut cache,
                PricedUsage {
                    model: &row.model,
                    provider: &row.provider,
                    input_tokens: row.input_tokens,
                    output_tokens: row.output_tokens,
                    cache_read_tokens: row.cache_read_tokens,
                    cache_creation_tokens: row.cache_creation_tokens,
                    native_cost: row.native_cost,
                },
            );
            ids.push(row.id);
            costs.push(unified.cost);
            sources.push(pricing_source_str(unified.pricing_source));
            origins.push(unified.cost_source);
        }
        updated += sqlx::query(
            "UPDATE usage_records u SET
                 unified_cost = v.cost,
                 unified_pricing_source = v.source,
                 unified_cost_source = v.origin
             FROM UNNEST($1::bigint[], $2::float8[], $3::text[], $4::text[])
                  AS v(id, cost, source, origin)
             WHERE u.id = v.id",
        )
        .bind(&ids)
        .bind(&costs)
        .bind(&sources)
        .bind(&origins)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    }
    Ok(updated)
}

/// 价目变更或手动重算的入口：一个事务里拿排他锁、读最新价表、重算。
pub async fn recompute_locked(pool: &PgPool, scope: Scope<'_>) -> Result<u64, AppError> {
    let mut tx = pool.begin().await?;
    lock_exclusive(&mut tx).await?;
    let table = load_table(&mut tx).await?;
    let updated = recompute(&mut tx, &table, scope).await?;
    tx.commit().await?;
    Ok(updated)
}

/// 补算还没有统一费用的记录（本迁移之前入库的）。服务启动时跑一次，已算过的不动。
pub async fn backfill_missing(pool: &PgPool) -> Result<u64, AppError> {
    recompute_locked(pool, Scope::Missing).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use pricing::{price_usage, PriceTable};

    fn entry(model: &str, provider: Option<&str>, input: f64) -> PriceEntry {
        PriceEntry {
            model: model.into(),
            provider: provider.map(str::to_owned),
            input,
            output: input,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }
    }

    fn usage<'a>(model: &'a str, provider: &'a str, native: Option<f64>) -> PricedUsage<'a> {
        PricedUsage {
            model,
            provider,
            input_tokens: 1000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            native_cost: native,
        }
    }

    #[test]
    fn builtin_snapshot_parses_and_is_not_empty() {
        let snapshot = builtin_snapshot();
        assert_eq!(snapshot.source, "litellm");
        assert!(!snapshot.as_of.is_empty());
        assert!(snapshot.entries.len() > 100);
        assert!(snapshot.entries.iter().all(|e| e.provider.is_none()));
    }

    #[test]
    fn team_model_hides_the_snapshot_entries_of_that_model_only() {
        let team = [entry("a", Some("p"), 1.0)];
        let snapshot = [entry("a", None, 9.0), entry("b", None, 9.0)];

        let table = effective_table(&team, &snapshot);

        assert_eq!(table.prices.len(), 2);
        assert_eq!(table.prices[0].origin, PriceOrigin::User);
        assert_eq!(table.prices[1].model, "b");
        assert_eq!(table.prices[1].origin, PriceOrigin::Snapshot);
    }

    #[test]
    fn unify_follows_the_documented_priority() {
        let team = [entry("m", Some("p"), 2.0), entry("m2", None, 3.0)];
        let snapshot = [entry("m3", None, 4.0)];
        let table = effective_table(&team, &snapshot);
        let mut cache = PriceCache::new(&table);

        let native = unify(&mut cache, usage("m", "p", Some(7.0)));
        let exact = unify(&mut cache, usage("m", "p", None));
        let team_fallback = unify(&mut cache, usage("m2", "x", None));
        let from_snapshot = unify(&mut cache, usage("m3", "", None));
        let none = unify(&mut cache, usage("m4", "", None));

        assert_eq!(
            (native.cost, native.pricing_source, native.cost_source),
            (Some(7.0), PricingSource::Native, "native")
        );
        assert_eq!(
            (exact.cost, exact.pricing_source, exact.cost_source),
            (Some(2000.0), PricingSource::Exact, "team")
        );
        assert_eq!(
            (team_fallback.cost, team_fallback.pricing_source),
            (Some(3000.0), PricingSource::Fallback)
        );
        assert_eq!(
            (from_snapshot.cost, from_snapshot.cost_source),
            (Some(4000.0), "snapshot")
        );
        assert_eq!(
            (none.cost, none.pricing_source, none.cost_source),
            (None, PricingSource::Unpriced, "none")
        );
    }

    #[test]
    fn unify_agrees_with_single_record_pricing() {
        let table = PriceTable {
            prices: vec![entry("m", None, 2.0)],
        };
        let mut cache = PriceCache::new(&table);

        let unified = unify(&mut cache, usage("M", "", None));
        let single = price_usage(usage("M", "", None), &table, false);

        assert_eq!(unified.cost, single.derived.amount);
    }

    fn request(model: &str, provider: Option<&str>, prices: [f64; 4]) -> SetTeamPriceRequest {
        SetTeamPriceRequest {
            model: model.into(),
            provider: provider.map(str::to_owned),
            input: prices[0],
            output: prices[1],
            cache_read: prices[2],
            cache_creation: prices[3],
        }
    }

    #[test]
    fn normalize_trims_and_treats_blank_provider_as_model_only() {
        let price =
            normalize_price(&request("  gpt-5 ", Some("  "), [1e-6, 2e-6, 0.0, 0.0])).unwrap();

        assert_eq!(price.model, "gpt-5");
        assert_eq!(price.provider, None);
    }

    #[test]
    fn normalize_rejects_bad_names_and_out_of_range_prices() {
        let zero = [0.0; 4];
        let bad = [
            request("", None, zero),
            request("m", None, [-1e-9, 0.0, 0.0, 0.0]),
            request("m", None, [f64::NAN, 0.0, 0.0, 0.0]),
            request("m", None, [0.0, f64::INFINITY, 0.0, 0.0]),
            request("m", None, [0.0, 0.0, MAX_PRICE_PER_TOKEN + 1.0, 0.0]),
            request(&"x".repeat(MAX_NAME_CHARS + 1), None, zero),
            request("m", Some(&"x".repeat(MAX_NAME_CHARS + 1)), zero),
        ];
        for request in &bad {
            assert!(normalize_price(request).is_err(), "{request:?}");
        }
        let edge = request("m", None, [0.0, 0.0, 0.0, MAX_PRICE_PER_TOKEN]);
        assert!(normalize_price(&edge).is_ok());
    }
}
