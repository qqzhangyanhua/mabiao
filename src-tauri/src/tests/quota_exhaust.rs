use chrono::{DateTime, Utc};

use crate::domain::{OfficialQuotaConfig, OfficialQuotaWindow, QuotaExhaustDto, QuotaExhaustKind};
use crate::official_quota::{self as quota, exhaust};
use crate::store;

fn at(stamp: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(stamp)
        .unwrap()
        .with_timezone(&Utc)
}

fn window(kind: &str, percent: f64, resets_at: Option<&str>) -> OfficialQuotaWindow {
    OfficialQuotaWindow {
        kind: kind.into(),
        label: kind.into(),
        used_percent: Some(percent),
        resets_at: resets_at.map(str::to_string),
        ..Default::default()
    }
}

fn amount_window(used: f64, limit: f64) -> OfficialQuotaWindow {
    OfficialQuotaWindow {
        kind: "budget".into(),
        label: "预算".into(),
        used_amount: Some(used),
        limit_amount: Some(limit),
        currency: Some("USD".into()),
        ..Default::default()
    }
}

fn attach(
    current: OfficialQuotaWindow,
    captured_at: &str,
    prev: Option<(OfficialQuotaWindow, &str)>,
    now: &str,
) -> Option<QuotaExhaustDto> {
    let mut windows = vec![current];
    let (prev_windows, prev_at) = match prev {
        Some((window, stamp)) => (vec![window], Some(stamp)),
        None => (Vec::new(), None),
    };
    exhaust::attach(&mut windows, captured_at, &prev_windows, prev_at, at(now));
    windows.remove(0).exhaust
}

#[test]
fn first_snapshot_has_no_eta() {
    assert!(attach(
        window("session_5h", 40.0, Some("2026-09-01T17:00:00+00:00")),
        "2026-09-01T12:00:00+00:00",
        None,
        "2026-09-01T12:01:00+00:00",
    )
    .is_none());
}

#[test]
fn two_snapshots_project_hit_from_capture_time() {
    let exhaust = attach(
        window("session_5h", 60.0, Some("2026-09-01T17:00:00+00:00")),
        "2026-09-01T12:10:00+00:00",
        Some((
            window("session_5h", 50.0, Some("2026-09-01T17:00:00+00:00")),
            "2026-09-01T12:00:00+00:00",
        )),
        "2026-09-01T12:12:00+00:00",
    )
    .expect("应有撞线估计");
    assert_eq!(exhaust.kind, QuotaExhaustKind::Hits);
    let eta = at(exhaust.at.as_deref().expect("hits 应带时刻"));
    // 10 分钟涨 10 个百分点 → 1%/分，还剩 40% → 从 12:10 起 40 分钟。
    assert_eq!(eta, at("2026-09-01T12:50:00+00:00"));
}

#[test]
fn interval_under_a_minute_is_ignored() {
    assert!(attach(
        window("session_5h", 55.0, Some("2026-09-01T17:00:00+00:00")),
        "2026-09-01T12:00:30+00:00",
        Some((
            window("session_5h", 50.0, Some("2026-09-01T17:00:00+00:00")),
            "2026-09-01T12:00:00+00:00",
        )),
        "2026-09-01T12:00:40+00:00",
    )
    .is_none());
}

#[test]
fn reset_change_or_percent_drop_starts_over() {
    assert!(attach(
        window("session_5h", 10.0, Some("2026-09-01T22:00:00+00:00")),
        "2026-09-01T17:10:00+00:00",
        Some((
            window("session_5h", 90.0, Some("2026-09-01T17:00:00+00:00")),
            "2026-09-01T16:50:00+00:00",
        )),
        "2026-09-01T17:11:00+00:00",
    )
    .is_none());
    assert!(attach(
        window("session_5h", 20.0, Some("2026-09-01T17:00:00+00:00")),
        "2026-09-01T12:20:00+00:00",
        Some((
            window("session_5h", 40.0, Some("2026-09-01T17:00:00+00:00")),
            "2026-09-01T12:00:00+00:00",
        )),
        "2026-09-01T12:21:00+00:00",
    )
    .is_none());
}

#[test]
fn hit_after_reset_is_will_not_hit() {
    let exhaust = attach(
        window("session_5h", 20.0, Some("2026-09-01T12:40:00+00:00")),
        "2026-09-01T12:20:00+00:00",
        Some((
            window("session_5h", 10.0, Some("2026-09-01T12:40:00+00:00")),
            "2026-09-01T12:00:00+00:00",
        )),
        "2026-09-01T12:21:00+00:00",
    )
    .expect("应有本窗打不满");
    // 20 分钟涨 10 个百分点 → 0.5%/分，还剩 80% → 160 分钟，远晚于 12:40 重置。
    assert_eq!(exhaust.kind, QuotaExhaustKind::WillNotHit);
    assert!(exhaust.at.is_none());
}

#[test]
fn flat_percent_with_reset_is_will_not_hit() {
    let exhaust = attach(
        window("session_5h", 40.0, Some("2026-09-01T17:00:00+00:00")),
        "2026-09-01T12:10:00+00:00",
        Some((
            window("session_5h", 40.0, Some("2026-09-01T17:00:00+00:00")),
            "2026-09-01T12:00:00+00:00",
        )),
        "2026-09-01T12:11:00+00:00",
    )
    .expect("应有本窗打不满");
    assert_eq!(exhaust.kind, QuotaExhaustKind::WillNotHit);
}

#[test]
fn unused_window_stays_quiet() {
    assert!(attach(
        window("session_5h", 0.0, Some("2026-09-01T17:00:00+00:00")),
        "2026-09-01T12:10:00+00:00",
        Some((
            window("session_5h", 0.0, Some("2026-09-01T17:00:00+00:00")),
            "2026-09-01T12:00:00+00:00",
        )),
        "2026-09-01T12:11:00+00:00",
    )
    .is_none());
}

#[test]
fn already_full_does_not_need_a_previous_snapshot() {
    let exhaust = attach(
        window("session_5h", 100.0, Some("2026-09-01T17:00:00+00:00")),
        "2026-09-01T12:00:00+00:00",
        None,
        "2026-09-01T12:01:00+00:00",
    )
    .expect("已打满");
    assert_eq!(exhaust.kind, QuotaExhaustKind::Exhausted);
    assert!(exhaust.at.is_none());
}

#[test]
fn amount_windows_use_used_over_limit() {
    let exhaust = attach(
        amount_window(30.0, 50.0),
        "2026-09-01T12:10:00+00:00",
        Some((amount_window(20.0, 50.0), "2026-09-01T12:00:00+00:00")),
        "2026-09-01T12:12:00+00:00",
    )
    .expect("金额窗也应估计");
    assert_eq!(exhaust.kind, QuotaExhaustKind::Hits);
    // 20/50 → 30/50 即 40% → 60%，10 分钟涨 20 个百分点，还剩 40% → 12:30。
    assert_eq!(at(&exhaust.at.unwrap()), at("2026-09-01T12:30:00+00:00"));
}

#[test]
fn load_dto_attaches_exhaust_from_stored_prev_snapshot() {
    let conn = store::open_memory().unwrap();
    let reset = "2026-09-01T17:00:00+00:00";
    quota::apply_fetch_results(
        &conn,
        [(
            crate::domain::OfficialQuotaProvider::Claude,
            Ok((
                vec![window("session_5h", 50.0, Some(reset))],
                "2026-09-01T12:00:00+00:00".into(),
            )
                .into()),
        )],
    )
    .unwrap();
    quota::apply_fetch_results(
        &conn,
        [(
            crate::domain::OfficialQuotaProvider::Claude,
            Ok((
                vec![window("session_5h", 60.0, Some(reset))],
                "2026-09-01T12:10:00+00:00".into(),
            )
                .into()),
        )],
    )
    .unwrap();
    let stored = store::load_official_quota_row(&conn, "claude")
        .unwrap()
        .unwrap();
    assert_eq!(stored.windows[0].used_percent, Some(60.0));
    assert_eq!(stored.prev_windows[0].used_percent, Some(50.0));
    assert_eq!(
        stored.prev_captured_at.as_deref(),
        Some("2026-09-01T12:00:00+00:00")
    );
    assert!(stored.windows[0].exhaust.is_none());

    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[],
        at("2026-09-01T12:12:00+00:00"),
    );
    let claude = dto
        .rows
        .iter()
        .find(|row| row.provider == "claude")
        .expect("Claude 行");
    let exhaust = claude.windows[0].exhaust.as_ref().expect("DTO 应带撞线");
    assert_eq!(exhaust.kind, QuotaExhaustKind::Hits);
    assert_eq!(
        at(exhaust.at.as_deref().unwrap()),
        at("2026-09-01T12:50:00+00:00")
    );
}

#[test]
fn fetch_failure_keeps_prev_snapshot() {
    let conn = store::open_memory().unwrap();
    let reset = "2026-09-01T17:00:00+00:00";
    quota::apply_success(
        &conn,
        crate::domain::OfficialQuotaProvider::Claude,
        vec![window("session_5h", 50.0, Some(reset))],
        "2026-09-01T12:00:00+00:00",
    )
    .unwrap();
    quota::apply_success(
        &conn,
        crate::domain::OfficialQuotaProvider::Claude,
        vec![window("session_5h", 60.0, Some(reset))],
        "2026-09-01T12:10:00+00:00",
    )
    .unwrap();
    quota::apply_failure(
        &conn,
        crate::domain::OfficialQuotaProvider::Claude,
        "解析失败",
    )
    .unwrap();
    let stored = store::load_official_quota_row(&conn, "claude")
        .unwrap()
        .unwrap();
    assert_eq!(stored.windows[0].used_percent, Some(60.0));
    assert_eq!(stored.prev_windows[0].used_percent, Some(50.0));
    assert_eq!(stored.error.as_deref(), Some("解析失败"));
}

#[test]
fn same_captured_at_does_not_rotate_prev() {
    let conn = store::open_memory().unwrap();
    quota::apply_success(
        &conn,
        crate::domain::OfficialQuotaProvider::Claude,
        vec![window("session_5h", 40.0, None)],
        "2026-09-01T12:00:00+00:00",
    )
    .unwrap();
    quota::apply_success(
        &conn,
        crate::domain::OfficialQuotaProvider::Claude,
        vec![window("session_5h", 41.0, None)],
        "2026-09-01T12:00:00+00:00",
    )
    .unwrap();
    let stored = store::load_official_quota_row(&conn, "claude")
        .unwrap()
        .unwrap();
    assert!(stored.prev_windows.is_empty());
    assert!(stored.prev_captured_at.is_none());
}

#[test]
fn eta_is_measured_from_capture_not_from_now() {
    let exhaust = attach(
        window("session_5h", 60.0, Some("2026-09-01T17:00:00+00:00")),
        "2026-09-01T12:10:00+00:00",
        Some((
            window("session_5h", 50.0, Some("2026-09-01T17:00:00+00:00")),
            "2026-09-01T12:00:00+00:00",
        )),
        "2026-09-01T12:18:00+00:00",
    )
    .unwrap();
    // now 已经 12:18，时刻仍按快照 12:10 + 40 分钟 = 12:50，不把闲置时间加进去。
    assert_eq!(at(&exhaust.at.unwrap()), at("2026-09-01T12:50:00+00:00"));
}
