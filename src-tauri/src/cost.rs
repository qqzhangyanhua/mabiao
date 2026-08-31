use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::domain::{
    CostSource, CursorUsageEvent, DerivedCost, PriceEntry, PriceOrigin, PriceTable,
    UnpricedGroupDto, UnpricedReason, UsageRecord,
};

struct PricedTokens<'a> {
    model: &'a str,
    provider: &'a str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    native_cost: Option<f64>,
}

/// 批量计价时的价目查找缓存。
///
/// `find_price` 是对价表的线性扫描，而 LiteLLM 快照有 1400+ 条；一批记录里不同的
/// (model, provider) 组合却极少——实测 17 万行消耗记录只有 41 个模型。缓存把
/// O(记录数 × 价表长度) 压成 O(不同组合数 × 价表长度)。
///
/// 键直接借用记录里的字段，命中路径上不分配。
struct PriceCache<'p, 'r> {
    prices: &'p PriceTable,
    /// `allow_signature` 会改变查找结果，必须进键，否则宽松与严格两条路径会互相污染。
    entries: HashMap<(&'r str, &'r str, bool), Option<&'p PriceEntry>>,
}

impl<'p, 'r> PriceCache<'p, 'r> {
    fn new(prices: &'p PriceTable) -> Self {
        Self {
            prices,
            entries: HashMap::new(),
        }
    }

    fn resolve(
        &mut self,
        model: &'r str,
        provider: &'r str,
        allow_signature_match: bool,
    ) -> Option<&'p PriceEntry> {
        let prices = self.prices;
        *self
            .entries
            .entry((model, provider, allow_signature_match))
            .or_insert_with(|| resolve_entry(model, provider, prices, allow_signature_match))
    }
}

/// 查出该模型适用的价目条目；与 `derive_priced_lookup` 的取价顺序一致。
fn resolve_entry<'p>(
    model: &str,
    provider: &str,
    prices: &'p PriceTable,
    allow_signature_match: bool,
) -> Option<&'p PriceEntry> {
    find_price(model, provider, prices).or_else(|| {
        if allow_signature_match {
            find_price_by_signature(model, prices)
        } else {
            None
        }
    })
}

/// 把查好的价目条目套到 token 数上。与查价分开，好让批量路径只缓存前者。
fn apply_entry(usage: &PricedTokens<'_>, entry: Option<&PriceEntry>) -> DerivedCost {
    if let Some(amount) = usage.native_cost {
        return DerivedCost {
            amount: Some(amount),
            unpriced: false,
            source_native: true,
            cost_source: CostSource::Native,
        };
    }
    let Some(entry) = entry else {
        return DerivedCost {
            amount: None,
            unpriced: true,
            source_native: false,
            cost_source: CostSource::None,
        };
    };
    let amount = (usage.input_tokens as f64) * entry.input
        + (usage.output_tokens as f64) * entry.output
        + (usage.cache_read_tokens as f64) * entry.cache_read
        + (usage.cache_creation_tokens as f64) * entry.cache_creation;
    DerivedCost {
        amount: Some(amount),
        unpriced: false,
        source_native: false,
        cost_source: match entry.origin {
            PriceOrigin::Snapshot => CostSource::Snapshot,
            PriceOrigin::User => CostSource::User,
        },
    }
}

pub fn derive_cost(record: &UsageRecord, prices: &PriceTable) -> DerivedCost {
    derive_priced(
        PricedTokens {
            model: &record.model,
            provider: &record.provider,
            input_tokens: record.input_tokens,
            output_tokens: record.output_tokens,
            cache_read_tokens: record.cache_read_tokens,
            cache_creation_tokens: record.cache_creation_tokens,
            native_cost: record.native_cost,
        },
        prices,
    )
}

/// 按模型计价：native_cost 优先，其次用户价目，再次 LiteLLM 快照（provider 为空的兜底）。
/// 单条路径；批量计价走 `sum_costs` / `sum_cursor_event_costs`，那两条带 `PriceCache`。
fn derive_priced(usage: PricedTokens<'_>, prices: &PriceTable) -> DerivedCost {
    derive_priced_lookup(usage, prices, false)
}

fn derive_priced_lookup(
    usage: PricedTokens<'_>,
    prices: &PriceTable,
    allow_signature_match: bool,
) -> DerivedCost {
    if usage.native_cost.is_some() {
        return apply_entry(&usage, None);
    }
    let entry = resolve_entry(usage.model, usage.provider, prices, allow_signature_match);
    apply_entry(&usage, entry)
}

fn find_price<'a>(
    model: &str,
    provider: &str,
    prices: &'a PriceTable,
) -> Option<&'a crate::domain::PriceEntry> {
    prices
        .prices
        .iter()
        .find(|p| {
            model_matches(&p.model, model)
                && p.provider
                    .as_deref()
                    .map(|prov| provider_matches(prov, provider))
                    .unwrap_or(false)
        })
        .or_else(|| {
            prices
                .prices
                .iter()
                .find(|p| model_matches(&p.model, model) && p.provider.is_none())
        })
}

/// 精确匹配优先；大小写不一致（如来源上报 `"GPT-4o"`、用户价目表填 `"gpt-4o"`）时仍按同一模型兜底。
fn model_matches(entry_model: &str, record_model: &str) -> bool {
    entry_model == record_model || entry_model.eq_ignore_ascii_case(record_model)
}

fn provider_matches(entry_provider: &str, record_provider: &str) -> bool {
    entry_provider == record_provider || entry_provider.eq_ignore_ascii_case(record_provider)
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

/// Cursor 仪表盘模型名常与 LiteLLM 键不一致（`claude-4.6-sonnet` ↔ `claude-sonnet-4-6`，
/// 或带 `-thinking` / `-high` 后缀）。在精确匹配失败后，用家族 + 版本 + 档位签名对齐。
fn find_price_by_signature<'a>(
    model: &str,
    prices: &'a PriceTable,
) -> Option<&'a crate::domain::PriceEntry> {
    let want = model_signature(model)?;
    let mut best: Option<(MatchScore, &'a crate::domain::PriceEntry)> = None;
    for entry in &prices.prices {
        if entry.provider.is_some() {
            continue;
        }
        let Some(got) = model_signature(&entry.model) else {
            continue;
        };
        if !signatures_compatible(&want, &got) {
            continue;
        }
        let score = match_score(&want, &got, entry);
        if best.as_ref().is_none_or(|(current, _)| score > *current) {
            best = Some((score, entry));
        }
    }
    best.map(|(_, entry)| entry)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelSignature {
    family: String,
    version: String,
    flavor: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MatchScore {
    flavor_equal: bool,
    user_origin: bool,
    canonical: bool,
    name_shortness: i32,
}

fn signatures_compatible(want: &ModelSignature, got: &ModelSignature) -> bool {
    want.family == got.family
        && want.version == got.version
        && !want.family.is_empty()
        && !want.version.is_empty()
        && got.flavor.iter().all(|token| want.flavor.contains(token))
}

fn match_score(
    want: &ModelSignature,
    got: &ModelSignature,
    entry: &crate::domain::PriceEntry,
) -> MatchScore {
    MatchScore {
        flavor_equal: want.flavor == got.flavor,
        user_origin: matches!(entry.origin, PriceOrigin::User),
        canonical: is_canonical_price_name(&entry.model),
        name_shortness: -(entry.model.len() as i32),
    }
}

fn is_canonical_price_name(model: &str) -> bool {
    !model.contains('/')
        && !model.contains('@')
        && !model.contains(':')
        && !has_date_token(model)
        && !model.contains("anthropic")
        && !model.contains("databricks")
}

fn has_date_token(model: &str) -> bool {
    model
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| {
            token.len() == 8
                && token.chars().all(|c| c.is_ascii_digit())
                && (token.starts_with("19") || token.starts_with("20"))
        })
}

fn model_signature(model: &str) -> Option<ModelSignature> {
    let tokens = signature_tokens(model);
    if tokens.is_empty() {
        return None;
    }
    let family = tokens
        .iter()
        .find(|token| is_family_token(token))
        .cloned()
        .or_else(|| tokens.first().cloned())?;
    let version = tokens
        .iter()
        .find(|token| is_version_token(token) && token.as_str() != family)
        .cloned()
        .unwrap_or_default();
    if family.is_empty() || version.is_empty() {
        return None;
    }
    let mut flavor: Vec<String> = tokens
        .into_iter()
        .filter(|token| token != &family && token != &version && !is_noise_token(token))
        .collect();
    flavor.sort();
    flavor.dedup();
    Some(ModelSignature {
        family,
        version,
        flavor,
    })
}

fn signature_tokens(model: &str) -> Vec<String> {
    let normalized = normalize_model_separators(model);
    let stripped = strip_date_suffixes(&normalized);
    let raw: Vec<String> = stripped
        .split('-')
        .filter(|token| !token.is_empty() && !is_noise_token(token))
        .map(ToOwned::to_owned)
        .collect();
    let without_affix = strip_known_affixes(raw);
    merge_version_tokens(without_affix)
}

fn normalize_model_separators(model: &str) -> String {
    let chars: Vec<char> = model.chars().collect();
    let mut out = String::with_capacity(chars.len());
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            continue;
        }
        let keep_dot = ch == '.'
            && index > 0
            && chars[index - 1].is_ascii_digit()
            && index + 1 < chars.len()
            && chars[index + 1].is_ascii_digit();
        if keep_dot {
            out.push('.');
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn strip_date_suffixes(model: &str) -> String {
    let mut tokens: Vec<&str> = model.split('-').filter(|token| !token.is_empty()).collect();
    tokens.retain(|token| !is_date_like(token));
    tokens.join("-")
}

fn is_date_like(token: &str) -> bool {
    if token.starts_with('v') && token.len() > 1 && token[1..].chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    let digits = token.chars().all(|c| c.is_ascii_digit());
    if !digits {
        return false;
    }
    matches!(token.len(), 8) && (token.starts_with("19") || token.starts_with("20"))
        || matches!(token.len(), 4) && (token.starts_with("19") || token.starts_with("20"))
}

fn strip_known_affixes(mut tokens: Vec<String>) -> Vec<String> {
    const PREFIXES: &[&str] = &[
        "anthropic",
        "openai",
        "google",
        "bedrock",
        "vertex",
        "vertexai",
        "databricks",
        "azure",
        "aws",
        "together",
        "fireworks",
        "groq",
        "openrouter",
        "apac",
        "eu",
        "us",
        "au",
        "jp",
        "global",
        "gov",
    ];
    while tokens
        .first()
        .is_some_and(|token| PREFIXES.contains(&token.as_str()))
    {
        tokens.remove(0);
    }
    tokens
}

fn merge_version_tokens(tokens: Vec<String>) -> Vec<String> {
    let mut merged = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        let current = &tokens[index];
        if is_plain_version_part(current)
            && index + 1 < tokens.len()
            && is_plain_version_part(&tokens[index + 1])
            && tokens[index + 1].len() == 1
        {
            merged.push(format!("{current}.{}", tokens[index + 1]));
            index += 2;
            continue;
        }
        merged.push(current.clone());
        index += 1;
    }
    merged
}

fn is_plain_version_part(token: &str) -> bool {
    !token.is_empty() && token.len() <= 2 && token.chars().all(|c| c.is_ascii_digit())
}

fn is_family_token(token: &str) -> bool {
    const FAMILIES: &[&str] = &[
        "claude",
        "gpt",
        "gemini",
        "gemma",
        "grok",
        "kimi",
        "deepseek",
        "qwen",
        "llama",
        "mistral",
        "codestral",
        "composer",
        "glm",
        "command",
        "sonar",
        "dbrx",
    ];
    FAMILIES.contains(&token)
        || (token.len() == 2 && token.starts_with('o') && token.as_bytes()[1].is_ascii_digit())
}

fn is_version_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    token.chars().all(|c| c.is_ascii_digit() || c == '.')
        && token.chars().any(|c| c.is_ascii_digit())
        && !token.starts_with('.')
        && !token.ends_with('.')
}

fn is_noise_token(token: &str) -> bool {
    const NOISE: &[&str] = &[
        "thinking",
        "high",
        "low",
        "medium",
        "fast",
        "preview",
        "latest",
        "default",
        "turbo",
        "instruct",
        "chat",
        "experimental",
        "1m",
        "200k",
        "hf",
    ];
    NOISE.contains(&token) || is_date_like(token)
}

pub fn sum_costs(records: &[&UsageRecord], prices: &PriceTable) -> (Option<f64>, bool) {
    let mut cache = PriceCache::new(prices);
    accumulate_costs(records.iter().map(|record| {
        let usage = PricedTokens {
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
        let usage = PricedTokens {
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
