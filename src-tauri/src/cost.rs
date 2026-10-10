use std::collections::{BTreeMap, BTreeSet};

use pricing::{
    apply_entry, apply_entry_attribution, derive_priced, find_price, find_price_by_signature,
    CostAttribution, PriceCache, PricedUsage,
};

use crate::domain::{
    CostSource, CursorUsageEvent, DerivedCost, OverviewCostBreakdown, OverviewCostSources,
    PriceEntry, PriceTable, UnpricedGroupDto, UnpricedReason, UsageRecord,
};

fn add_amount(slot: &mut Option<f64>, value: Option<f64>) {
    if let Some(amount) = value {
        *slot = Some(slot.unwrap_or(0.0) + amount);
    }
}

fn fold_overview_costs(
    items: impl IntoIterator<Item = CostAttribution>,
) -> (
    Option<f64>,
    bool,
    OverviewCostBreakdown,
    OverviewCostSources,
) {
    let mut total = 0.0;
    let mut any = false;
    let mut unpriced = false;
    let mut breakdown = OverviewCostBreakdown::default();
    let mut sources = OverviewCostSources::default();
    for item in items {
        if let Some(amount) = item.derived.amount {
            total += amount;
            any = true;
        }
        if item.derived.unpriced {
            unpriced = true;
            sources.unpriced_records += 1;
        }
        add_amount(&mut breakdown.input, item.input);
        add_amount(&mut breakdown.output, item.output);
        add_amount(&mut breakdown.cache_read, item.cache_read);
        add_amount(&mut breakdown.cache_creation, item.cache_creation);
        match item.derived.cost_source {
            CostSource::Native => add_amount(&mut sources.native, item.derived.amount),
            CostSource::User => add_amount(&mut sources.user, item.derived.amount),
            CostSource::Snapshot => add_amount(&mut sources.snapshot, item.derived.amount),
            CostSource::None => {}
        }
    }
    (
        if any { Some(total) } else { None },
        unpriced,
        breakdown,
        sources,
    )
}

/// 概览费用：总额 + 四档口径 + 来源分层。优先级与 `sum_costs` 相同。
pub fn overview_costs(
    records: &[&UsageRecord],
    prices: &PriceTable,
) -> (
    Option<f64>,
    bool,
    OverviewCostBreakdown,
    OverviewCostSources,
) {
    let mut cache = PriceCache::new(prices);
    fold_overview_costs(records.iter().map(|record| {
        let usage = PricedUsage {
            model: &record.model,
            provider: &record.provider,
            input_tokens: record.input_tokens,
            output_tokens: record.output_tokens,
            cache_read_tokens: record.cache_read_tokens,
            cache_creation_tokens: record.cache_creation_tokens,
            native_cost: record.native_cost,
        };
        if usage.native_cost.is_some() {
            return apply_entry_attribution(&usage, None);
        }
        let entry = cache.resolve(&record.model, &record.provider, false);
        apply_entry_attribution(&usage, entry)
    }))
}

pub fn derive_cost(record: &UsageRecord, prices: &PriceTable) -> DerivedCost {
    derive_priced(
        PricedUsage {
            model: &record.model,
            provider: &record.provider,
            input_tokens: record.input_tokens,
            output_tokens: record.output_tokens,
            cache_read_tokens: record.cache_read_tokens,
            cache_creation_tokens: record.cache_creation_tokens,
            native_cost: record.native_cost,
        },
        prices,
        false,
    )
}

/// 诊断路径：精确查价未命中时，给出签名兼容的最佳候选条目。
///
/// 复用 [`find_price_by_signature`] 的启发式打分，不另造匹配逻辑。
/// **不**用于消耗记录费用推导——那边的签名模糊匹配保持关闭。
///
/// - 已有精确价（model+provider，或 model 且 provider 为空）时返回空
/// - 用户价目在打分里优先于快照
/// - 完全对不上时返回空
/// - 返回的条目保持被命中价目的形状（含四个口径与来源），可直接预填
pub fn snapshot_price_candidate(model: &str, prices: &PriceTable) -> Option<PriceEntry> {
    if model.is_empty() {
        return None;
    }
    if find_price(model, "", prices).is_some() {
        return None;
    }
    find_price_by_signature(model, prices).cloned()
}

/// 给未定价诊断的可补组挂上快照候选。结构性那档（空模型名）不查。
fn attach_snapshot_candidates(groups: &mut [UnpricedGroupDto], prices: &PriceTable) {
    for group in groups {
        group.candidate = if group.reason == UnpricedReason::Pricable {
            snapshot_price_candidate(&group.model, prices)
        } else {
            None
        };
    }
}

#[derive(Debug, Default)]
pub(crate) struct UnpricedGroupAcc {
    pub sources: BTreeSet<String>,
    pub total_tokens: i64,
    pub record_count: i64,
}

/// 未定价诊断收尾：reason、排序、快照候选。
///
/// query / aggregate 只按 `(model, provider)` 累加；滤行仍走各自的 SQL 或 `derive_cost`。
pub(crate) fn finish_unpriced_groups(
    groups: BTreeMap<(String, String), UnpricedGroupAcc>,
    prices: &PriceTable,
) -> Vec<UnpricedGroupDto> {
    let mut rows: Vec<UnpricedGroupDto> = groups
        .into_iter()
        .map(|((model, provider), acc)| UnpricedGroupDto {
            reason: if model.is_empty() {
                UnpricedReason::StructurallyUnbillable
            } else {
                UnpricedReason::Pricable
            },
            model,
            provider,
            sources: acc.sources.into_iter().collect(),
            total_tokens: acc.total_tokens,
            record_count: acc.record_count,
            candidate: None,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.model.cmp(&b.model))
            .then_with(|| a.provider.cmp(&b.provider))
    });
    attach_snapshot_candidates(&mut rows, prices);
    rows
}

pub fn sum_costs(records: &[&UsageRecord], prices: &PriceTable) -> (Option<f64>, bool) {
    let mut cache = PriceCache::new(prices);
    accumulate_costs(records.iter().map(|record| {
        let usage = PricedUsage {
            model: &record.model,
            provider: &record.provider,
            input_tokens: record.input_tokens,
            output_tokens: record.output_tokens,
            cache_read_tokens: record.cache_read_tokens,
            cache_creation_tokens: record.cache_creation_tokens,
            native_cost: record.native_cost,
        };
        if usage.native_cost.is_some() {
            return apply_entry(&usage, None);
        }
        let entry = cache.resolve(&record.model, &record.provider, false);
        apply_entry(&usage, entry)
    }))
}

/// Cursor 账号事件没有 native_cost，按模型走用户价目 / LiteLLM 快照。
/// 精确名对不上时，再按家族+版本+档位签名匹配（如 `claude-4.6-sonnet` → `claude-sonnet-4-6`）。
pub fn sum_cursor_event_costs(
    events: &[&CursorUsageEvent],
    prices: &PriceTable,
) -> (Option<f64>, bool) {
    let mut cache = PriceCache::new(prices);
    accumulate_costs(events.iter().map(|event| {
        let usage = PricedUsage {
            model: &event.model,
            provider: "",
            input_tokens: event.input_tokens,
            output_tokens: event.output_tokens,
            cache_read_tokens: event.cache_read_tokens,
            cache_creation_tokens: event.cache_creation_tokens,
            native_cost: None,
        };
        let entry = cache.resolve(&event.model, "", true);
        apply_entry(&usage, entry)
    }))
}

fn accumulate_costs(derived: impl IntoIterator<Item = DerivedCost>) -> (Option<f64>, bool) {
    let mut total = 0.0;
    let mut any = false;
    let mut unpriced = false;
    for item in derived {
        if let Some(amount) = item.amount {
            total += amount;
            any = true;
        }
        if item.unpriced {
            unpriced = true;
        }
    }
    (if any { Some(total) } else { None }, unpriced)
}
