//! 推送台账（ADR 0026）：已推成功且内容没变的会话、已推过的消耗记录不重复发送；
//! 变了、失败了、强制时照推。

use push_protocol::{ApiErrorCode, PushUsageRequest};

use super::push::{
    login_reply, no_progress, pushed_requests, range, refreshed, seed_codex, seed_three_sessions,
    session_ok, usage_ok, usage_row, Harness,
};
use crate::push::ledger::{self, Ledger, UsageLedger};
use crate::push::{self, PushRunInput};
use crate::remote_server::store::RemoteServerPaths;
use crate::test_support::http_stub::{api_error, serve, Stub};
use crate::test_support::*;

fn input(force: bool) -> PushRunInput {
    PushRunInput {
        range: range(None, None),
        only: Vec::new(),
        include_usage: false,
        force,
    }
}

fn pushed_ids(stub: &Stub) -> Vec<String> {
    pushed_requests(stub)
        .into_iter()
        .map(|r| r.session.session_id)
        .collect()
}

#[test]
fn second_push_skips_sessions_that_have_not_changed() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let h = Harness::new(home, refreshed(home));
    let stub = serve(vec![
        login_reply(),
        session_ok("a"),
        session_ok("b"),
        session_ok("c"),
    ]);
    h.login(&stub);

    let first = push::run(&h.env(), &input(false), &no_progress).unwrap();
    assert_eq!((first.sessions_succeeded, first.sessions_unchanged), (3, 0));

    let second = push::run(&h.env(), &input(false), &no_progress).unwrap();
    assert_eq!(
        (second.sessions_succeeded, second.sessions_unchanged),
        (0, 3)
    );
    assert!(second.failed.is_empty());
    assert_eq!(
        pushed_ids(&stub),
        ["a", "b", "c"],
        "第二次一个会话请求都不该发"
    );
}

#[test]
fn changed_session_is_pushed_again_and_the_rest_are_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let h = Harness::new(home, refreshed(home));
    let stub = serve(vec![
        login_reply(),
        session_ok("a"),
        session_ok("b"),
        session_ok("c"),
        session_ok("b"),
    ]);
    h.login(&stub);
    push::run(&h.env(), &input(false), &no_progress).unwrap();

    // 会话 b 后来又聊了几句：本机重新摄取后内容不同。
    seed_codex(
        home,
        "b",
        "/w/b",
        "2026-09-02T10:00:00Z",
        "2026-09-02T11:00:00Z",
        "q",
        "a，后来补充的回答",
    );
    let mut later = Harness::new(home, refreshed(home));
    later.remote = RemoteServerPaths::in_dir(h.dir_path());
    later.history = h.history.clone();

    let outcome = push::run(&later.env(), &input(false), &no_progress).unwrap();
    assert_eq!(
        (outcome.sessions_succeeded, outcome.sessions_unchanged),
        (1, 2)
    );
    assert_eq!(pushed_ids(&stub), ["a", "b", "c", "b"]);
}

#[test]
fn failed_session_is_not_recorded_so_the_next_push_retries_only_it() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let h = Harness::new(home, refreshed(home));
    let stub = serve(vec![
        login_reply(),
        session_ok("a"),
        api_error(500, ApiErrorCode::Internal),
        session_ok("c"),
        session_ok("b"),
    ]);
    h.login(&stub);

    let first = push::run(&h.env(), &input(false), &no_progress).unwrap();
    assert_eq!(first.failed.len(), 1);

    let second = push::run(&h.env(), &input(false), &no_progress).unwrap();
    assert_eq!(
        (second.sessions_succeeded, second.sessions_unchanged),
        (1, 2)
    );
    assert_eq!(pushed_ids(&stub), ["a", "b", "c", "b"]);
}

#[test]
fn force_pushes_everything_again() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let h = Harness::new(home, refreshed(home));
    let stub = serve(vec![
        login_reply(),
        session_ok("a"),
        session_ok("b"),
        session_ok("c"),
        session_ok("a"),
        session_ok("b"),
        session_ok("c"),
    ]);
    h.login(&stub);
    push::run(&h.env(), &input(false), &no_progress).unwrap();

    let forced = push::run(&h.env(), &input(true), &no_progress).unwrap();
    assert_eq!(
        (forced.sessions_succeeded, forced.sessions_unchanged),
        (3, 0)
    );
    assert_eq!(pushed_requests(&stub).len(), 6);
}

#[test]
fn preview_counts_unchanged_sessions_apart_from_what_will_be_sent() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_three_sessions(home);
    let h = Harness::new(home, refreshed(home));
    let stub = serve(vec![
        login_reply(),
        session_ok("a"),
        session_ok("b"),
        session_ok("c"),
    ]);
    h.login(&stub);

    let before = push::preview(&h.env(), &range(None, None)).unwrap();
    assert_eq!((before.sessions, before.unchanged_sessions), (3, 0));

    push::run(&h.env(), &input(false), &no_progress).unwrap();

    let after = push::preview(&h.env(), &range(None, None)).unwrap();
    assert_eq!((after.sessions, after.unchanged_sessions), (0, 3));
    assert_eq!(after.events, 0);
    assert!(
        after.estimated_bytes < before.estimated_bytes,
        "没变化的会话不计入预计大小"
    );
}

#[test]
fn ledger_is_scoped_to_server_account_and_device() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(ledger::LEDGER_FILE);

    let mut mine = Ledger::load(&path, "https://a.example", "alice", "device-1");
    assert!(!mine.is_current("codex", "s1", "fp"));
    mine.record("codex", "s1", "fp".into());
    mine.save().unwrap();

    let same = Ledger::load(&path, "https://a.example", "alice", "device-1");
    assert!(same.is_current("codex", "s1", "fp"));
    assert!(!same.is_current("codex", "s1", "other"), "内容变了不算");
    for (url, account, device) in [
        ("https://b.example", "alice", "device-1"),
        ("https://a.example", "bob", "device-1"),
        ("https://a.example", "alice", "device-2"),
    ] {
        let other = Ledger::load(&path, url, account, device);
        assert!(
            !other.is_current("codex", "s1", "fp"),
            "{url} {account} {device}"
        );
    }
}

#[test]
fn broken_ledger_file_means_nothing_was_pushed_before() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(ledger::LEDGER_FILE);
    std::fs::write(&path, "{ not json").unwrap();
    let ledger = Ledger::load(&path, "https://a.example", "alice", "device-1");
    assert!(!ledger.is_current("codex", "s1", "fp"));
}

// ---- 消耗记录 ----

fn usage_input(force: bool) -> PushRunInput {
    PushRunInput {
        include_usage: true,
        ..input(force)
    }
}

fn usage_requests(stub: &Stub) -> Vec<PushUsageRequest> {
    stub.captured
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.request_line.contains("/api/v1/push/usage"))
        .map(|c| serde_json::from_str(&c.body).unwrap())
        .collect()
}

fn usage_harness(records: &[(&str, &str, i64)]) -> (tempfile::TempDir, Harness) {
    let temp = tempfile::tempdir().unwrap();
    let conn = refreshed(temp.path());
    let rows: Vec<_> = records
        .iter()
        .map(|(time, session, tokens)| usage_row(time, session, *tokens))
        .collect();
    store::insert_records(&conn, &rows).unwrap();
    let h = Harness::new(temp.path(), conn);
    (temp, h)
}

const TWO_USAGE: [(&str, &str, i64); 2] = [
    ("2026-09-01T00:30:00Z", "a", 10),
    ("2026-09-02T00:30:00Z", "b", 20),
];

#[test]
fn second_push_does_not_resend_usage_records_already_pushed() {
    let (_temp, h) = usage_harness(&TWO_USAGE);
    let stub = serve(vec![login_reply(), usage_ok(2, 0)]);
    h.login(&stub);

    let first = push::run(&h.env(), &usage_input(false), &no_progress).unwrap();
    assert_eq!((first.usage_inserted, first.usage_unchanged), (2, 0));

    let second = push::run(&h.env(), &usage_input(false), &no_progress).unwrap();
    assert_eq!(
        (
            second.usage_inserted,
            second.usage_duplicates,
            second.usage_unchanged
        ),
        (0, 0, 2)
    );
    assert!(second.usage_error.is_none());
    assert_eq!(
        usage_requests(&stub).len(),
        1,
        "第二次一条消耗记录都不该再发"
    );
}

#[test]
fn only_new_usage_records_are_sent_next_time() {
    let (temp, h) = usage_harness(&TWO_USAGE);
    let stub = serve(vec![login_reply(), usage_ok(2, 0), usage_ok(1, 0)]);
    h.login(&stub);
    push::run(&h.env(), &usage_input(false), &no_progress).unwrap();

    let conn = refreshed(temp.path());
    let mut rows: Vec<_> = TWO_USAGE
        .iter()
        .map(|(time, session, tokens)| usage_row(time, session, *tokens))
        .collect();
    rows.push(usage_row("2026-09-03T00:30:00Z", "c", 30));
    store::insert_records(&conn, &rows).unwrap();
    let mut later = Harness::new(temp.path(), conn);
    later.remote = RemoteServerPaths::in_dir(h.dir_path());
    later.history = h.history.clone();

    let outcome = push::run(&later.env(), &usage_input(false), &no_progress).unwrap();
    assert_eq!((outcome.usage_inserted, outcome.usage_unchanged), (1, 2));
    let sent = usage_requests(&stub);
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].records.len(), 1);
}

#[test]
fn failed_usage_batch_is_not_recorded_so_it_is_sent_again() {
    let (_temp, h) = usage_harness(&TWO_USAGE);
    let stub = serve(vec![
        login_reply(),
        api_error(500, ApiErrorCode::Internal),
        usage_ok(2, 0),
    ]);
    h.login(&stub);

    let first = push::run(&h.env(), &usage_input(false), &no_progress).unwrap();
    assert!(first.usage_error.is_some());

    let second = push::run(&h.env(), &usage_input(false), &no_progress).unwrap();
    assert_eq!((second.usage_inserted, second.usage_unchanged), (2, 0));
}

#[test]
fn force_sends_pushed_usage_records_again() {
    let (_temp, h) = usage_harness(&TWO_USAGE);
    let stub = serve(vec![login_reply(), usage_ok(2, 0), usage_ok(0, 2)]);
    h.login(&stub);
    push::run(&h.env(), &usage_input(false), &no_progress).unwrap();

    let forced = push::run(&h.env(), &usage_input(true), &no_progress).unwrap();
    assert_eq!((forced.usage_duplicates, forced.usage_unchanged), (2, 0));
    assert_eq!(usage_requests(&stub).len(), 2);
}

#[test]
fn preview_separates_usage_records_already_pushed() {
    let (_temp, h) = usage_harness(&TWO_USAGE);
    let stub = serve(vec![login_reply(), usage_ok(2, 0)]);
    h.login(&stub);

    let before = push::preview(&h.env(), &range(None, None)).unwrap();
    assert_eq!((before.usage_records, before.usage_already_pushed), (2, 0));

    push::run(&h.env(), &usage_input(false), &no_progress).unwrap();

    let after = push::preview(&h.env(), &range(None, None)).unwrap();
    assert_eq!((after.usage_records, after.usage_already_pushed), (0, 2));
    assert!(after.estimated_bytes < before.estimated_bytes);
}

#[test]
fn usage_ledger_is_scoped_and_keeps_other_scopes_when_saving() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(ledger::USAGE_LEDGER_FILE);
    let fingerprint = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    let mut mine = UsageLedger::load(&path, "https://a.example", "alice", "device-1");
    assert!(!mine.contains(fingerprint));
    mine.record([fingerprint]);
    mine.save().unwrap();

    let mut other = UsageLedger::load(&path, "https://b.example", "alice", "device-1");
    assert!(!other.contains(fingerprint), "换服务器不作数");
    other.record(["ffffffffffffffffffffffffffffffff"]);
    other.save().unwrap();

    let again = UsageLedger::load(&path, "https://a.example", "alice", "device-1");
    assert!(again.contains(fingerprint), "别的范围写盘不能冲掉我的记录");
}
