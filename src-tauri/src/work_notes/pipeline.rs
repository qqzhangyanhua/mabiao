//! 区间纪要的三个阶段：读连接取输入（`prepare`）、无连接调引擎（`run`）、取写连接落盘
//! （`persist`）。
//!
//! 阶段之所以分开，是因为 spawn 引擎那段不能占着连接（ADR 0021）；阶段之所以私有，是因为
//! 拼接顺序、以及「没东西可写就不取写连接」这些都不该由调用方记。锁编排在模块根的
//! `generate` 上。

use chrono::{DateTime, Local};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use crate::conversation;
use crate::domain::{
    ConversationQuery, ConversationSessionRow, EngineCommand, Filter, PriceTable, WorkNotesDto,
    WorkNotesParams,
};
use crate::query;

use super::cache;
use super::engines::{self, EngineError, EngineRunner};
use super::estimate;
use super::input;
use super::job::{self, WorkNotesJob};
use super::orchestrate;
use super::period;
use super::scale;
use super::EligibleSession;

pub(super) struct PreparedWorkNotes {
    pub(super) resolved: period::ResolvedRange,
    pub(super) skipped_sparse: i64,
    pub(super) numbers: HardNumbers,
    pub(super) extra: String,
    pub(super) engine: String,
    pub(super) model: String,
    pub(super) session_set_hash: String,
    pub(super) exact: Option<WorkNotesDto>,
    pub(super) latest: Option<WorkNotesDto>,
    pub(super) eligible: Vec<EligibleSession>,
}

pub(super) fn prepare(
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

pub(super) struct RunOutput {
    pub(super) result: Result<WorkNotesDto, String>,
    writes: CacheWrites,
    generated: Vec<GeneratedRecord>,
}

impl RunOutput {
    /// 命中 exact 缓存或一条都没跑成时没有任何东西要写，此时不该去抢写连接。
    pub(super) fn needs_persist(&self) -> bool {
        !self.generated.is_empty()
            || self.writes.notes.is_some()
            || !self.writes.sessions.is_empty()
    }
}

#[derive(Default)]
struct CacheWrites {
    sessions: Vec<FreshCache>,
    notes: Option<WorkNotesDto>,
    session_set_hash: String,
    extra: String,
    engine: String,
    model: String,
    created_at: String,
}

struct FreshCache {
    source: String,
    session_id: String,
    fingerprint: String,
    summary: String,
}

/// 跑完 map/reduce 但不碰连接。返回值里除了纪要本身，还有已完成的单会话摘要与引擎落痕——
/// 取消与失败时这两样仍要落盘，否则用户重开一次就得为同样的会话再付一次钱。
pub(super) fn run(
    prepared: PreparedWorkNotes,
    prices: &PriceTable,
    runner: &dyn EngineRunner,
    app_data_dir: &Path,
    params: &WorkNotesParams,
    now: DateTime<Local>,
    job: &WorkNotesJob,
) -> RunOutput {
    let recorder = RecordingRunner::new(runner, params.engine_id());
    let mut writes = CacheWrites {
        session_set_hash: prepared.session_set_hash.clone(),
        extra: prepared.extra.clone(),
        engine: prepared.engine.clone(),
        model: prepared.model.clone(),
        created_at: now.to_rfc3339(),
        ..CacheWrites::default()
    };
    let result = run_inner(
        prepared,
        prices,
        &recorder,
        app_data_dir,
        params,
        job,
        &mut writes,
    );
    RunOutput {
        result,
        writes,
        generated: recorder.take(),
    }
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

/// 引擎落痕与两张缓存表一起写。落痕必须先写：先记下「这条会话是码表自己烧出来的」，
/// 再写它产出的摘要（ADR 0021）。
pub(super) fn persist(conn: &Connection, output: &RunOutput) -> Result<(), String> {
    flush_generated(conn, &output.generated)?;
    let writes = &output.writes;
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

pub(super) struct GeneratedRecord {
    engine_id: String,
    session_id: Option<String>,
    cwd: PathBuf,
    started_at: String,
    ended_at: String,
}

/// 包住注入的 runner，记下会写回自己会话目录的引擎跑出来的那些会话（ADR 0021 约束三）。
/// 它在 `run` 内部套上，调用方没有「忘记包」这个选项。
pub(super) struct RecordingRunner<'a> {
    inner: &'a dyn EngineRunner,
    engine_id: String,
    records: Mutex<Vec<GeneratedRecord>>,
}

impl<'a> RecordingRunner<'a> {
    pub(super) fn new(inner: &'a dyn EngineRunner, engine_id: &str) -> Self {
        Self {
            inner,
            engine_id: engine_id.to_string(),
            records: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn take(&self) -> Vec<GeneratedRecord> {
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

pub(super) fn flush_generated(
    conn: &Connection,
    records: &[GeneratedRecord],
) -> Result<(), String> {
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

pub(super) struct HardNumbers {
    pub(super) session_count: i64,
    pub(super) project_count: i64,
    pub(super) active_days: i64,
    pub(super) total_tokens: i64,
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
