//! 工作纪要：按区间读对话正文，经本机 CLI 总结成结构化条目（ADR 0021）。
//! 主入口 `build` 对齐 `report::build`：注入连接、注入 now、注入 runner。

mod cache;
mod engines;
mod estimate;
mod input;
mod job;
mod orchestrate;
mod parse;
mod period;
mod prompt;
mod scale;
mod session;
mod usage;

use chrono::{DateTime, Local};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use crate::conversation;
use crate::domain::{
    ConversationEvent, ConversationQuery, ConversationSessionRow, EngineCommand, Filter,
    PriceTable, WorkNotesDto, WorkNotesHistoryPage, WorkNotesHistoryQuery, WorkNotesParams,
    WorkNotesPreviewDto,
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
pub use session::{persist_session_summary, prepare_session_summary, run_session_summary};

pub struct EligibleSession {
    pub session: ConversationSessionRow,
    pub events: Vec<ConversationEvent>,
    pub fingerprint: String,
    pub cached_summary: Option<String>,
}

/// 选完区间立刻返回会话数、闸门、成本预估与上次纪要，不调引擎。
pub fn preview(
    conn: &Connection,
    prices: &PriceTable,
    params: &WorkNotesParams,
    now: DateTime<Local>,
) -> Result<WorkNotesPreviewDto, String> {
    let prepared = prepare(conn, prices, params, now, true)?;
    let mut profile =
        engines::profile(params.engine_id()).unwrap_or_else(|_| engines::codex_profile());
    if !params.model_id().is_empty() {
        profile.model = params.model_id().to_string();
    }
    let estimate = if prepared.exact.is_some() {
        estimate::Estimate {
            calls: 0,
            secs: 0,
            input_tokens: 0,
            cost: None,
            unpriced: false,
        }
    } else {
        estimate::for_remaining(&prepared.eligible, params.extra(), &profile, prices)
    };
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
        cached: prepared.latest,
    })
}

pub fn require_engine(engine_id: &str) -> Result<(), String> {
    engines::require(engine_id)
}

/// 历史纪要列表：按创建时间倒序分页，可选按引擎筛选。
pub fn history(
    conn: &Connection,
    query: &WorkNotesHistoryQuery,
) -> Result<WorkNotesHistoryPage, String> {
    let engine = query
        .engine
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    cache::list_reports(
        conn,
        engine,
        query.page.unwrap_or(1),
        query.page_size.unwrap_or(20),
    )
}

/// 取历史列表某一条的完整内容（entries/closing/failures 等）。
pub fn history_entry(conn: &Connection, id: i64) -> Result<WorkNotesDto, String> {
    cache::load_notes_by_id(conn, id)?.ok_or_else(|| "这条纪要已经不在了".to_string())
}

/// 删除一条历史纪要。
pub fn delete_history_entry(conn: &Connection, id: i64) -> Result<(), String> {
    if cache::delete_report(conn, id)? {
        Ok(())
    } else {
        Err("这条纪要已经不在了".to_string())
    }
}

/// 工作纪要模块的单一入口。`now`、`runner`、`job` 由调用方注入。
pub fn build(
    conn: &Connection,
    prices: &PriceTable,
    params: &WorkNotesParams,
    now: DateTime<Local>,
    runner: &dyn EngineRunner,
    app_data_dir: &Path,
    job: &WorkNotesJob,
) -> Result<WorkNotesDto, String> {
    engines::require(params.engine_id())?;
    let prepared = prepare(conn, prices, params, now, false)?;
    let recorder = RecordingRunner::new(runner, params.engine_id());
    let output = run(prepared, prices, &recorder, app_data_dir, params, now, job);
    flush_generated(conn, &recorder.take())?;
    persist_cache(conn, &output.writes)?;
    output.result
}

pub fn prepare(
    conn: &Connection,
    prices: &PriceTable,
    params: &WorkNotesParams,
    now: DateTime<Local>,
    skip_gate: bool,
) -> Result<PreparedWorkNotes, String> {
    let engine = params.engine_id().to_string();
    let model = params.model_id().to_string();
    let extra = params.extra().to_string();
    let resolved = period::resolve(&params.range, now)?;
    let filter = period::usage_filter(&resolved);
    let numbers = hard_numbers(conn, prices, &filter)?;
    let sessions = load_sessions(conn, prices, &resolved.from, &resolved.to)?;
    let (skipped_sparse, eligible_sessions) = classify_sessions(conn, sessions)?;
    let latest = cache::load_latest_notes(conn, &resolved, &engine, &model)?;

    let mut parts = Vec::new();
    for session in &eligible_sessions {
        let fingerprint = cache::session_fingerprint(conn, &session.source, &session.session_id)?;
        parts.push((
            session.source.clone(),
            session.session_id.clone(),
            fingerprint,
        ));
    }
    let session_set_hash = cache::session_set_hash(&parts);
    let exact =
        cache::load_exact_notes(conn, &resolved, &engine, &model, &extra, &session_set_hash)?;
    if exact.is_none() && !skip_gate {
        scale::enforce(eligible_sessions.len() as i64, params.confirmed)?;
    }

    let mut eligible = Vec::new();
    for (session, fingerprint) in eligible_sessions
        .into_iter()
        .zip(parts.into_iter().map(|part| part.2))
    {
        let key = cache::SessionCacheKey {
            source: &session.source,
            session_id: &session.session_id,
            fingerprint: &fingerprint,
            engine: &engine,
            model: &model,
        };
        let cached_summary = cache::load_session_summary(conn, &key)?;
        let events = if cached_summary.is_some() {
            Vec::new()
        } else {
            conversation::indexed_events(conn, &session.source, &session.session_id)?
        };
        eligible.push(EligibleSession {
            session,
            events,
            fingerprint,
            cached_summary,
        });
    }
    Ok(PreparedWorkNotes {
        resolved,
        skipped_sparse,
        numbers,
        extra,
        engine,
        model,
        session_set_hash,
        exact,
        latest,
        eligible,
    })
}

pub struct RunOutput {
    pub result: Result<WorkNotesDto, String>,
    pub writes: CacheWrites,
}

#[derive(Default)]
pub struct CacheWrites {
    pub sessions: Vec<FreshCache>,
    pub notes: Option<WorkNotesDto>,
    pub session_set_hash: String,
    pub extra: String,
    pub engine: String,
    pub model: String,
    pub created_at: String,
}

pub struct FreshCache {
    pub source: String,
    pub session_id: String,
    pub fingerprint: String,
    pub summary: String,
}

pub fn run(
    prepared: PreparedWorkNotes,
    prices: &PriceTable,
    runner: &dyn EngineRunner,
    app_data_dir: &Path,
    params: &WorkNotesParams,
    now: DateTime<Local>,
    job: &WorkNotesJob,
) -> RunOutput {
    let created_at = now.to_rfc3339();
    let mut writes = CacheWrites {
        session_set_hash: prepared.session_set_hash.clone(),
        extra: prepared.extra.clone(),
        engine: prepared.engine.clone(),
        model: prepared.model.clone(),
        created_at,
        ..CacheWrites::default()
    };
    let result = run_inner(
        prepared,
        prices,
        runner,
        app_data_dir,
        params,
        job,
        &mut writes,
    );
    RunOutput { result, writes }
}

fn run_inner(
    prepared: PreparedWorkNotes,
    prices: &PriceTable,
    runner: &dyn EngineRunner,
    app_data_dir: &Path,
    params: &WorkNotesParams,
    job: &WorkNotesJob,
    writes: &mut CacheWrites,
) -> Result<WorkNotesDto, String> {
    job.clear_completed_if_idle()?;
    if let Some(mut dto) = prepared.exact {
        dto.skipped_sparse = prepared.skipped_sparse;
        dto.session_count = prepared.numbers.session_count;
        dto.project_count = prepared.numbers.project_count;
        dto.active_days = prepared.numbers.active_days;
        dto.total_tokens = prepared.numbers.total_tokens;
        return Ok(dto);
    }
    let engine_id = params.engine_id();
    let model = params.model_id();
    let model_opt = if model.is_empty() { None } else { Some(model) };
    let mut profile = engines::profile(engine_id)?;
    if let Some(model) = model_opt {
        profile.model = model.to_string();
    }
    job.set_total(prepared.eligible.len() as u32)?;
    if prepared.eligible.is_empty() {
        return Ok(empty_dto(
            &prepared.resolved,
            prepared.skipped_sparse,
            prepared.numbers,
            &prepared.extra,
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
        model_opt,
        job,
    )?;
    writes.sessions = mapped
        .fresh
        .into_iter()
        .map(|item| FreshCache {
            source: item.source,
            session_id: item.session_id,
            fingerprint: item.fingerprint,
            summary: item.summary,
        })
        .collect();
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
            model_opt,
            &prepared.extra,
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
    let dto = WorkNotesDto {
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
        extra_instructions: prepared.extra.clone(),
        failed_count: mapped.failures.len() as i64,
        failures: mapped.failures,
        actual_input_tokens: usage.input_tokens,
        actual_output_tokens: usage.output_tokens,
        actual_cost,
        actual_unpriced,
    };
    writes.notes = Some(dto.clone());
    Ok(dto)
}

pub fn persist_cache(conn: &Connection, writes: &CacheWrites) -> Result<(), String> {
    for item in &writes.sessions {
        let key = cache::SessionCacheKey {
            source: &item.source,
            session_id: &item.session_id,
            fingerprint: &item.fingerprint,
            engine: &writes.engine,
            model: &writes.model,
        };
        cache::store_session_summary(conn, &key, &item.summary, &writes.created_at)?;
    }
    if let Some(dto) = &writes.notes {
        cache::store_notes(
            conn,
            dto,
            &writes.engine,
            &writes.model,
            &writes.extra,
            &writes.session_set_hash,
            &writes.created_at,
        )?;
    }
    Ok(())
}

pub struct PreparedWorkNotes {
    resolved: period::ResolvedRange,
    skipped_sparse: i64,
    numbers: HardNumbers,
    extra: String,
    engine: String,
    model: String,
    session_set_hash: String,
    exact: Option<WorkNotesDto>,
    latest: Option<WorkNotesDto>,
    eligible: Vec<EligibleSession>,
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
    extra: &str,
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
        extra_instructions: extra.to_string(),
        failed_count: 0,
        failures: Vec::new(),
        actual_input_tokens: 0,
        actual_output_tokens: 0,
        actual_cost: None,
        actual_unpriced: true,
    }
}
