use chrono::{DateTime, Datelike, Duration, Local, Utc};

use crate::domain::{Filter, WorkNotesRange, WorkNotesRangeKind};
use crate::work_timeline;

pub struct ResolvedRange {
    pub kind: WorkNotesRangeKind,
    pub start_date: String,
    pub end_date: String,
    pub from: String,
    pub to: String,
}

pub fn resolve(range: &WorkNotesRange, now: DateTime<Local>) -> Result<ResolvedRange, String> {
    match range.kind {
        WorkNotesRangeKind::ThisWeek => {
            let today = now.date_naive();
            let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
            let from = work_timeline::rfc3339_millis(work_timeline::local_midnight_utc(monday));
            let to = work_timeline::rfc3339_millis(now.with_timezone(&Utc));
            Ok(ResolvedRange {
                kind: range.kind,
                start_date: monday.format("%Y-%m-%d").to_string(),
                end_date: today.format("%Y-%m-%d").to_string(),
                from,
                to,
            })
        }
        WorkNotesRangeKind::ThisMonth | WorkNotesRangeKind::Custom => {
            Err("目前只支持本周".to_string())
        }
    }
}

pub fn usage_filter(range: &ResolvedRange) -> Filter {
    Filter {
        from: Some(range.from.clone()),
        to: Some(range.to.clone()),
        sources: Vec::new(),
        models: Vec::new(),
        projects: Vec::new(),
        providers: Vec::new(),
    }
}
