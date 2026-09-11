//! 工作纪要：按区间读对话正文，经本机 CLI 总结成结构化条目（ADR 0021）。
//!
//! 对外只有三个入口：`preview` 只读取输入不调引擎；`generate` 产出区间纪要；
//! `summarize_session` 只总结一条会话。后两个收 `ConnectionSource` 而不是 `&Connection`，
//! 因为「读连接取输入 → 放开连接调引擎 → 有东西可写才取写连接」这套编排属于实现，
//! 不该由调用方拼。阶段实现在 `pipeline` 与 `session`。
//!
//! 「这条会话是不是码表自己生成的」由 `identity` 判定，对话记录只经 `decorate_sessions` 取结果。

mod cache;
mod engines;
mod estimate;
mod identity;
mod input;
mod job;
mod orchestrate;
mod parse;
mod period;
mod pipeline;
mod prompt;
mod scale;
mod session;
mod usage;

use chrono::{DateTime, Local};
use rusqlite::Connection;
use std::path::Path;
use std::sync::MutexGuard;

use crate::domain::{
    ConversationEvent, ConversationSessionRow, PriceTable, WorkNotesDto, WorkNotesHistoryPage,
    WorkNotesHistoryQuery, WorkNotesParams, WorkNotesPreviewDto, WorkNotesSessionParams,
    WorkNotesSessionSummary,
};

#[cfg(test)]
pub use engines::{
    claude_command, codex_command, codex_profile, cursor_agent_command, detect_with,
    ensure_work_dir, extra_bin_dirs, grok_command, merge_search_dirs, which_in_dirs, write_schemas,
    SchemaKind, ScriptedRunner,
};
pub use engines::{detect_engines, EngineRunner, ProcessRunner};
pub use job::WorkNotesJob;

/// 连接来源。生产上是读池加写锁两把，测试上是同一条内存连接。
///
/// **实现不得同时持有 `read()` 与 `write()` 的 guard。** 生产上嵌套持有会在整段引擎调用期间
/// 堵住写路径——那正是 ADR 0021 要求 spawn 前放开连接的原因；测试 adapter 只有一条连接，
/// 嵌套会直接撞上自己。
pub trait ConnectionSource {
    fn read(&self) -> Result<MutexGuard<'_, Connection>, String>;
    fn write(&self) -> Result<MutexGuard<'_, Connection>, String>;
}

/// 参与本次纪要的一条会话。只在 `pipeline` / `orchestrate` / `estimate` 之间流转，不上 interface。
struct EligibleSession {
    session: ConversationSessionRow,
    events: Vec<ConversationEvent>,
    fingerprint: String,
    cached_summary: Option<String>,
}

/// 选完区间立刻返回会话数、闸门、成本预估与上次纪要，不调引擎。只读，因而直接收连接。
pub fn preview(
    conn: &Connection,
    prices: &PriceTable,
    params: &WorkNotesParams,
    now: DateTime<Local>,
) -> Result<WorkNotesPreviewDto, String> {
    let prepared = pipeline::prepare(conn, prices, params, now, true)?;
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
        sessions: prepared.choices,
    })
}

pub fn require_engine(engine_id: &str) -> Result<(), String> {
    engines::require(engine_id)
}

/// 对话记录给会话行打「码表生成」标记、挂上已有摘要的唯一入口。
///
/// 判定规则与落痕表都在 `identity` 里，调用方不需要知道是靠 session id 还是工作目录认出来的。
pub fn decorate_sessions(
    conn: &Connection,
    rows: &mut [ConversationSessionRow],
) -> Result<(), String> {
    identity::decorate(conn, rows)
}

/// 区间纪要的唯一入口。`now`、runner、`job` 由调用方注入。
pub fn generate(
    conns: &dyn ConnectionSource,
    prices: &PriceTable,
    params: &WorkNotesParams,
    now: DateTime<Local>,
    runner: &dyn EngineRunner,
    app_data_dir: &Path,
    job: &WorkNotesJob,
) -> Result<WorkNotesDto, String> {
    engines::require(params.engine_id())?;
    let prepared = {
        let conn = conns.read()?;
        pipeline::prepare(&conn, prices, params, now, false)?
    };
    let output = pipeline::run(prepared, prices, runner, app_data_dir, params, now, job);
    if output.needs_persist() {
        let conn = conns.write()?;
        pipeline::persist(&conn, &output)?;
    }
    output.result
}

/// 单条会话摘要。与 `generate` 同一套锁编排，只是不跑 reduce、不写纪要表。
pub fn summarize_session(
    conns: &dyn ConnectionSource,
    params: &WorkNotesSessionParams,
    now: DateTime<Local>,
    runner: &dyn EngineRunner,
    app_data_dir: &Path,
    job: &WorkNotesJob,
) -> Result<Vec<WorkNotesSessionSummary>, String> {
    let prepared = {
        let conn = conns.read()?;
        session::prepare(&conn, params)?
    };
    let ran = session::run(&prepared, runner, app_data_dir, job);
    match ran.nothing_to_persist() {
        // 既没有引擎落痕要记、摘要也没跑出来，没有任何东西要写，不必去抢写连接。
        Some(error) => Err(error),
        None => {
            let conn = conns.write()?;
            session::persist(&conn, &prepared, &ran, now)
        }
    }
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
