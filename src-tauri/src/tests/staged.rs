use crate::domain::ConversationCatalogProgressDto;
use crate::ingest::CatalogProgress;
use crate::staged::{
    catalog_progress_dto, catalog_stage_failure, Flight, OneShot, PriorityQueue, SingleFlight,
    WriteWaiters,
};
use crate::test_support::*;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn key(source: &str, id: &str) -> (String, String) {
    (source.to_string(), id.to_string())
}

#[test]
fn single_flight_starts_once_and_folds_requests_into_one_rerun() {
    let mut flight = SingleFlight::default();
    assert!(flight.request(), "空闲时第一次请求要启动一轮");
    assert!(!flight.request());
    assert!(!flight.request());

    assert!(flight.finish(), "运行中被请求过，结束时要补跑一轮");
    assert!(!flight.finish(), "补跑只记一次，不随请求次数累积");

    assert!(flight.request(), "补跑结束并空闲后，可以重新启动");
}

#[test]
fn single_flight_without_requests_goes_idle_after_one_run() {
    let mut flight = SingleFlight::default();
    assert!(flight.request());
    assert!(!flight.finish());
    assert!(flight.request());
}

#[test]
fn rerun_requested_during_the_rerun_is_honoured() {
    let mut flight = SingleFlight::default();
    assert!(flight.request());
    assert!(!flight.request());
    assert!(flight.finish());
    assert!(!flight.request(), "补跑期间仍是运行状态");
    assert!(flight.finish());
    assert!(!flight.finish());
}

#[test]
fn priority_queue_puts_the_latest_request_first_without_duplicates() {
    let mut queue = PriorityQueue::default();
    assert!(queue.is_empty());
    queue.push_front(key("codex", "a"));
    queue.push_front(key("claude", "b"));
    queue.push_front(key("codex", "c"));
    assert_eq!(
        queue.snapshot(),
        vec![key("codex", "c"), key("claude", "b"), key("codex", "a")]
    );

    queue.push_front(key("codex", "a"));
    assert_eq!(
        queue.snapshot(),
        vec![key("codex", "a"), key("codex", "c"), key("claude", "b")]
    );
}

#[test]
fn priority_queue_removes_entries_and_caps_its_size() {
    let mut queue = PriorityQueue::default();
    queue.push_front(key("codex", "a"));
    queue.push_front(key("codex", "b"));
    queue.remove(&key("codex", "a"));
    queue.remove(&key("codex", "missing"));
    assert_eq!(queue.snapshot(), vec![key("codex", "b")]);
    queue.remove(&key("codex", "b"));
    assert!(queue.is_empty());

    for index in 0..100 {
        queue.push_front(key("codex", &index.to_string()));
    }
    let snapshot = queue.snapshot();
    assert!(snapshot.len() < 100);
    assert_eq!(snapshot[0], key("codex", "99"), "最新的留在最前");
    assert!(!snapshot.contains(&key("codex", "0")), "最旧的被挤掉");
}

#[test]
fn one_shot_fires_exactly_once() {
    let shot = OneShot::default();
    assert!(shot.try_fire());
    assert!(!shot.try_fire());
    assert!(!shot.try_fire());
}

#[test]
fn one_shot_fires_once_across_threads() {
    let shot = std::sync::Arc::new(OneShot::default());
    let fired: usize = (0..8)
        .map(|_| {
            let shot = shot.clone();
            std::thread::spawn(move || shot.try_fire())
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|handle| handle.join().unwrap() as usize)
        .sum();
    assert_eq!(fired, 1);
}

#[test]
fn progress_dto_matches_the_event_contract() {
    let step = CatalogProgress {
        done: 3,
        total: 21,
        source: Some(Source::Codex),
        finished: false,
    };
    assert_eq!(
        catalog_progress_dto(&step, 0),
        ConversationCatalogProgressDto {
            done: 3,
            total: 21,
            source: Some("codex".to_string()),
            finished: false,
            issue_count: 0,
        }
    );
    let json = serde_json::to_value(catalog_progress_dto(
        &CatalogProgress {
            done: 21,
            total: 21,
            source: None,
            finished: true,
        },
        2,
    ))
    .unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "done": 21,
            "total": 21,
            "source": null,
            "finished": true,
            "issue_count": 2,
        })
    );
}

#[test]
fn stage_failure_is_reported_as_one_issue() {
    let issue = catalog_stage_failure("boom".to_string());
    assert_eq!(issue.message, "boom");
    assert!(issue.source.is_empty());
}

#[test]
fn flight_runs_requested_rerun_in_the_same_loop() {
    let flight = Flight::default();
    assert!(flight.request());
    let mut rounds = 0;
    flight.run_rounds(|| {
        rounds += 1;
        if rounds == 1 {
            assert!(!flight.request(), "运行中的请求折成补跑");
        }
    });
    assert_eq!(rounds, 2);
    assert!(flight.request(), "跑完后回到空闲");
}

#[test]
fn flight_is_released_when_a_round_panics() {
    let flight = Flight::default();
    assert!(flight.request());
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        flight.run_rounds(|| panic!("boom"));
    }));
    assert!(outcome.is_err());
    assert!(flight.request(), "panic 之后单飞位必须已放开");
}

#[test]
fn wait_idle_blocks_until_the_running_round_finishes() {
    let flight = Arc::new(Flight::default());
    assert!(flight.request());
    let waiter = {
        let flight = Arc::clone(&flight);
        std::thread::spawn(move || {
            let started = Instant::now();
            flight.wait_idle();
            started.elapsed()
        })
    };
    std::thread::sleep(Duration::from_millis(50));
    flight.run_rounds(|| {});
    assert!(waiter.join().unwrap() >= Duration::from_millis(40));
}

#[test]
fn wait_idle_returns_at_once_when_nothing_runs() {
    Flight::default().wait_idle();
}

#[test]
fn waiters_make_the_catalog_yield_until_they_have_the_lock() {
    let waiters = WriteWaiters::default();
    let waiting = waiters.enter();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            drop(waiting);
        });
        let started = Instant::now();
        waiters.yield_to_waiters(Duration::from_millis(2), Duration::from_secs(5));
        assert!(started.elapsed() >= Duration::from_millis(25));
        assert!(started.elapsed() < Duration::from_secs(4));
    });
}

#[test]
fn yielding_to_a_stuck_waiter_is_bounded_and_free_without_waiters() {
    let waiters = WriteWaiters::default();
    let started = Instant::now();
    waiters.yield_to_waiters(Duration::from_millis(2), Duration::from_secs(5));
    assert!(started.elapsed() < Duration::from_millis(100));

    let _stuck = waiters.enter();
    let started = Instant::now();
    waiters.yield_to_waiters(Duration::from_millis(2), Duration::from_millis(40));
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(40));
    assert!(elapsed < Duration::from_secs(2));
}
