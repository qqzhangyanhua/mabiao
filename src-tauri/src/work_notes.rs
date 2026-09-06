//! 工作纪要：按区间读对话正文，经本机 CLI 总结成结构化条目（ADR 0021）。
//! 主入口 `build` 对齐 `report::build`：注入连接、注入 now、注入 runner。

mod engines;
mod input;
mod parse;
mod period;
mod prompt;

use chrono::{DateTime, Local};
use rusqlite::Connection;
use serde::Deserialize;
use std::path::Path;

use crate::conversation;
use crate::domain::{
    ConversationEvent, ConversationQuery, ConversationSessionRow, Filter, PriceTable, WorkNotesDto,
    WorkNotesEntry, WorkNotesRange,
};
use crate::query;

#[cfg(test)]
pub use engines::{codex_command, ensure_work_dir, write_schemas, SchemaKind, ScriptedRunner};
pub use engines::{EngineRunner, ProcessRunner};

#[derive(Debug, Deserialize)]
struct MapOut {
    summary: String,
}

#[derive(Debug, Deserialize)]
struct ReduceOut {
    headline: String,
    entries: Vec<WorkNotesEntry>,
    closing: String,
}

/// 工作纪要模块的单一入口。`now` 与 `runner` 由调用方注入。
pub fn build(
    conn: &Connection,
    prices: &PriceTable,
    range: WorkNotesRange,
    now: DateTime<Local>,
    runner: &dyn EngineRunner,
    app_data_dir: &Path,
) -> Result<WorkNotesDto, String> {
    let resolved = period::resolve(&range, now)?;
    let filter = period::usage_filter(&resolved);
    let numbers = hard_numbers(conn, prices, &filter)?;
    let sessions = load_sessions(conn, prices, &resolved.from, &resolved.to)?;

    let mut skipped_sparse = 0i64;
    let mut eligible: Vec<(ConversationSessionRow, Vec<ConversationEvent>)> = Vec::new();
    for session in sessions {
        let events = conversation::indexed_events(conn, &session.source, &session.session_id)?;
        if input::is_sparse(events.len(), session.total_tokens) {
            skipped_sparse += 1;
            continue;
        }
        eligible.push((session, events));
    }

    if eligible.is_empty() {
        return Ok(empty_dto(&resolved, skipped_sparse, numbers));
    }

    let work_dir = engines::ensure_work_dir(app_data_dir)?;
    engines::write_schemas(&work_dir)?;

    let mut summaries = Vec::new();
    for (session, events) in &eligible {
        let compressed = input::compress(session, events);
        let command = engines::codex_command(
            &work_dir,
            engines::SchemaKind::Map,
            prompt::map_prompt(&compressed),
        )?;
        let summary = match parse::run::<MapOut>(runner, &command) {
            parse::ParseOutcome::Parsed(value) => value.summary,
            parse::ParseOutcome::Plain(raw) => input::take_chars(raw.trim(), 200),
            parse::ParseOutcome::Failed(error) => return Err(error),
        };
        summaries.push(prompt::SessionSummary {
            project: input::project_dir_name(&session.project),
            title: session.title.clone(),
            summary,
        });
    }

    let command = engines::codex_command(
        &work_dir,
        engines::SchemaKind::Reduce,
        prompt::reduce_prompt(&summaries),
    )?;
    let (headline, entries, closing) = match parse::run_with(runner, &command, parse_reduce) {
        parse::ParseOutcome::Parsed(value) => (value.headline, value.entries, value.closing),
        parse::ParseOutcome::Plain(raw) => parse::degrade_reduce(&raw),
        parse::ParseOutcome::Failed(error) => return Err(error),
    };

    Ok(WorkNotesDto {
        range_kind: resolved.kind,
        start_date: resolved.start_date,
        end_date: resolved.end_date,
        has_data: true,
        skipped_sparse,
        session_count: numbers.session_count,
        project_count: numbers.project_count,
        active_days: numbers.active_days,
        total_tokens: numbers.total_tokens,
        headline,
        entries,
        closing,
    })
}

fn parse_reduce(raw: &str) -> Option<ReduceOut> {
    let value: ReduceOut = parse::parse_structured(raw)?;
    (3..=6).contains(&value.entries.len()).then_some(value)
}

struct HardNumbers {
    session_count: i64,
    project_count: i64,
    active_days: i64,
    total_tokens: i64,
}

fn hard_numbers(
    conn: &Connection,
    prices: &PriceTable,
    filter: &Filter,
) -> Result<HardNumbers, String> {
    let overview = query::overview(conn, filter, prices)?;
    let project_count = query::breakdown(conn, filter, prices, "project")?
        .into_iter()
        .filter(|row| row.total_tokens > 0)
        .count() as i64;
    let active_days = query::tokens_by_local_day(conn, filter)?
        .into_iter()
        .filter(|(_, tokens)| *tokens > 0)
        .count() as i64;
    Ok(HardNumbers {
        session_count: overview.session_count,
        project_count,
        active_days,
        total_tokens: overview.total_tokens,
    })
}

fn load_sessions(
    conn: &Connection,
    prices: &PriceTable,
    from: &str,
    to: &str,
) -> Result<Vec<ConversationSessionRow>, String> {
    let mut page = 1u32;
    let mut rows = Vec::new();
    loop {
        let query = ConversationQuery {
            from: Some(from.to_string()),
            to: Some(to.to_string()),
            page: Some(page),
            page_size: Some(200),
            ..ConversationQuery::default()
        };
        let page_rows = conversation::sessions_page_with_prices(conn, &query, prices)?;
        let total = page_rows.total;
        let batch = page_rows.rows.len();
        rows.extend(page_rows.rows);
        if rows.len() >= total as usize || batch == 0 {
            break;
        }
        page += 1;
    }
    Ok(rows)
}

fn empty_dto(
    range: &period::ResolvedRange,
    skipped_sparse: i64,
    numbers: HardNumbers,
) -> WorkNotesDto {
    WorkNotesDto {
        range_kind: range.kind,
        start_date: range.start_date.clone(),
        end_date: range.end_date.clone(),
        has_data: false,
        skipped_sparse,
        session_count: numbers.session_count,
        project_count: numbers.project_count,
        active_days: numbers.active_days,
        total_tokens: numbers.total_tokens,
        headline: String::new(),
        entries: Vec::new(),
        closing: String::new(),
    }
}
