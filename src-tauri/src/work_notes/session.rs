//! 单条会话摘要的三个阶段，与 `pipeline` 同形：读连接取正文、无连接调引擎、取写连接落盘。
//! 它只跑 map、不跑 reduce，也不写纪要表。锁编排在模块根的 `summarize_session` 上。

use chrono::{DateTime, Local};
use rusqlite::Connection;
use std::path::Path;

use crate::conversation;
use crate::domain::{
    ConversationEvent, ConversationSessionRow, WorkNotesSessionParams, WorkNotesSessionSummary,
};

use super::cache;
use super::engines::{self, EngineRunner};
use super::job::WorkNotesJob;
use super::orchestrate;
use super::pipeline::{flush_generated, GeneratedRecord, RecordingRunner};

pub(super) struct PreparedSessionSummary {
    source: String,
    session_id: String,
    title: String,
    fingerprint: String,
    engine: String,
    model: String,
    events: Vec<ConversationEvent>,
    session: ConversationSessionRow,
}

pub(super) struct SessionSummaryRun {
    result: Result<String, String>,
    generated: Vec<GeneratedRecord>,
}

impl SessionSummaryRun {
    /// 没有引擎落痕、摘要也没跑出来时，回那条错误：一个字都不用写，写连接也不必取。
    pub(super) fn nothing_to_persist(&self) -> Option<String> {
        if !self.generated.is_empty() {
            return None;
        }
        self.result.as_ref().err().cloned()
    }
}

pub(super) fn prepare(
    conn: &Connection,
    params: &WorkNotesSessionParams,
) -> Result<PreparedSessionSummary, String> {
    let (source, session_id) = (params.source.as_str(), params.session_id.as_str());
    engines::require(params.engine_id())?;
    let session = conversation::load_session(conn, source, session_id)?
        .ok_or_else(|| "未找到该对话记录".to_string())?;
    if session.generated_by_work_notes {
        return Err("码表生成的会话不写摘要".to_string());
    }
    let events = conversation::indexed_events(conn, source, session_id)?;
    if events.is_empty() {
        return Err("没有可总结的正文".to_string());
    }
    let fingerprint = cache::session_fingerprint(conn, source, session_id)?;
    Ok(PreparedSessionSummary {
        source: source.to_string(),
        session_id: session_id.to_string(),
        title: session.title.clone(),
        fingerprint,
        engine: params.engine_id().to_string(),
        model: params.model_id().to_string(),
        events,
        session,
    })
}

pub(super) fn run(
    prepared: &PreparedSessionSummary,
    runner: &dyn EngineRunner,
    app_data_dir: &Path,
    job: &WorkNotesJob,
) -> SessionSummaryRun {
    let recorder = RecordingRunner::new(runner, &prepared.engine);
    let result = (|| {
        job.set_total(1)?;
        job.set_current(&prepared.title)?;
        let work_dir = engines::ensure_work_dir(app_data_dir)?;
        engines::write_schemas(&work_dir)?;
        let model = if prepared.model.is_empty() {
            None
        } else {
            Some(prepared.model.as_str())
        };
        let summary = orchestrate::summarize_one(
            &prepared.session,
            &prepared.events,
            &recorder,
            &work_dir,
            &prepared.engine,
            model,
            job,
        )?;
        let trimmed = summary.trim();
        if trimmed.is_empty() {
            return Err("引擎没有写出摘要".to_string());
        }
        Ok(trimmed.to_string())
    })();
    SessionSummaryRun {
        result,
        generated: recorder.take(),
    }
}

pub(super) fn persist(
    conn: &Connection,
    prepared: &PreparedSessionSummary,
    ran: &SessionSummaryRun,
    now: DateTime<Local>,
) -> Result<Vec<WorkNotesSessionSummary>, String> {
    flush_generated(conn, &ran.generated)?;
    let summary = ran.result.clone()?;
    cache::store_session_summary(
        conn,
        &cache::SessionCacheKey {
            source: &prepared.source,
            session_id: &prepared.session_id,
            fingerprint: &prepared.fingerprint,
            engine: &prepared.engine,
            model: &prepared.model,
        },
        &summary,
        &now.to_rfc3339(),
    )?;
    let session = conversation::load_session(conn, &prepared.source, &prepared.session_id)?
        .ok_or_else(|| "未找到该对话记录".to_string())?;
    Ok(session.work_notes_summaries)
}
