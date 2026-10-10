use chrono::{DateTime, Utc};
use pricing::{PriceCache, PriceTable, PricedUsage};
use push_protocol::{PricingSource, UsageRecordPayload};
use sqlx::{PgConnection, QueryBuilder};

use crate::error::AppError;
use crate::team_pricing::{unify, UnifiedCost};

/// 一次 INSERT 的行数。每行 23 个绑定参数，远低于 PostgreSQL 的 65535 上限。
const ROWS_PER_STATEMENT: usize = 1000;

pub fn pricing_source_str(source: PricingSource) -> &'static str {
    match source {
        PricingSource::Native => "native",
        PricingSource::Exact => "exact",
        PricingSource::Fallback => "fallback",
        PricingSource::Unpriced => "unpriced",
    }
}

/// 已校验、已解析的一行，连同它所属的项目。
pub struct PreparedUsage<'a> {
    pub record: &'a UsageRecordPayload,
    pub occurred_at: DateTime<Utc>,
    pub project_id: Option<i64>,
}

/// 按（账号, 设备, 指纹）去重插入，返回真正新增的行数。
///
/// 费用快照、`native_cost` 与定价来源原样入库，不改写；另按 `table`（团队价目 + 内置快照）
/// 算出统一费用一并写入。请求内部与库里已有的重复指纹都被 `DO NOTHING` 跳过，所以重复推送
/// 不会重复计数。
pub async fn insert_new(
    conn: &mut PgConnection,
    account_id: i64,
    device_pk: i64,
    rows: &[PreparedUsage<'_>],
    table: &PriceTable,
) -> Result<u64, AppError> {
    let mut inserted = 0;
    let mut cache = PriceCache::new(table);
    for chunk in rows.chunks(ROWS_PER_STATEMENT) {
        let unified: Vec<UnifiedCost> = chunk
            .iter()
            .map(|item| {
                let r = item.record;
                unify(
                    &mut cache,
                    PricedUsage {
                        model: &r.model,
                        provider: &r.provider,
                        input_tokens: r.tokens.input,
                        output_tokens: r.tokens.output,
                        cache_read_tokens: r.tokens.cache_read,
                        cache_creation_tokens: r.tokens.cache_creation,
                        native_cost: r.native_cost,
                    },
                )
            })
            .collect();
        let mut query = QueryBuilder::new(
            "INSERT INTO usage_records (
                 account_id, device_pk, fingerprint, occurred_at, source, model, provider,
                 project_path, project_id, session_id, source_file,
                 input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                 reasoning_tokens, total_tokens, native_cost, cost_snapshot, pricing_source,
                 unified_cost, unified_pricing_source, unified_cost_source) ",
        );
        query.push_values(chunk.iter().zip(&unified), |mut row, (item, unified)| {
            let r = item.record;
            row.push_bind(account_id)
                .push_bind(device_pk)
                .push_bind(&r.fingerprint)
                .push_bind(item.occurred_at)
                .push_bind(&r.source)
                .push_bind(&r.model)
                .push_bind(&r.provider)
                .push_bind(&r.project)
                .push_bind(item.project_id)
                .push_bind(&r.session_id)
                .push_bind(&r.source_file)
                .push_bind(r.tokens.input)
                .push_bind(r.tokens.output)
                .push_bind(r.tokens.cache_read)
                .push_bind(r.tokens.cache_creation)
                .push_bind(r.tokens.reasoning)
                .push_bind(r.tokens.total)
                .push_bind(r.native_cost)
                .push_bind(r.cost_snapshot)
                .push_bind(pricing_source_str(r.pricing_source))
                .push_bind(unified.cost)
                .push_bind(pricing_source_str(unified.pricing_source))
                .push_bind(unified.cost_source);
        });
        query.push(" ON CONFLICT (account_id, device_pk, fingerprint) DO NOTHING");
        inserted += query.build().execute(&mut *conn).await?.rows_affected();
    }
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_source_strings_match_the_wire_names_and_the_check_constraint() {
        for source in [
            PricingSource::Native,
            PricingSource::Exact,
            PricingSource::Fallback,
            PricingSource::Unpriced,
        ] {
            let wire = serde_json::to_value(source).unwrap();
            assert_eq!(wire.as_str(), Some(pricing_source_str(source)));
        }
    }
}
