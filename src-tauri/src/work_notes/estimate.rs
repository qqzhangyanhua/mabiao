use crate::cost;
use crate::domain::{DerivedCost, EngineProfile, PriceTable, Source, UsageRecord};

use super::input;
use super::prompt::{self, SessionSummary};
use super::EligibleSession;

pub const CHARS_PER_TOKEN: usize = 4;

pub struct Estimate {
    pub calls: i64,
    pub secs: i64,
    pub input_tokens: i64,
    pub cost: Option<f64>,
    pub unpriced: bool,
}

pub fn chars_to_tokens(chars: usize) -> i64 {
    chars.div_ceil(CHARS_PER_TOKEN) as i64
}

pub fn for_remaining(
    sessions: &[EligibleSession],
    extra: &str,
    profile: &EngineProfile,
    prices: &PriceTable,
) -> Estimate {
    if sessions.is_empty() {
        return Estimate {
            calls: 0,
            secs: 0,
            input_tokens: 0,
            cost: None,
            unpriced: false,
        };
    }
    let mut input_tokens = 0i64;
    let mut reduce_summaries = Vec::with_capacity(sessions.len());
    let mut map_calls = 0i64;
    for item in sessions {
        if let Some(summary) = &item.cached_summary {
            reduce_summaries.push(SessionSummary {
                project: input::project_dir_name(&item.session.project),
                title: item.session.title.clone(),
                summary: summary.clone(),
            });
            continue;
        }
        map_calls += 1;
        let compressed = input::compress(&item.session, &item.events);
        input_tokens += chars_to_tokens(prompt::map_prompt(&compressed).chars().count());
        reduce_summaries.push(SessionSummary {
            project: input::project_dir_name(&item.session.project),
            title: item.session.title.clone(),
            summary: item.session.title.clone(),
        });
    }
    input_tokens += chars_to_tokens(
        prompt::reduce_prompt(&reduce_summaries, extra)
            .chars()
            .count(),
    );
    let priced = price_tokens(profile, input_tokens, 0, prices);
    let secs = if map_calls == 0 {
        i64::from(profile.secs_per_call)
    } else {
        estimated_secs(map_calls, profile)
    };
    Estimate {
        calls: map_calls + 1,
        secs,
        input_tokens,
        cost: priced.amount,
        unpriced: priced.unpriced,
    }
}

pub fn estimated_secs(map_calls: i64, profile: &EngineProfile) -> i64 {
    if map_calls <= 0 {
        return 0;
    }
    let concurrency = i64::from(profile.concurrency.max(1));
    let rounds = (map_calls + concurrency - 1) / concurrency;
    (rounds + 1) * i64::from(profile.secs_per_call)
}

fn source_of(profile: &EngineProfile) -> Source {
    match profile.id.as_str() {
        "claude" => Source::Claude,
        "grok" => Source::Grok,
        "cursor-agent" => Source::CursorAgent,
        _ => Source::Codex,
    }
}

pub fn price_tokens(
    profile: &EngineProfile,
    input_tokens: i64,
    output_tokens: i64,
    prices: &PriceTable,
) -> DerivedCost {
    cost::derive_cost(
        &UsageRecord {
            occurred_at: String::new(),
            source: source_of(profile),
            model: profile.model.clone(),
            provider: profile.provider.clone(),
            project: String::new(),
            session_id: String::new(),
            source_file: String::new(),
            input_tokens,
            output_tokens,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: input_tokens + output_tokens,
            native_cost: None,
        },
        prices,
    )
}
