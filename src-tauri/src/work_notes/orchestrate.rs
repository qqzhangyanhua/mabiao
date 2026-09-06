use std::path::Path;
use std::sync::Mutex;
use std::thread;

use crate::domain::{
    ConversationEvent, ConversationSessionRow, EngineProfile, WorkNotesEntry, WorkNotesFailure,
};

use super::engines::{self, EngineRunner, SchemaKind};
use super::input;
use super::job::{SessionKey, WorkNotesJob, CANCELLED_MESSAGE};
use super::parse::{self, ParseOutcome};
use super::prompt::{self, SessionSummary};
use super::usage::EngineUsage;

#[derive(serde::Deserialize)]
struct MapOut {
    summary: String,
}

pub struct MapPhase {
    pub summaries: Vec<SessionSummary>,
    pub failures: Vec<WorkNotesFailure>,
    pub usage: EngineUsage,
    pub cancelled: bool,
}

pub fn map_sessions(
    sessions: &[(ConversationSessionRow, Vec<ConversationEvent>)],
    runner: &dyn EngineRunner,
    work_dir: &Path,
    profile: &EngineProfile,
    engine_id: &str,
    model: Option<&str>,
    job: &WorkNotesJob,
) -> Result<MapPhase, String> {
    let mut summaries = Vec::new();
    let mut remaining = Vec::new();
    for (session, events) in sessions {
        let key = SessionKey {
            source: session.source.clone(),
            session_id: session.session_id.clone(),
        };
        if let Some(summary) = job.completed_summary(&key)? {
            summaries.push(SessionSummary {
                project: input::project_dir_name(&session.project),
                title: session.title.clone(),
                summary,
            });
        } else {
            remaining.push((session, events));
        }
    }

    let queue = Mutex::new(remaining.into_iter());
    let collected = Mutex::new(PartialMap {
        summaries,
        failures: Vec::new(),
        usage: EngineUsage::default(),
        cancelled: false,
    });
    let workers = profile.concurrency.max(1) as usize;
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                if job.is_cancelled() {
                    if let Ok(mut partial) = collected.lock() {
                        partial.cancelled = true;
                    }
                    break;
                }
                let next = queue.lock().ok().and_then(|mut guard| guard.next());
                let Some((session, events)) = next else {
                    break;
                };
                let _ = job.set_current(&session.title);
                match map_one(session, events, runner, work_dir, engine_id, model, job) {
                    MapOne::Cancelled => {
                        if let Ok(mut partial) = collected.lock() {
                            partial.cancelled = true;
                        }
                        break;
                    }
                    MapOne::Failed(failure) => {
                        if let Ok(mut partial) = collected.lock() {
                            partial.usage.add(&failure.usage);
                            partial.failures.push(failure.item);
                        }
                        let _ = job.record_processed();
                    }
                    MapOne::Ok(done) => {
                        let key = SessionKey {
                            source: session.source.clone(),
                            session_id: session.session_id.clone(),
                        };
                        let _ = job.record_completed(key, done.summary.summary.clone());
                        if let Ok(mut partial) = collected.lock() {
                            partial.usage.add(&done.usage);
                            partial.summaries.push(done.summary);
                        }
                    }
                }
            });
        }
    });
    let partial = collected
        .into_inner()
        .map_err(|error| format!("工作纪要汇总锁损坏：{error}"))?;
    Ok(MapPhase {
        summaries: partial.summaries,
        failures: partial.failures,
        usage: partial.usage,
        cancelled: partial.cancelled || job.is_cancelled(),
    })
}

struct PartialMap {
    summaries: Vec<SessionSummary>,
    failures: Vec<WorkNotesFailure>,
    usage: EngineUsage,
    cancelled: bool,
}

struct Mapped {
    summary: SessionSummary,
    usage: EngineUsage,
}

struct FailedMap {
    item: WorkNotesFailure,
    usage: EngineUsage,
}

enum MapOne {
    Ok(Mapped),
    Failed(FailedMap),
    Cancelled,
}

fn map_one(
    session: &ConversationSessionRow,
    events: &[ConversationEvent],
    runner: &dyn EngineRunner,
    work_dir: &Path,
    engine_id: &str,
    model: Option<&str>,
    job: &WorkNotesJob,
) -> MapOne {
    let compressed = input::compress(session, events);
    let map_prompt = prompt::map_prompt(&compressed);
    let parsed = parse::run::<MapOut>(
        runner,
        || {
            engines::command(
                engine_id,
                work_dir,
                SchemaKind::Map,
                map_prompt.clone(),
                model,
            )
        },
        job.cancel_flag(),
    );
    match parsed.outcome {
        ParseOutcome::Cancelled => MapOne::Cancelled,
        ParseOutcome::Failed(error) => MapOne::Failed(FailedMap {
            item: WorkNotesFailure {
                title: session.title.clone(),
                error,
            },
            usage: parsed.usage,
        }),
        ParseOutcome::Parsed(value) => MapOne::Ok(Mapped {
            summary: SessionSummary {
                project: input::project_dir_name(&session.project),
                title: session.title.clone(),
                summary: value.summary,
            },
            usage: parsed.usage,
        }),
        ParseOutcome::Plain(raw) => MapOne::Ok(Mapped {
            summary: SessionSummary {
                project: input::project_dir_name(&session.project),
                title: session.title.clone(),
                summary: input::take_chars(raw.trim(), 200),
            },
            usage: parsed.usage,
        }),
    }
}

pub fn reduce_summaries(
    summaries: &[SessionSummary],
    runner: &dyn EngineRunner,
    work_dir: &Path,
    engine_id: &str,
    model: Option<&str>,
    job: &WorkNotesJob,
) -> Result<(String, Vec<WorkNotesEntry>, String, EngineUsage), String> {
    if job.is_cancelled() {
        return Err(CANCELLED_MESSAGE.to_string());
    }
    let _ = job.set_current("正在汇总");
    let reduce_prompt = prompt::reduce_prompt(summaries);
    let parsed = parse::run_with(
        runner,
        || {
            engines::command(
                engine_id,
                work_dir,
                SchemaKind::Reduce,
                reduce_prompt.clone(),
                model,
            )
        },
        job.cancel_flag(),
        parse_reduce,
    );
    match parsed.outcome {
        ParseOutcome::Cancelled => Err(CANCELLED_MESSAGE.to_string()),
        ParseOutcome::Failed(error) => Err(error),
        ParseOutcome::Parsed(value) => {
            Ok((value.headline, value.entries, value.closing, parsed.usage))
        }
        ParseOutcome::Plain(raw) => {
            let (headline, entries, closing) = parse::degrade_reduce(&raw);
            Ok((headline, entries, closing, parsed.usage))
        }
    }
}

#[derive(serde::Deserialize)]
pub struct ReduceOut {
    pub headline: String,
    pub entries: Vec<WorkNotesEntry>,
    pub closing: String,
}

fn parse_reduce(raw: &str) -> Option<ReduceOut> {
    let value: ReduceOut = parse::parse_structured(raw)?;
    (3..=6).contains(&value.entries.len()).then_some(value)
}
