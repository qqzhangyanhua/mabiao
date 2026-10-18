//! 把价目套到 token 数上，得出费用与来源。

use crate::lookup::{resolve_entry, PriceCache};
use crate::{CostSource, DerivedCost, PriceEntry, PriceOrigin, PriceTable};

pub struct PricedUsage<'a> {
    pub model: &'a str,
    pub provider: &'a str,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    /// 1h 缓存创建；计入 `cache_creation_tokens`（KPI 仍是 5m+1h），计价时单独乘 1h 单价。
    pub cache_creation_1h_tokens: i64,
    pub native_cost: Option<f64>,
}

/// LiteLLM `*_above_200k_tokens` 的默认边际阈值。
pub const LONG_CONTEXT_THRESHOLD: i64 = 200_000;
/// ccusage：未配置 1h 单价时用 `2 × input`。
pub const CACHE_CREATE_1H_MULTIPLIER: f64 = 2.0;

struct CostParts {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
}

impl CostParts {
    fn total(&self) -> f64 {
        self.input + self.output + self.cache_read + self.cache_creation
    }
}

fn tiered(tokens: i64, base: f64, above: f64) -> f64 {
    if above > 0.0 && tokens > LONG_CONTEXT_THRESHOLD {
        (LONG_CONTEXT_THRESHOLD as f64) * base + (tokens - LONG_CONTEXT_THRESHOLD) as f64 * above
    } else {
        tokens as f64 * base
    }
}

fn cache_creation_1h_rate(entry: &PriceEntry) -> f64 {
    if entry.cache_creation_1h > 0.0 {
        entry.cache_creation_1h
    } else {
        entry.input * CACHE_CREATE_1H_MULTIPLIER
    }
}

fn cache_creation_1h_above(entry: &PriceEntry) -> f64 {
    if entry.cache_creation_1h_above_200k > 0.0 {
        entry.cache_creation_1h_above_200k
    } else if entry.input_above_200k > 0.0 {
        entry.input_above_200k * CACHE_CREATE_1H_MULTIPLIER
    } else {
        0.0
    }
}

fn priced_parts(usage: &PricedUsage<'_>, entry: &PriceEntry) -> CostParts {
    let one_h = usage
        .cache_creation_1h_tokens
        .max(0)
        .min(usage.cache_creation_tokens.max(0));
    let five_m = usage.cache_creation_tokens.max(0) - one_h;
    CostParts {
        input: tiered(usage.input_tokens, entry.input, entry.input_above_200k),
        output: tiered(usage.output_tokens, entry.output, entry.output_above_200k),
        cache_read: tiered(
            usage.cache_read_tokens,
            entry.cache_read,
            entry.cache_read_above_200k,
        ),
        cache_creation: tiered(
            five_m,
            entry.cache_creation,
            entry.cache_creation_above_200k,
        ) + tiered(
            one_h,
            cache_creation_1h_rate(entry),
            cache_creation_1h_above(entry),
        ),
    }
}

pub struct CostAttribution {
    pub derived: DerivedCost,
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_creation: Option<f64>,
}

/// 把查好的价目条目套到 token 数上。与查价分开，好让批量路径只缓存前者。
///
/// 来源自带费用是整笔，四档口径为 `None`；未命中价目同样不拆。推理含在输出里，不单独乘单价。
pub fn apply_entry(usage: &PricedUsage<'_>, entry: Option<&PriceEntry>) -> DerivedCost {
    apply_entry_attribution(usage, entry).derived
}

pub fn apply_entry_attribution(
    usage: &PricedUsage<'_>,
    entry: Option<&PriceEntry>,
) -> CostAttribution {
    if let Some(amount) = usage.native_cost {
        return CostAttribution {
            derived: DerivedCost {
                amount: Some(amount),
                unpriced: false,
                source_native: true,
                cost_source: CostSource::Native,
            },
            input: None,
            output: None,
            cache_read: None,
            cache_creation: None,
        };
    }
    let Some(entry) = entry else {
        return CostAttribution {
            derived: DerivedCost {
                amount: None,
                unpriced: true,
                source_native: false,
                cost_source: CostSource::None,
            },
            input: None,
            output: None,
            cache_read: None,
            cache_creation: None,
        };
    };
    let parts = priced_parts(usage, entry);
    CostAttribution {
        derived: DerivedCost {
            amount: Some(parts.total()),
            unpriced: false,
            source_native: false,
            cost_source: match entry.origin {
                PriceOrigin::Snapshot => CostSource::Snapshot,
                PriceOrigin::User => CostSource::User,
            },
        },
        input: Some(parts.input),
        output: Some(parts.output),
        cache_read: Some(parts.cache_read),
        cache_creation: Some(parts.cache_creation),
    }
}

/// 费用的定价来源，给推送费用快照用。
///
/// 与 [`CostSource`] 不同：那个说「钱来自谁」（自带 / 用户 / 快照），
/// 这个说「价目是怎么命中的」（精确 / 兜底）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingBasis {
    /// 来源自带 `native_cost`。
    Native,
    /// 价目条目带 provider，且 model+provider 都对上。
    Exact,
    /// provider 为空的条目：按 model 兜底，含 LiteLLM 快照与签名模糊匹配。
    Fallback,
    Unpriced,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PricedCost {
    pub derived: DerivedCost,
    pub basis: PricingBasis,
}

/// 单条计价：费用加定价来源。`allow_signature_match` 只给 Cursor 账号事件开。
pub fn price_usage(
    usage: PricedUsage<'_>,
    prices: &PriceTable,
    allow_signature_match: bool,
) -> PricedCost {
    if usage.native_cost.is_some() {
        return priced_with_entry(&usage, None);
    }
    let entry = resolve_entry(usage.model, usage.provider, prices, allow_signature_match);
    priced_with_entry(&usage, entry)
}

/// 批量版 [`price_usage`]：同一 (model, provider) 只查一次价目，结果与单条路径逐条一致。
pub fn price_usage_cached<'p, 'r>(
    cache: &mut PriceCache<'p, 'r>,
    usage: PricedUsage<'r>,
    allow_signature_match: bool,
) -> PricedCost {
    if usage.native_cost.is_some() {
        return priced_with_entry(&usage, None);
    }
    let entry = cache.resolve(usage.model, usage.provider, allow_signature_match);
    priced_with_entry(&usage, entry)
}

fn priced_with_entry(usage: &PricedUsage<'_>, entry: Option<&PriceEntry>) -> PricedCost {
    let basis = if usage.native_cost.is_some() {
        PricingBasis::Native
    } else {
        match entry {
            None => PricingBasis::Unpriced,
            Some(entry) if entry.provider.is_some() => PricingBasis::Exact,
            Some(_) => PricingBasis::Fallback,
        }
    };
    PricedCost {
        derived: apply_entry(usage, entry),
        basis,
    }
}

/// 按模型计价：native_cost 优先，其次用户价目，再次 LiteLLM 快照（provider 为空的兜底）。
/// 单条路径；批量计价带 `PriceCache`。
pub fn derive_priced(
    usage: PricedUsage<'_>,
    prices: &PriceTable,
    allow_signature_match: bool,
) -> DerivedCost {
    price_usage(usage, prices, allow_signature_match).derived
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(model: &str, provider: Option<&str>, input: f64, origin: PriceOrigin) -> PriceEntry {
        PriceEntry {
            model: model.into(),
            provider: provider.map(str::to_string),
            input,
            output: 0.0,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin,
            ..Default::default()
        }
    }

    fn usage<'a>(model: &'a str, provider: &'a str, native: Option<f64>) -> PricedUsage<'a> {
        PricedUsage {
            model,
            provider,
            input_tokens: 10,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            cache_creation_1h_tokens: 0,
            native_cost: native,
        }
    }

    #[test]
    fn one_hour_cache_creation_uses_double_input_when_unpriced() {
        let entry = PriceEntry {
            model: "claude".into(),
            provider: None,
            input: 3e-6,
            output: 0.0,
            cache_read: 0.0,
            cache_creation: 3.75e-6,
            origin: PriceOrigin::Snapshot,
            ..Default::default()
        };
        let cost = apply_entry(
            &PricedUsage {
                model: "claude",
                provider: "",
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 100,
                cache_creation_1h_tokens: 40,
                native_cost: None,
            },
            Some(&entry),
        );
        // 60 * 3.75e-6 + 40 * (2 * 3e-6)
        assert!((cost.amount.unwrap() - 0.000465).abs() < 1e-12);
    }

    #[test]
    fn above_200k_is_marginal_per_bucket() {
        let entry = PriceEntry {
            model: "claude".into(),
            provider: None,
            input: 3e-6,
            output: 15e-6,
            cache_read: 0.3e-6,
            cache_creation: 3.75e-6,
            input_above_200k: 6e-6,
            output_above_200k: 22.5e-6,
            origin: PriceOrigin::Snapshot,
            ..Default::default()
        };
        let cost = apply_entry(
            &PricedUsage {
                model: "claude",
                provider: "",
                input_tokens: 250_000,
                output_tokens: 10,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                cache_creation_1h_tokens: 0,
                native_cost: None,
            },
            Some(&entry),
        );
        let expected = 200_000.0 * 3e-6 + 50_000.0 * 6e-6 + 10.0 * 15e-6;
        assert!((cost.amount.unwrap() - expected).abs() < 1e-12);
    }

    fn table(prices: Vec<PriceEntry>) -> PriceTable {
        PriceTable { prices }
    }

    #[test]
    fn native_cost_wins_over_any_price_entry() {
        let prices = table(vec![entry("m", Some("p"), 1.0, PriceOrigin::User)]);
        let priced = price_usage(usage("m", "p", Some(0.5)), &prices, false);
        assert_eq!(priced.basis, PricingBasis::Native);
        assert_eq!(priced.derived.amount, Some(0.5));
        assert_eq!(priced.derived.cost_source, CostSource::Native);
    }

    #[test]
    fn provider_entry_is_exact_and_beats_model_only_entry() {
        let prices = table(vec![
            entry("m", None, 9.0, PriceOrigin::Snapshot),
            entry("m", Some("p"), 1.0, PriceOrigin::User),
        ]);
        let priced = price_usage(usage("m", "p", None), &prices, false);
        assert_eq!(priced.basis, PricingBasis::Exact);
        assert_eq!(priced.derived.amount, Some(10.0));
        assert_eq!(priced.derived.cost_source, CostSource::User);
    }

    #[test]
    fn model_only_entry_is_fallback_for_user_and_snapshot_alike() {
        for origin in [PriceOrigin::User, PriceOrigin::Snapshot] {
            let prices = table(vec![entry("m", None, 2.0, origin)]);
            let priced = price_usage(usage("m", "other", None), &prices, false);
            assert_eq!(priced.basis, PricingBasis::Fallback);
            assert_eq!(priced.derived.amount, Some(20.0));
        }
    }

    #[test]
    fn provider_entry_for_a_different_provider_does_not_match() {
        let prices = table(vec![entry("m", Some("p"), 1.0, PriceOrigin::User)]);
        let priced = price_usage(usage("m", "q", None), &prices, false);
        assert_eq!(priced.basis, PricingBasis::Unpriced);
        assert!(priced.derived.unpriced);
        assert_eq!(priced.derived.amount, None);
    }

    #[test]
    fn model_match_ignores_ascii_case() {
        let prices = table(vec![entry("gpt-4o", None, 1.0, PriceOrigin::User)]);
        let priced = price_usage(usage("GPT-4o", "", None), &prices, false);
        assert_eq!(priced.basis, PricingBasis::Fallback);
    }

    #[test]
    fn signature_match_is_fallback_and_only_when_allowed() {
        let prices = table(vec![entry(
            "claude-sonnet-4-6",
            None,
            1.0,
            PriceOrigin::Snapshot,
        )]);
        let strict = price_usage(usage("claude-4.6-sonnet", "", None), &prices, false);
        assert_eq!(strict.basis, PricingBasis::Unpriced);
        let loose = price_usage(usage("claude-4.6-sonnet", "", None), &prices, true);
        assert_eq!(loose.basis, PricingBasis::Fallback);
        assert_eq!(loose.derived.cost_source, CostSource::Snapshot);
    }

    #[test]
    fn cached_pricing_agrees_with_single_pricing_for_every_basis() {
        let prices = table(vec![
            entry("exact", Some("p"), 1.0, PriceOrigin::User),
            entry("fallback", None, 2.0, PriceOrigin::Snapshot),
        ]);
        let mut cache = PriceCache::new(&prices);
        for (model, provider, native) in [
            ("exact", "p", None),
            ("fallback", "x", None),
            ("missing", "p", None),
            ("exact", "p", Some(0.25)),
            // 同一 (model, provider) 第二次走缓存
            ("fallback", "x", None),
        ] {
            let cached = price_usage_cached(&mut cache, usage(model, provider, native), false);
            let single = price_usage(usage(model, provider, native), &prices, false);
            assert_eq!(cached, single, "{model}/{provider}/{native:?}");
        }
    }

    #[test]
    fn derive_priced_agrees_with_price_usage() {
        let prices = table(vec![entry("m", None, 3.0, PriceOrigin::User)]);
        assert_eq!(
            derive_priced(usage("m", "", None), &prices, false),
            price_usage(usage("m", "", None), &prices, false).derived
        );
    }
}
