use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::domain::{WorkNotesDto, WorkNotesJobStatus, WorkNotesProgressDto, WorkNotesRange};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub source: String,
    pub session_id: String,
    pub engine: String,
    pub model: String,
}

struct JobState {
    status: WorkNotesJobStatus,
    range: Option<WorkNotesRange>,
    engine_id: Option<String>,
    model: Option<String>,
    done: u32,
    total: u32,
    current_title: String,
    error: String,
    result: Option<WorkNotesDto>,
    completed: HashMap<SessionKey, String>,
}

impl Default for JobState {
    fn default() -> Self {
        Self {
            status: WorkNotesJobStatus::Idle,
            range: None,
            engine_id: None,
            model: None,
            done: 0,
            total: 0,
            current_title: String::new(),
            error: String::new(),
            result: None,
            completed: HashMap::new(),
        }
    }
}

/// 后台任务把进度写进这里，前端轮询。取消用独立原子标志，好让 spawn 的 CLI 立刻停。
pub struct WorkNotesJob {
    cancel: AtomicBool,
    state: Mutex<JobState>,
}

impl Default for WorkNotesJob {
    fn default() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            state: Mutex::new(JobState::default()),
        }
    }
}

impl WorkNotesJob {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn clear_completed_if_idle(&self) -> Result<(), String> {
        let mut state = self.lock()?;
        if state.status != WorkNotesJobStatus::Running {
            state.completed.clear();
        }
        Ok(())
    }

    pub fn begin(
        &self,
        range: &WorkNotesRange,
        engine_id: &str,
        model: Option<&str>,
    ) -> Result<(), String> {
        let mut state = self.lock()?;
        if state.status == WorkNotesJobStatus::Running {
            return Err("正在生成工作纪要".to_string());
        }
        let model = model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let reuse = matches!(
            state.status,
            WorkNotesJobStatus::Cancelled | WorkNotesJobStatus::Error
        ) && state.range.as_ref() == Some(range)
            && state.engine_id.as_deref() == Some(engine_id)
            && state.model == model;
        if !reuse {
            state.completed.clear();
        }
        state.range = Some(range.clone());
        state.engine_id = Some(engine_id.to_string());
        state.model = model;
        state.status = WorkNotesJobStatus::Running;
        state.error.clear();
        state.result = None;
        state.done = state.completed.len() as u32;
        state.total = 0;
        state.current_title.clear();
        self.cancel.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn begin_session_summary(
        &self,
        engine_id: &str,
        model: Option<&str>,
    ) -> Result<(), String> {
        let mut state = self.lock()?;
        if state.status == WorkNotesJobStatus::Running {
            return Err("正在生成工作纪要".to_string());
        }
        let model = model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        state.completed.clear();
        state.range = None;
        state.engine_id = Some(engine_id.to_string());
        state.model = model;
        state.status = WorkNotesJobStatus::Running;
        state.error.clear();
        state.result = None;
        state.done = 0;
        state.total = 1;
        state.current_title.clear();
        self.cancel.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn finish_session_summary(&self, result: Result<(), String>) -> Result<(), String> {
        let cancelled = self.is_cancelled()
            || result
                .as_ref()
                .err()
                .is_some_and(|error| error == CANCELLED_MESSAGE);
        let mut state = self.lock()?;
        if cancelled {
            state.status = WorkNotesJobStatus::Cancelled;
            state.current_title.clear();
            state.error.clear();
            state.result = None;
            return Ok(());
        }
        match result {
            Ok(()) => {
                state.status = WorkNotesJobStatus::Done;
                state.current_title.clear();
                state.error.clear();
                state.result = None;
                state.completed.clear();
            }
            Err(error) => {
                state.status = WorkNotesJobStatus::Error;
                state.current_title.clear();
                state.error = error;
                state.result = None;
            }
        }
        Ok(())
    }

    pub fn set_total(&self, total: u32) -> Result<(), String> {
        let mut state = self.lock()?;
        state.total = total;
        Ok(())
    }

    pub fn set_current(&self, title: &str) -> Result<(), String> {
        let mut state = self.lock()?;
        state.current_title = title.to_string();
        Ok(())
    }

    pub fn completed_summary(&self, key: &SessionKey) -> Result<Option<String>, String> {
        let state = self.lock()?;
        Ok(state.completed.get(key).cloned())
    }

    pub fn record_completed(&self, key: SessionKey, summary: String) -> Result<(), String> {
        let mut state = self.lock()?;
        state.completed.insert(key, summary);
        state.done = state.done.saturating_add(1);
        Ok(())
    }

    pub fn record_processed(&self) -> Result<(), String> {
        let mut state = self.lock()?;
        state.done = state.done.saturating_add(1);
        Ok(())
    }

    pub fn finish(&self, result: Result<WorkNotesDto, String>) -> Result<(), String> {
        let cancelled = self.is_cancelled()
            || result
                .as_ref()
                .err()
                .is_some_and(|error| error == CANCELLED_MESSAGE);
        let mut state = self.lock()?;
        if cancelled {
            state.status = WorkNotesJobStatus::Cancelled;
            state.current_title.clear();
            state.error.clear();
            state.result = None;
            return Ok(());
        }
        match result {
            Ok(dto) => {
                state.status = WorkNotesJobStatus::Done;
                state.current_title.clear();
                state.error.clear();
                state.result = Some(dto);
                state.completed.clear();
            }
            Err(error) => {
                state.status = WorkNotesJobStatus::Error;
                state.current_title.clear();
                state.error = error;
                state.result = None;
            }
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Result<WorkNotesProgressDto, String> {
        let state = self.lock()?;
        Ok(WorkNotesProgressDto {
            status: state.status,
            done: state.done,
            total: state.total,
            current_title: state.current_title.clone(),
            error: state.error.clone(),
            result: state.result.clone(),
        })
    }

    #[cfg(test)]
    pub fn completed_session_ids(&self) -> Vec<String> {
        let state = self.state.lock().expect("work notes job");
        let mut ids: Vec<String> = state
            .completed
            .keys()
            .map(|key| key.session_id.clone())
            .collect();
        ids.sort();
        ids
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, JobState>, String> {
        self.state
            .lock()
            .map_err(|error| format!("工作纪要任务锁损坏：{error}"))
    }
}

pub const CANCELLED_MESSAGE: &str = "已取消";
