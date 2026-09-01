//! 按连续两次官方快照估计「何时打满」。
//!
//! 官方额度是快照百分比，不是时间序列。本机 5 小时燃烧是另一口径，
//! 不能叠进官方进度条（ADR 0008）。这里只认官方自己的前后两拍。

use chrono::{DateTime, Duration, Utc};

use crate::domain::{OfficialQuotaWindow, QuotaExhaustDto, QuotaExhaustKind};

/// 两次快照间隔太短（连点刷新）时速率没有意义。
const MIN_INTERVAL: Duration = Duration::seconds(60);
/// 小于这个百分点的变化当成抖动。
const MIN_PROGRESS: f64 = 0.05;
/// 百分点回落超过这个值，当成新窗口，不用上一拍。
const DROP_PROGRESS: f64 = 1.0;
/// ≥ 这个百分点视为已经打满。
const EXHAUSTED_AT: f64 = 99.5;
/// 超过这个跨度的撞线时刻不再展示（月额度闲置时会远到没意义）。
const MAX_HORIZON: Duration = Duration::days(90);

pub fn attach(
    windows: &mut [OfficialQuotaWindow],
    captured_at: &str,
    prev_windows: &[OfficialQuotaWindow],
    prev_captured_at: Option<&str>,
    now: DateTime<Utc>,
) {
    let captured = parse_time(captured_at);
    let prev_at = prev_captured_at.and_then(parse_time);
    for window in windows.iter_mut() {
        let prev = match (
            prev_windows.iter().find(|item| item.kind == window.kind),
            captured,
            prev_at,
        ) {
            (Some(prev_window), Some(captured_at), Some(prev_at)) => {
                Some((prev_window, captured_at, prev_at))
            }
            _ => None,
        };
        window.exhaust = estimate(window, prev, now);
    }
}

fn estimate(
    current: &OfficialQuotaWindow,
    prev: Option<(&OfficialQuotaWindow, DateTime<Utc>, DateTime<Utc>)>,
    now: DateTime<Utc>,
) -> Option<QuotaExhaustDto> {
    let progress = progress_of(current)?;
    if progress >= EXHAUSTED_AT {
        return Some(QuotaExhaustDto {
            kind: QuotaExhaustKind::Exhausted,
            at: None,
        });
    }
    let reset_at = current.resets_at.as_deref().and_then(parse_time);
    if reset_at.is_some_and(|reset| reset <= now) {
        return None;
    }
    let (prev_window, captured_at, prev_at) = prev?;
    if !same_window(prev_window, current) {
        return None;
    }
    let interval = captured_at - prev_at;
    if interval < MIN_INTERVAL {
        return None;
    }
    let prev_progress = progress_of(prev_window)?;
    let delta = progress - prev_progress;
    if delta < MIN_PROGRESS {
        if reset_at.is_some() && progress > 0.0 {
            return Some(QuotaExhaustDto {
                kind: QuotaExhaustKind::WillNotHit,
                at: None,
            });
        }
        return None;
    }
    let dt_secs = interval.num_milliseconds() as f64 / 1000.0;
    if dt_secs <= 0.0 {
        return None;
    }
    let remaining = 100.0 - progress;
    let eta_ms = remaining / (delta / dt_secs) * 1000.0;
    if !eta_ms.is_finite() || eta_ms < 0.0 {
        return None;
    }
    if eta_ms > MAX_HORIZON.num_milliseconds() as f64 {
        return if reset_at.is_some() {
            Some(QuotaExhaustDto {
                kind: QuotaExhaustKind::WillNotHit,
                at: None,
            })
        } else {
            None
        };
    }
    let eta = captured_at + Duration::milliseconds(eta_ms.round() as i64);
    if reset_at.is_some_and(|reset| eta >= reset) {
        return Some(QuotaExhaustDto {
            kind: QuotaExhaustKind::WillNotHit,
            at: None,
        });
    }
    Some(QuotaExhaustDto {
        kind: QuotaExhaustKind::Hits,
        at: Some(eta.to_rfc3339()),
    })
}

fn same_window(prev: &OfficialQuotaWindow, current: &OfficialQuotaWindow) -> bool {
    if prev.kind != current.kind {
        return false;
    }
    match (
        prev.resets_at.as_deref().and_then(parse_time),
        current.resets_at.as_deref().and_then(parse_time),
    ) {
        (Some(left), Some(right)) if left != right => return false,
        (Some(_), None) | (None, Some(_)) => return false,
        _ => {}
    }
    !matches!(
        (progress_of(prev), progress_of(current)),
        (Some(left), Some(right)) if right + DROP_PROGRESS < left
    )
}

fn progress_of(window: &OfficialQuotaWindow) -> Option<f64> {
    if let Some(percent) = window.used_percent {
        return Some(percent.clamp(0.0, 100.0));
    }
    match (window.used_amount, window.limit_amount) {
        (Some(used), Some(limit)) if limit > 0.0 => Some((used / limit * 100.0).clamp(0.0, 100.0)),
        _ => None,
    }
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|stamp| stamp.with_timezone(&Utc))
}
