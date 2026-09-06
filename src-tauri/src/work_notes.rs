//! 工作纪要：按区间读对话正文，经本机 CLI 总结成结构化条目（ADR 0021）。
//! 主入口 `build` 对齐 `report::build`：注入连接、注入 now、注入 runner。

mod engines;
mod estimate;
mod input;
mod job;
mod orchestrate;
mod parse;
mod period;
mod prompt;
mod scale;
mod usage;

use chrono::{DateTime, Local};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use crate::conversation;
use crate::domain::{
    ConversationEvent, ConversationQuery, ConversationSessionRow, EngineCommand, Filter,
    PriceTable, WorkNotesDto, WorkNotesPreviewDto, WorkNotesRange,
};
use crate::query;

use engines::EngineError;

#[cfg(test)]
pub use engines::{
    claude_command, codex_command, cursor_agent_command, detect_with, grok_command, SchemaKind,
    ScriptedRunner,
};
pub use engines::{
    codex_profile, detect_engines, ensure_work_dir, write_schemas, EngineRunner, ProcessRunner,
};
pub use job::WorkNotesJob;

/// 选完区间立刻返回会话数、闸门与成本预估，不调引擎。
pub fn preview(
    conn: &Connection,
    prices: &PriceTable,
    range: WorkNotesRange,
    now: DateTime<Local>,
    engine_id: Option<&str>,
    model: Option<&str>,
) -> Result<WorkNotesPreviewDto, String> {
    let prepared = prepare(conn, prices, range, now, false, true)?;
    let mut profile = match engine_id {
        Some(id) => engines::profile(id).unwrap_or_else(|_| engines::codex_profile()),
        None => engines::codex_profile(),
    };
    if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
        profile.model = model.to_string();
    }
    let estimate = estimate::for_sessions(&prepared.eligible, &profile, prices);
    let (gate, message) = scale::assess(prepared.eligible.len() as i64);
    Ok(WorkNotesPreviewDto {
        range_kind: prepared.resolved.kind,
        start_date: prepared.resolved.start_date,
        end_date: prepared.resolved.end_date,
        session_count: prepared.eligible.len() as i64,
        skipped_sparse: prepared.skipped_sparse,
        gate,
        message,
        estimated_calls: estimate.calls,
        estimated_secs: estimate.secs,
        estimated_input_tokens: estimate.input_tokens,
        estimated_cost: estimate.cost,
        estimated_unpriced: estimate.unpriced,
    })
}

pub fn require_engine(engine_id: &str) -> Result<(), String> {
    engines::require(engine_id)
}

/// 工作纪要模块的单一入口。`now`、`runner`、`job` 由调用方注入。
#[allow(clippy::too_many_arguments)]
pub fn build(
    conn: &Connection,
    prices: &PriceTable,
    range: WorkNotesRange,
    now: DateTime<Local>,
    runner: &dyn EngineRunner,
    app_data_dir: &Path,
    engine_id: &str,
    model: Option<&str>,
    confirmed: bool,
    job: &WorkNotesJob,
) -> Result<WorkNotesDto, String> {
    engines::require(engine_id)?;
    let prepared = prepare(conn, prices, range, now, confirmed, false)?;
    let recorder = RecordingRunner::new(runner, engine_id);
    let result = run(
        prepared,
        prices,
        &recorder,
        app_data_dir,
        engine_id,
        model,
        job,
    );
    flush_generated(conn, &recorder.take())?;
    result
}

pub fn prepare(
    conn: &Connection,
    prices: &PriceTable,
    range: WorkNotesRange,
    now: DateTime<Local>,
    confirmed: bool,
    skip_gate: bool,
) -> Result<PreparedWorkNotes, String> {
    let resolved = period::resolve(&range, now)?;
    let filter = period::usage_filter(&resolved);
    let numbers = hard_numbers(conn, prices, &filter)?;
    let sessions = load_sessions(conn, prices, &resolved.from, &resolved.to)?;
    let (skipped_sparse, eligible_sessions) = classify_sessions(conn, sessions)?;
    if !skip_gate {
        scale::enforce(eligible_sessions.len() as i64, confirmed)?;
    }
    let mut eligible = Vec::new();
    for session in eligible_sessions {
        let events = conversation::indexed_events(conn, &session.source, &session.session_id)?;
        eligible.push((session, events));
    }
    Ok(PreparedWorkNotes {
        resolved,
        skipped_sparse,
        numbers,
        eligible,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    prepared: PreparedWorkNotes,
    prices: &PriceTable,
    runner: &dyn EngineRunner,
    app_data_dir: &Path,
    engine_id: &str,
    model: Option<&str>,
    job: &WorkNotesJob,
) -> Result<WorkNotesDto, String> {
    let mut profile = engines::profile(engine_id)?;
    if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
        profile.model = model.to_string();
    }
    job.set_total(prepared.eligible.len() as u32)?;
    if prepared.eligible.is_empty() {
        return Ok(empty_dto(
            &prepared.resolved,
            prepared.skipped_sparse,
            prepared.numbers,
        ));
    }

    let work_dir = engines::ensure_work_dir(app_data_dir)?;
    engines::write_schemas(&work_dir)?;
    let mapped = orchestrate::map_sessions(
        &prepared.eligible,
        runner,
        &work_dir,
        &profile,
        engine_id,
        model,
        job,
    )?;
    if mapped.cancelled {
        return Err(job::CANCELLED_MESSAGE.to_string());
    }

    let mut usage = mapped.usage;
    let (headline, entries, closing) = if mapped.summaries.is_empty() {
        (String::new(), Vec::new(), String::new())
    } else {
        job.set_total(prepared.eligible.len() as u32 + 1)?;
        let reduced = orchestrate::reduce_summaries(
            &mapped.summaries,
            runner,
            &work_dir,
            engine_id,
            model,
            job,
        )?;
        usage.add(&reduced.3);
        job.record_processed()?;
        (reduced.0, reduced.1, reduced.2)
    };

    let priced = estimate::price_tokens(&profile, usage.input_tokens, usage.output_tokens, prices);
    let actual_unpriced = if usage.known { priced.unpriced } else { true };
    let actual_cost = if usage.known { priced.amount } else { None };
    let has_data = !headline.is_empty() || !entries.is_empty() || !closing.is_empty();

    Ok(WorkNotesDto {
        range_kind: prepared.resolved.kind,
        start_date: prepared.resolved.start_date,
        end_date: prepared.resolved.end_date,
        has_data,
        skipped_sparse: prepared.skipped_sparse,
        session_count: prepared.numbers.session_count,
        project_count: prepared.numbers.project_count,
        active_days: prepared.numbers.active_days,
        total_tokens: prepared.numbers.total_tokens,
        headline,
        entries,
        closing,
        failed_count: mapped.failures.len() as i64,
        failures: mapped.failures,
        actual_input_tokens: usage.input_tokens,
        actual_output_tokens: usage.output_tokens,
        actual_cost,
        actual_unpriced,
    })
}

pub struct PreparedWorkNotes {
    resolved: period::ResolvedRange,
    skipped_sparse: i64,
    numbers: HardNumbers,
    eligible: Vec<(ConversationSessionRow, Vec<ConversationEvent>)>,
}

pub struct GeneratedRecord {
    pub engine_id: String,
    pub session_id: Option<String>,
    pub cwd: PathBuf,
    pub started_at: String,
    pub ended_at: String,
}

pub struct RecordingRunner<'a> {
    inner: &'a dyn EngineRunner,
    engine_id: String,
    records: Mutex<Vec<GeneratedRecord>>,
}

impl<'a> RecordingRunner<'a> {
    pub fn new(inner: &'a dyn EngineRunner, engine_id: &str) -> Self {
        Self {
            inner,
            engine_id: engine_id.to_string(),
            records: Mutex::new(Vec::new()),
        }
    }

    pub fn take(&self) -> Vec<GeneratedRecord> {
        self.records
            .lock()
            .map(|mut records| std::mem::take(&mut *records))
            .unwrap_or_default()
    }
}

impl EngineRunner for RecordingRunner<'_> {
    fn run(&self, command: &EngineCommand, cancel: &AtomicBool) -> Result<String, EngineError> {
        if !engines::writes_session_dir(&self.engine_id) {
            return self.inner.run(command, cancel);
        }
        let started_at = chrono::Utc::now().to_rfc3339();
        let result = self.inner.run(command, cancel);
        let ended_at = chrono::Utc::now().to_rfc3339();
        if let Ok(mut records) = self.records.lock() {
            records.push(GeneratedRecord {
                engine_id: self.engine_id.clone(),
                session_id: command.session_id.clone(),
                cwd: command.cwd.clone(),
                started_at,
                ended_at,
            });
        }
        result
    }
}

pub fn flush_generated(conn: &Connection, records: &[GeneratedRecord]) -> Result<(), String> {
    for record in records {
        crate::store::record_generated_session(
            conn,
            &record.engine_id,
            record.session_id.as_deref(),
            &record.cwd,
            &record.started_at,
            &record.ended_at,
        )?;
    }
    Ok(())
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

fn classify_sessions(
    conn: &Connection,
    sessions: Vec<ConversationSessionRow>,
) -> Result<(i64, Vec<ConversationSessionRow>), String> {
    let generated = crate::store::load_generated_sessions(conn)?;
    let mut skipped_sparse = 0i64;
    let mut eligible = Vec::new();
    for session in sessions {
        if crate::store::session_is_generated(
            &generated,
            &session.source,
            &session.session_id,
            &session.project,
        ) {
            continue;
        }
        let event_count =
            conversation::indexed_event_count(conn, &session.source, &session.session_id)? as usize;
        if input::is_sparse(event_count, session.total_tokens) {
            skipped_sparse += 1;
            continue;
        }
        eligible.push(session);
    }
    Ok((skipped_sparse, eligible))
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
        failed_count: 0,
        failures: Vec::new(),
        actual_input_tokens: 0,
        actual_output_tokens: 0,
        actual_cost: None,
        actual_unpriced: true,
    }
}
