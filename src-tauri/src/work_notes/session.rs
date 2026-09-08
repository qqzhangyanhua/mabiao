use chrono::{DateTime, Local};
use rusqlite::Connection;
use std::path::Path;

use crate::conversation;
use crate::domain::WorkNotesSessionSummary;

use super::cache;
use super::engines::{self, EngineRunner};
use super::job::WorkNotesJob;
use super::orchestrate;
use super::{flush_generated, RecordingRunner};

pub struct PreparedSessionSummary {
    pub source: String,
    pub session_id: String,
    pub title: String,
    pub fingerprint: String,
    pub engine: String,
    pub model: String,
    pub events: Vec<crate::domain::ConversationEvent>,
    pub session: crate::domain::ConversationSessionRow,
}

pub struct SessionSummaryRun {
    pub result: Result<String, String>,
    pub generated: Vec<super::GeneratedRecord>,
}

pub fn prepare_session_summary(
    conn: &Connection,
    source: &str,
    session_id: &str,
    engine_id: &str,
    model: Option<&str>,
) -> Result<PreparedSessionSummary, String> {
    engines::require(engine_id)?;
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
        engine: engine_id.to_string(),
        model: model.unwrap_or("").trim().to_string(),
        events,
        session,
    })
}

pub fn run_session_summary(
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

pub fn persist_session_summary(
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
