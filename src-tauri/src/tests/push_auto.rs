//! 每日自动推送（ADR 0026）：默认关、补推区间、只在干净推完后推进进度、未登录不推并提示。

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use push_protocol::{ApiErrorCode, PushSessionRequest};

use super::push::{login_reply, refreshed, seed_codex, session_ok, usage_ok, usage_row, Harness};
use crate::push::auto::{self, AutoPushState, AutoPushStatus, MAX_CATCH_UP_DAYS};
use crate::push::{history, PushEnv};
use crate::remote_server::SessionState;
use crate::test_support::http_stub::{api_error, serve, Stub};
use crate::test_support::*;

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

fn at(text: &str) -> DateTime<Utc> {
    text.parse().unwrap()
}

fn utc() -> FixedOffset {
    FixedOffset::east_opt(0).unwrap()
}

fn state(enabled: bool, pushed_through: Option<&str>) -> AutoPushState {
    AutoPushState {
        enabled,
        pushed_through: pushed_through.map(str::to_string),
        ..Default::default()
    }
}

// ---- 默认关 ----

#[test]
fn off_by_default_and_nothing_is_due() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("push_auto.json");
    let loaded = auto::load(&path);
    assert!(!loaded.enabled, "文件不存在时必须是关");
    assert_eq!(auto::pending_days(&loaded, day("2026-10-10")), None);

    std::fs::write(&path, "{ not json").unwrap();
    assert!(!auto::load(&path).enabled, "文件坏了也按关处理");
    std::fs::write(&path, "{}").unwrap();
    assert!(!auto::load(&path).enabled, "缺字段按关处理");
}

#[test]
fn run_due_does_nothing_when_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let h = Harness::new(temp.path(), refreshed(temp.path()));
    let stub = serve(vec![]);
    let state_path = h.dir_path().join("push_auto.json");

    let status = run_due(&h, &state_path, "2026-10-10T08:00:00Z", &stub);

    assert!(matches!(status, AutoPushStatus::Disabled));
    assert!(stub.captured.lock().unwrap().is_empty());
    assert!(history::load(&h.history).is_empty());
}

// ---- 补推区间 ----

#[test]
fn first_enable_pushes_only_yesterday() {
    assert_eq!(
        auto::pending_days(&state(true, None), day("2026-10-10")),
        Some((day("2026-10-09"), day("2026-10-09")))
    );
}

#[test]
fn nothing_pending_once_yesterday_is_covered() {
    let today = day("2026-10-10");
    assert_eq!(
        auto::pending_days(&state(true, Some("2026-10-09")), today),
        None
    );
    assert_eq!(
        auto::pending_days(&state(true, Some("2026-10-20")), today),
        None,
        "进度在未来（改过系统时间）也不推"
    );
}

#[test]
fn missed_days_are_caught_up_in_one_range() {
    assert_eq!(
        auto::pending_days(&state(true, Some("2026-10-06")), day("2026-10-10")),
        Some((day("2026-10-07"), day("2026-10-09")))
    );
}

#[test]
fn catch_up_is_capped() {
    let (first, last) =
        auto::pending_days(&state(true, Some("2025-01-01")), day("2026-10-10")).unwrap();
    assert_eq!(last, day("2026-10-09"));
    assert_eq!((last - first).num_days() + 1, MAX_CATCH_UP_DAYS);
}

#[test]
fn unreadable_cursor_falls_back_to_yesterday() {
    assert_eq!(
        auto::pending_days(&state(true, Some("昨天")), day("2026-10-10")),
        Some((day("2026-10-09"), day("2026-10-09")))
    );
}

#[test]
fn range_covers_whole_local_days_expressed_in_utc() {
    let utc_range = auto::range_for_days(&utc(), day("2026-10-07"), day("2026-10-09"));
    assert_eq!(utc_range.from.as_deref(), Some("2026-10-07T00:00:00Z"));
    assert_eq!(utc_range.to.as_deref(), Some("2026-10-09T23:59:59Z"));
    assert!(utc_range.sources.is_empty(), "自动推送不按来源筛，推全部");

    let china = FixedOffset::east_opt(8 * 3600).unwrap();
    let local = auto::range_for_days(&china, day("2026-10-09"), day("2026-10-09"));
    assert_eq!(local.from.as_deref(), Some("2026-10-08T16:00:00Z"));
    assert_eq!(local.to.as_deref(), Some("2026-10-09T15:59:59Z"));
}

// ---- 开关 ----

#[test]
fn enabling_starts_fresh_and_disabling_keeps_progress() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("push_auto.json");

    let on = auto::set_enabled(&path, true).unwrap();
    assert!(on.enabled && on.pushed_through.is_none());

    auto::record_attempt(
        &path,
        at("2026-10-10T08:00:00Z"),
        Some(day("2026-10-09")),
        None,
    )
    .unwrap();
    let off = auto::set_enabled(&path, false).unwrap();
    assert!(!off.enabled);
    assert_eq!(off.pushed_through.as_deref(), Some("2026-10-09"));

    let again = auto::set_enabled(&path, true).unwrap();
    assert!(
        again.pushed_through.is_none(),
        "关着的那些天不补：重新打开从昨天算起"
    );
    assert!(auto::load(&path).enabled, "开关落盘");
}

// ---- 推送 ----

fn run_due(h: &Harness, state_path: &std::path::Path, now: &str, _stub: &Stub) -> AutoPushStatus {
    let env = PushEnv {
        conns: &h.conns,
        home: &h.home,
        prices: &h.prices,
        remote: &h.remote,
        history: &h.history,
        now: at(now),
    };
    auto::run_due(&env, state_path, &utc(), &|| Ok(()))
}

fn session_requests(stub: &Stub) -> Vec<PushSessionRequest> {
    stub.captured
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.request_line.contains("/api/v1/push/session"))
        .map(|c| serde_json::from_str(&c.body).unwrap())
        .collect()
}

fn seeded(temp: &std::path::Path) -> rusqlite::Connection {
    for (id, start, end) in [
        ("old", "2026-10-05T10:00:00Z", "2026-10-05T11:00:00Z"),
        ("d7", "2026-10-07T10:00:00Z", "2026-10-07T11:00:00Z"),
        ("d9", "2026-10-09T10:00:00Z", "2026-10-09T11:00:00Z"),
        ("today", "2026-10-10T01:00:00Z", "2026-10-10T02:00:00Z"),
    ] {
        seed_codex(temp, id, "/w/a", start, end, "q", "a");
    }
    let conn = refreshed(temp);
    store::insert_records(
        &conn,
        &[
            usage_row("2026-10-05T10:30:00Z", "old", 1),
            usage_row("2026-10-09T10:30:00Z", "d9", 2),
            usage_row("2026-10-10T01:30:00Z", "today", 3),
        ],
    )
    .unwrap();
    conn
}

#[test]
fn pushes_yesterday_only_records_history_and_advances_once() {
    let temp = tempfile::tempdir().unwrap();
    let h = Harness::new(temp.path(), seeded(temp.path()));
    let stub = serve(vec![login_reply(), session_ok("d9"), usage_ok(1, 0)]);
    h.login(&stub);
    let state_path = h.dir_path().join("push_auto.json");
    auto::set_enabled(&state_path, true).unwrap();

    let status = run_due(&h, &state_path, "2026-10-10T08:00:00Z", &stub);

    let AutoPushStatus::Pushed(outcome) = status else {
        panic!("应该推了");
    };
    assert_eq!(outcome.sessions_succeeded, 1);
    assert_eq!(outcome.usage_inserted, 1);
    let sent = session_requests(&stub);
    assert_eq!(sent.len(), 1, "只推昨天与昨天重叠的会话");
    assert_eq!(sent[0].session.session_id, "d9");

    let entries = history::load(&h.history);
    assert_eq!(entries.len(), 1, "结果写进推送历史");
    assert!(entries[0].automatic);
    assert_eq!(entries[0].from.as_deref(), Some("2026-10-09T00:00:00Z"));
    assert_eq!(entries[0].to.as_deref(), Some("2026-10-09T23:59:59Z"));

    let saved = auto::load(&state_path);
    assert_eq!(saved.pushed_through.as_deref(), Some("2026-10-09"));
    assert!(saved.last_error.is_none());

    let again = run_due(&h, &state_path, "2026-10-10T09:00:00Z", &stub);
    assert!(matches!(again, AutoPushStatus::UpToDate));
    assert_eq!(stub.captured.lock().unwrap().len(), 3, "同一天不重复推");
}

#[test]
fn days_missed_while_app_was_closed_are_pushed_on_next_start() {
    let temp = tempfile::tempdir().unwrap();
    let h = Harness::new(temp.path(), seeded(temp.path()));
    let stub = serve(vec![
        login_reply(),
        session_ok("d7"),
        session_ok("d9"),
        usage_ok(1, 0),
    ]);
    h.login(&stub);
    let state_path = h.dir_path().join("push_auto.json");
    auto::set_enabled(&state_path, true).unwrap();
    auto::record_attempt(
        &state_path,
        at("2026-10-06T08:00:00Z"),
        Some(day("2026-10-06")),
        None,
    )
    .unwrap();

    let status = run_due(&h, &state_path, "2026-10-10T08:00:00Z", &stub);

    assert!(matches!(status, AutoPushStatus::Pushed(_)));
    let ids: Vec<_> = session_requests(&stub)
        .into_iter()
        .map(|r| r.session.session_id)
        .collect();
    assert_eq!(
        ids,
        ["d7", "d9"],
        "10-07 到 10-09 一次补齐，更早与今天的不碰"
    );
    assert_eq!(
        auto::load(&state_path).pushed_through.as_deref(),
        Some("2026-10-09")
    );
}

#[test]
fn failed_push_keeps_progress_so_the_next_run_retries() {
    let temp = tempfile::tempdir().unwrap();
    let h = Harness::new(temp.path(), seeded(temp.path()));
    let stub = serve(vec![
        login_reply(),
        api_error(500, ApiErrorCode::Internal),
        usage_ok(1, 0),
        session_ok("d9"),
        usage_ok(0, 1),
    ]);
    h.login(&stub);
    let state_path = h.dir_path().join("push_auto.json");
    auto::set_enabled(&state_path, true).unwrap();

    let first = run_due(&h, &state_path, "2026-10-10T08:00:00Z", &stub);
    let AutoPushStatus::Pushed(outcome) = first else {
        panic!("应该尝试过");
    };
    assert_eq!(outcome.failed.len(), 1);
    let saved = auto::load(&state_path);
    assert!(saved.pushed_through.is_none(), "有失败就不能算推完");
    assert!(saved.last_error.is_some(), "设置页要能看到失败");
    assert_eq!(history::load(&h.history).len(), 1, "失败也写进历史");

    let second = run_due(&h, &state_path, "2026-10-10T09:00:00Z", &stub);
    assert!(matches!(second, AutoPushStatus::Pushed(_)));
    let saved = auto::load(&state_path);
    assert_eq!(saved.pushed_through.as_deref(), Some("2026-10-09"));
    assert!(saved.last_error.is_none());
}

#[test]
fn failed_local_refresh_pushes_nothing_and_keeps_progress() {
    let temp = tempfile::tempdir().unwrap();
    let h = Harness::new(temp.path(), seeded(temp.path()));
    let stub = serve(vec![login_reply()]);
    h.login(&stub);
    let state_path = h.dir_path().join("push_auto.json");
    auto::set_enabled(&state_path, true).unwrap();
    let env = PushEnv {
        conns: &h.conns,
        home: &h.home,
        prices: &h.prices,
        remote: &h.remote,
        history: &h.history,
        now: at("2026-10-10T08:00:00Z"),
    };

    let status = auto::run_due(&env, &state_path, &utc(), &|| Err("磁盘忙".to_string()));

    assert!(matches!(status, AutoPushStatus::Failed(m) if m.contains("磁盘忙")));
    assert_eq!(stub.captured.lock().unwrap().len(), 1, "只有登录那一次请求");
    let saved = auto::load(&state_path);
    assert!(saved.pushed_through.is_none(), "没推过就不能推进进度");
    assert!(saved.last_error.is_some());
}

#[test]
fn not_logged_in_pushes_nothing_and_tells_the_settings_page() {
    let temp = tempfile::tempdir().unwrap();
    let h = Harness::new(temp.path(), seeded(temp.path()));
    let stub = serve(vec![]);
    let state_path = h.dir_path().join("push_auto.json");
    auto::set_enabled(&state_path, true).unwrap();

    let status = run_due(&h, &state_path, "2026-10-10T08:00:00Z", &stub);

    let AutoPushStatus::NotLoggedIn(message) = status else {
        panic!("未登录不该推");
    };
    assert!(message.contains("登录"));
    assert!(stub.captured.lock().unwrap().is_empty());
    assert!(history::load(&h.history).is_empty(), "没推就不记历史");
    assert!(auto::load(&state_path).pushed_through.is_none());

    let panel = auto::panel(&state_path, &h.remote, at("2026-10-10T08:00:00Z"));
    assert!(panel.enabled);
    assert!(panel
        .login_notice
        .as_deref()
        .is_some_and(|n| n.contains("登录")));
}

#[test]
fn expired_login_is_reported_and_does_not_push() {
    let temp = tempfile::tempdir().unwrap();
    let h = Harness::new(temp.path(), seeded(temp.path()));
    let stub = serve(vec![login_reply()]);
    h.login(&stub);
    let state_path = h.dir_path().join("push_auto.json");
    auto::set_enabled(&state_path, true).unwrap();

    let later = "2026-12-31T08:00:00Z";
    let status = run_due(&h, &state_path, later, &stub);

    assert!(matches!(status, AutoPushStatus::NotLoggedIn(_)));
    assert_eq!(stub.captured.lock().unwrap().len(), 1, "只有登录那一次请求");
    let panel = auto::panel(&state_path, &h.remote, at(later));
    assert_eq!(panel.session_state, SessionState::Expired);
    assert!(panel.login_notice.is_some());
}

#[test]
fn token_rejected_mid_push_keeps_progress_and_asks_for_relogin() {
    let temp = tempfile::tempdir().unwrap();
    let h = Harness::new(temp.path(), seeded(temp.path()));
    let stub = serve(vec![
        login_reply(),
        api_error(401, ApiErrorCode::TokenExpired),
    ]);
    h.login(&stub);
    let state_path = h.dir_path().join("push_auto.json");
    auto::set_enabled(&state_path, true).unwrap();

    let status = run_due(&h, &state_path, "2026-10-10T08:00:00Z", &stub);

    assert!(matches!(status, AutoPushStatus::Pushed(o) if o.login_required));
    assert!(auto::load(&state_path).pushed_through.is_none());
    let panel = auto::panel(&state_path, &h.remote, at("2026-10-10T08:00:00Z"));
    assert_eq!(panel.session_state, SessionState::Rejected);
    assert!(panel.login_notice.is_some());
}

#[test]
fn panel_has_no_login_notice_when_off_or_logged_in() {
    let temp = tempfile::tempdir().unwrap();
    let h = Harness::new(temp.path(), refreshed(temp.path()));
    let stub = serve(vec![login_reply()]);
    let state_path = h.dir_path().join("push_auto.json");
    let now = at("2026-10-10T08:00:00Z");

    let off = auto::panel(&state_path, &h.remote, now);
    assert!(!off.enabled);
    assert!(off.login_notice.is_none(), "没开就不唠叨登录");

    h.login(&stub);
    auto::set_enabled(&state_path, true).unwrap();
    let on = auto::panel(&state_path, &h.remote, now);
    assert!(on.enabled && on.login_notice.is_none());
}
