//! 价目查找：精确匹配（model+provider，再 model 且 provider 为空），可选签名模糊匹配。

use std::collections::HashMap;

use crate::{PriceEntry, PriceOrigin, PriceTable};

/// 批量计价时的价目查找缓存。
///
/// `find_price` 是对价表的线性扫描，而 LiteLLM 快照有 1400+ 条；一批记录里不同的
/// (model, provider) 组合却极少——实测 17 万行消耗记录只有 41 个模型。缓存把
/// O(记录数 × 价表长度) 压成 O(不同组合数 × 价表长度)。
///
/// 键直接借用记录里的字段，命中路径上不分配。
pub struct PriceCache<'p, 'r> {
    prices: &'p PriceTable,
    /// `allow_signature` 会改变查找结果，必须进键，否则宽松与严格两条路径会互相污染。
    entries: HashMap<(&'r str, &'r str, bool), Option<&'p PriceEntry>>,
}

impl<'p, 'r> PriceCache<'p, 'r> {
    pub fn new(prices: &'p PriceTable) -> Self {
        Self {
            prices,
            entries: HashMap::new(),
        }
    }

    pub fn resolve(
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

/// 查出该模型适用的价目条目；与 `derive_priced` 的取价顺序一致。
pub fn resolve_entry<'p>(
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

pub fn find_price<'a>(
    model: &str,
    provider: &str,
    prices: &'a PriceTable,
) -> Option<&'a PriceEntry> {
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

/// Cursor 仪表盘模型名常与 LiteLLM 键不一致（`claude-4.6-sonnet` ↔ `claude-sonnet-4-6`，
/// 或带 `-thinking` / `-high` 后缀）。在精确匹配失败后，用家族 + 版本 + 档位签名对齐。
pub fn find_price_by_signature<'a>(model: &str, prices: &'a PriceTable) -> Option<&'a PriceEntry> {
    let want = model_signature(model)?;
    let mut best: Option<(MatchScore, &'a PriceEntry)> = None;
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

fn match_score(want: &ModelSignature, got: &ModelSignature, entry: &PriceEntry) -> MatchScore {
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
