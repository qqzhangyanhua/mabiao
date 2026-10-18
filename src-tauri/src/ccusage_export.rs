//! 与 `ccusage daily --json` 同构的本机消耗记录日报。
//!
//! 按本地日历日切，不叠 Cursor 账号用量。`totalTokens` 对齐 ccusage：
//! input + output + cacheCreation + cacheRead（不含 reasoning）。

use std::collections::BTreeMap;

use serde::Serialize;

#[cfg(test)]
use crate::cost::derive_cost;
#[cfg(test)]
use crate::domain::{PriceTable, UsageRecord};
#[cfg(test)]
use chrono::Local;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcusageDailyJson {
    pub daily: Vec<CcusageDailyRow>,
    pub totals: CcusageDailyTotals,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcusageDailyRow {
    pub date: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: f64,
    pub models_used: Vec<String>,
    pub model_breakdowns: Vec<CcusageModelBreakdown>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcusageModelBreakdown {
    pub model_name: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub cost: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcusageDailyTotals {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_cost: f64,
    pub total_tokens: i64,
}

#[derive(Default)]
struct TokenCost {
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    cost: f64,
}

impl TokenCost {
    #[cfg(test)]
    fn add_record(&mut self, record: &UsageRecord, prices: &PriceTable) {
        self.input_tokens += record.input_tokens;
        self.output_tokens += record.output_tokens;
        self.cache_creation_tokens += record.cache_creation_tokens;
        self.cache_read_tokens += record.cache_read_tokens;
        if let Some(amount) = derive_cost(record, prices).amount {
            self.cost += amount;
        }
    }

    fn add_acc(&mut self, other: &TokenCost) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cost += other.cost;
    }

    fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[cfg(test)]
pub fn from_records(records: &[&UsageRecord], prices: &PriceTable) -> CcusageDailyJson {
    let mut days: BTreeMap<String, BTreeMap<String, TokenCost>> = BTreeMap::new();
    for record in records {
        let Some(date) = local_calendar_day(&record.occurred_at) else {
            continue;
        };
        days.entry(date)
            .or_default()
            .entry(record.model.clone())
            .or_default()
            .add_record(record, prices);
    }
    finish_days(days)
}

pub fn from_model_rows(rows: Vec<ModelDayRow>) -> CcusageDailyJson {
    let mut days: BTreeMap<String, BTreeMap<String, TokenCost>> = BTreeMap::new();
    for row in rows {
        let acc = days
            .entry(row.date)
            .or_default()
            .entry(row.model)
            .or_default();
        acc.input_tokens += row.input_tokens;
        acc.output_tokens += row.output_tokens;
        acc.cache_creation_tokens += row.cache_creation_tokens;
        acc.cache_read_tokens += row.cache_read_tokens;
        acc.cost += row.cost;
    }
    finish_days(days)
}

pub struct ModelDayRow {
    pub date: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub cost: f64,
}

fn finish_days(days: BTreeMap<String, BTreeMap<String, TokenCost>>) -> CcusageDailyJson {
    let mut totals = TokenCost::default();
    let daily = days
        .into_iter()
        .map(|(date, models)| {
            let mut day = TokenCost::default();
            let mut models_used = Vec::new();
            let mut model_breakdowns = Vec::new();
            for (model_name, acc) in models {
                day.add_acc(&acc);
                if !model_name.is_empty() {
                    models_used.push(model_name.clone());
                }
                model_breakdowns.push(CcusageModelBreakdown {
                    model_name,
                    input_tokens: acc.input_tokens,
                    output_tokens: acc.output_tokens,
                    cache_creation_tokens: acc.cache_creation_tokens,
                    cache_read_tokens: acc.cache_read_tokens,
                    cost: acc.cost,
                });
            }
            totals.add_acc(&day);
            CcusageDailyRow {
                date,
                input_tokens: day.input_tokens,
                output_tokens: day.output_tokens,
                cache_creation_tokens: day.cache_creation_tokens,
                cache_read_tokens: day.cache_read_tokens,
                total_tokens: day.total_tokens(),
                total_cost: day.cost,
                models_used,
                model_breakdowns,
            }
        })
        .collect();
    CcusageDailyJson {
        daily,
        totals: CcusageDailyTotals {
            input_tokens: totals.input_tokens,
            output_tokens: totals.output_tokens,
            cache_creation_tokens: totals.cache_creation_tokens,
            cache_read_tokens: totals.cache_read_tokens,
            total_cost: totals.cost,
            total_tokens: totals.total_tokens(),
        },
    }
}

pub fn to_pretty_json(report: &CcusageDailyJson) -> Result<String, String> {
    serde_json::to_string_pretty(report).map_err(|error| error.to_string())
}

#[cfg(test)]
fn local_calendar_day(occurred_at: &str) -> Option<String> {
    Some(
        crate::billing_window::parse_occurred_at(occurred_at)?
            .with_timezone(&Local)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Source;
    use crate::test_support::rec;

    #[test]
    fn empty_records_yield_empty_daily_and_zero_totals() {
        let report = from_records(&[], &PriceTable::default());
        assert!(report.daily.is_empty());
        assert_eq!(report.totals.total_tokens, 0);
        assert_eq!(report.totals.total_cost, 0.0);
        let json = to_pretty_json(&report).unwrap();
        assert!(json.contains("\"daily\": []"));
        assert!(json.contains("\"totalTokens\": 0"));
    }

    #[test]
    fn total_tokens_exclude_reasoning_and_keep_cache() {
        let mut record = rec(
            "2026-08-01T12:00:00Z",
            Source::Claude,
            "claude-sonnet-4",
            "anthropic",
            "/proj/a",
            "s1",
            0,
        );
        record.input_tokens = 10;
        record.output_tokens = 4;
        record.cache_creation_tokens = 2;
        record.cache_read_tokens = 3;
        record.reasoning_tokens = 7;
        record.total_tokens = 26;
        record.native_cost = Some(0.11);

        let report = from_records(&[&record], &PriceTable::default());
        assert_eq!(report.daily.len(), 1);
        assert_eq!(report.daily[0].total_tokens, 19);
        assert!((report.daily[0].total_cost - 0.11).abs() < 1e-12);
        assert_eq!(report.daily[0].models_used, vec!["claude-sonnet-4"]);
        let json = to_pretty_json(&report).unwrap();
        assert!(json.contains("\"inputTokens\""));
        assert!(json.contains("\"cacheCreationTokens\""));
        assert!(json.contains("\"modelsUsed\""));
        assert!(json.contains("\"modelBreakdowns\""));
        assert!(!json.contains("reasoningTokens"));
    }
}
