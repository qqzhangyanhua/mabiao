//! #90：自定义提供商接入取数退避。
//!
//! 接缝是退避状态（字符串标识）和手动刷新入口。喂 tempfile 里的状态文件
//! 与取数结果，断言冷却时长、手动刷新的人话提示、以及「没打网的失败不进退避」。
//! 不联网、不读真实用户目录。

use chrono::{DateTime, Duration, Utc};

use super::{resolved, unresolved, SUBSCRIPTION, USAGE};
use crate::domain::OfficialQuotaConfig;
use crate::official_quota::backoff::{self, BackoffState};
use crate::official_quota::custom::{self, CustomQuotaPreset};
use crate::official_quota::fetch;
use crate::official_quota::{self as quota};
use crate::store as db;

fn at(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .unwrap()
        .with_timezone(&Utc)
}

fn now() -> DateTime<Utc> {
    at("2026-08-24T12:00:00Z")
}

fn custom_target() -> quota::FetchTarget {
    quota::FetchTarget::Custom(Box::new(resolved("custom:a3f9c1", "公司的中转")))
}

fn seeded_cooldown(id: &str, error: &str) -> (tempfile::TempDir, std::path::PathBuf, BackoffState) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(backoff::STATE_NAME);
    let mut state = BackoffState::default();
    backoff::record_failure(&mut state, id, error, now());
    backoff::save_state(&path, &state).unwrap();
    (dir, path, state)
}

// -------------------------------------------------- 连续失败 / 限流 / 成功清掉

/// 自定义提供商走同一套翻倍与封顶。限流起步 10、封顶 60；普通失败起步 1、封顶 15。
#[test]
fn custom_failures_double_then_cap_like_builtin_accounts() {
    let mut state = BackoffState::default();
    let limited = "对方限流了，稍后会自动重试";
    let ordinary = "网络不通，连不上这个地址，请检查网络或代理设置";

    let mut limited_seen = Vec::new();
    let mut ordinary_seen = Vec::new();
    for _ in 0..8 {
        backoff::record_failure(&mut state, "custom:a3f9c1", limited, now());
        backoff::record_failure(&mut state, "custom:b7e204", ordinary, now());
        limited_seen.push(
            backoff::cooldown_remaining(&state, "custom:a3f9c1", now())
                .unwrap()
                .num_minutes(),
        );
        ordinary_seen.push(
            backoff::cooldown_remaining(&state, "custom:b7e204", now())
                .unwrap()
                .num_minutes(),
        );
    }
    assert_eq!(limited_seen[0], 10);
    assert_eq!(limited_seen[1], 20);
    assert_eq!(limited_seen[2], 40);
    assert!(limited_seen[3..].iter().all(|minutes| *minutes == 60));
    assert_eq!(ordinary_seen[0], 1);
    assert_eq!(ordinary_seen[1], 2);
    assert_eq!(ordinary_seen[2], 4);
    assert_eq!(ordinary_seen[3], 8);
    assert!(ordinary_seen[4..].iter().all(|minutes| *minutes == 15));
}

/// 限流类失败的基数与上限高于普通失败，规则与内置账号一致。
#[test]
fn custom_rate_limited_failures_wait_longer_than_ordinary_network_errors() {
    let mut state = BackoffState::default();

    backoff::record_failure(
        &mut state,
        "custom:a3f9c1",
        "对方限流了，稍后会自动重试",
        now(),
    );
    backoff::record_failure(
        &mut state,
        "custom:b7e204",
        "网络不通，连不上这个地址，请检查网络或代理设置",
        now(),
    );

    assert_eq!(
        backoff::cooldown_remaining(&state, "custom:a3f9c1", now()).unwrap(),
        Duration::minutes(10)
    );
    assert_eq!(
        backoff::cooldown_remaining(&state, "custom:b7e204", now()).unwrap(),
        Duration::minutes(1)
    );
}

#[test]
fn custom_success_clears_backoff_so_the_next_failure_starts_short() {
    let mut state = BackoffState::default();
    backoff::record_failure(
        &mut state,
        "custom:a3f9c1",
        "对方限流了，稍后会自动重试",
        now(),
    );
    assert!(backoff::cooldown_remaining(&state, "custom:a3f9c1", now()).is_some());

    backoff::record_success(&mut state, "custom:a3f9c1");
    assert!(backoff::cooldown_remaining(&state, "custom:a3f9c1", now()).is_none());

    backoff::record_failure(
        &mut state,
        "custom:a3f9c1",
        "对方限流了，稍后会自动重试",
        now(),
    );
    assert_eq!(
        backoff::cooldown_remaining(&state, "custom:a3f9c1", now()).unwrap(),
        Duration::minutes(10)
    );
}

// -------------------------------------------------- 状态文件格式不变

/// 自定义提供商加进来之前落盘的退避文件只有内置标识。格式没变，旧文件必须直接能读，
/// 而且往里面再记一条 `custom:` 也不该把原来的条目挤掉。
#[test]
fn old_backoff_file_with_only_builtin_keys_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(backoff::STATE_NAME);
    std::fs::write(
        &path,
        r#"{
  "entries": {
    "claude": {
      "failures": 2,
      "retry_at": "2026-08-22T12:20:00+00:00"
    }
  }
}"#,
    )
    .unwrap();

    let mut state = backoff::load_state(&path);
    assert_eq!(
        backoff::cooldown_remaining(&state, "claude", at("2026-08-22T12:00:00Z"))
            .unwrap()
            .num_minutes(),
        20
    );
    assert!(
        backoff::cooldown_remaining(&state, "custom:a3f9c1", at("2026-08-22T12:00:00Z")).is_none(),
        "旧文件里没有自定义条目，不该凭空造一条出来"
    );

    backoff::record_failure(
        &mut state,
        "custom:a3f9c1",
        "对方限流了，稍后会自动重试",
        now(),
    );
    backoff::save_state(&path, &state).unwrap();

    let reloaded = backoff::load_state(&path);
    assert!(
        backoff::cooldown_remaining(&reloaded, "claude", at("2026-08-22T12:00:00Z")).is_some(),
        "新写入自定义条目不该弄丢内置那条"
    );
    assert!(backoff::cooldown_remaining(&reloaded, "custom:a3f9c1", now()).is_some());

    // 落盘形状仍是 `{ entries: { id: { failures, retry_at } } }`，没有多出字段。
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let root = raw.as_object().expect("根是对象");
    assert_eq!(
        root.keys().cloned().collect::<Vec<_>>(),
        vec!["entries".to_string()]
    );
    for id in ["claude", "custom:a3f9c1"] {
        let entry = raw["entries"][id].as_object().expect(id);
        let mut keys: Vec<&String> = entry.keys().collect();
        keys.sort();
        assert_eq!(keys, ["failures", "retry_at"]);
    }
}

// -------------------------------------------------- 手动刷新不静默失败

/// 冷却期内点单条刷新必须回一句「还要等多久」，上次窗口留着，冷却不被点一下就推后。
/// 走带路径的入口：真实 `state_path()` 指向用户目录，单测不许读。
#[test]
fn manual_refresh_during_custom_cooldown_says_how_long_and_keeps_last_window() {
    let conn = db::open_memory().unwrap();
    let windows =
        custom::parse_quota(CustomQuotaPreset::OpenAiCompatible, &[SUBSCRIPTION, USAGE]).unwrap();
    quota::apply_fetch_results(
        &conn,
        [(
            "custom:a3f9c1".to_string(),
            Ok((windows, now().to_rfc3339()).into()),
        )],
    )
    .unwrap();
    // 上一轮已经把真实失败写进库。冷却短路不能把这句话冲掉——用户刷新只是
    // 想知道还要等多久，不是想把「密钥无效」换成一句等待提示。
    quota::apply_fetch_results(
        &conn,
        [(
            "custom:a3f9c1".to_string(),
            Err("密钥无效或已失效，请在设置页更新密钥".to_string()) as quota::ProviderFetch,
        )],
    )
    .unwrap();

    let (_dir, path, _) = seeded_cooldown("custom:a3f9c1", "对方限流了，稍后会自动重试");
    let before = backoff::cooldown_remaining(&backoff::load_state(&path), "custom:a3f9c1", now());
    let fetch::ThrottledFetch::Cooldown(error) =
        fetch::fetch_target_throttled_at(&custom_target(), &path, now())
    else {
        panic!("冷却期内应该短路，不该真去打网");
    };

    assert!(error.contains("公司的中转"), "{error}");
    assert!(error.contains("10 分钟后自动重试"), "{error}");
    assert!(error.contains("上次结果"), "{error}");
    assert_eq!(
        backoff::cooldown_remaining(&backoff::load_state(&path), "custom:a3f9c1", now()),
        before,
        "点刷新不该把冷却再推后一次"
    );

    // 冷却提示不落库：上次窗口和错误原样留着。
    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[resolved("custom:a3f9c1", "公司的中转")],
        now(),
    );
    let row = dto
        .rows
        .iter()
        .find(|row| row.provider == "custom:a3f9c1")
        .unwrap();
    assert_eq!(row.windows[0].used_percent, Some(38.0));
    assert_eq!(
        row.error.as_deref(),
        Some("密钥无效或已失效，请在设置页更新密钥"),
        "冷却提示只出现在这次响应里，不能落库盖掉上次真实错误"
    );
}

/// 整体刷新把冷却中的自定义提供商整条跳过，避免对着已经限流的中转站再打一枪。
#[test]
fn cooling_custom_targets_are_skipped_on_the_next_pass() {
    let mut state = BackoffState::default();
    backoff::record_failure(
        &mut state,
        "custom:a3f9c1",
        "对方限流了，稍后会自动重试",
        now(),
    );
    let targets = vec![
        quota::FetchTarget::Custom(Box::new(resolved("custom:a3f9c1", "公司的中转"))),
        quota::FetchTarget::Custom(Box::new(resolved("custom:b7e204", "备用中转"))),
    ];
    let kept = fetch::exclude_cooling(targets, &state, now());
    let ids: Vec<&str> = kept.iter().map(quota::QuotaTarget::quota_id).collect();
    assert_eq!(ids, vec!["custom:b7e204"]);
}

// -------------------------------------------------- 取数结果 → 退避状态

/// 真正打过网的失败才进退避；连续两次普通失败翻倍，和内置账号同一套计数。
#[test]
fn network_failures_enter_backoff_through_the_fetch_adapter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(backoff::STATE_NAME);
    let mut state = BackoffState::default();
    let network: quota::ProviderFetch = Err("网络不通，连不上这个地址".into());

    fetch::record_backoff(&mut state, [("custom:a3f9c1", &network)], now(), &path);
    assert_eq!(
        backoff::cooldown_remaining(&backoff::load_state(&path), "custom:a3f9c1", now()).unwrap(),
        Duration::minutes(1)
    );
    fetch::record_backoff(&mut state, [("custom:a3f9c1", &network)], now(), &path);
    assert_eq!(
        backoff::cooldown_remaining(&backoff::load_state(&path), "custom:a3f9c1", now()).unwrap(),
        Duration::minutes(2)
    );
}

/// 没配密钥、预设未实现这两种在打网之前就被拦下，走完整条手动刷新也不进退避。
#[test]
fn precheck_failures_through_manual_refresh_do_not_enter_backoff() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(backoff::STATE_NAME);

    let missing = quota::FetchTarget::Custom(Box::new(unresolved("custom:b7e204", "缺密钥的")));
    let fetch::ThrottledFetch::Attempted(Err(missing_err)) =
        fetch::fetch_target_throttled_at(&missing, &path, now())
    else {
        panic!("缺密钥应该真正走到取数入口，而不是被当成冷却");
    };
    assert_eq!(missing_err, custom::MISSING_SECRET);
    assert!(
        backoff::cooldown_remaining(&backoff::load_state(&path), "custom:b7e204", now()).is_none(),
        "缺密钥不该进退避"
    );

    let mut unsupported = resolved("custom:c0ffee", "DeepSeek");
    unsupported.config.preset = CustomQuotaPreset::DeepSeek;
    let unsupported = quota::FetchTarget::Custom(Box::new(unsupported));
    let fetch::ThrottledFetch::Attempted(Err(unsupported_err)) =
        fetch::fetch_target_throttled_at(&unsupported, &path, now())
    else {
        panic!("未实现的预设应该真正走到取数入口，而不是被当成冷却");
    };
    assert!(unsupported_err.contains("暂未支持"), "{unsupported_err}");
    assert!(
        backoff::cooldown_remaining(&backoff::load_state(&path), "custom:c0ffee", now()).is_none(),
        "未实现的预设不该进退避"
    );
}

/// 取数成功要从磁盘上忘掉这一条。冷却文件是跨重启的，只改内存等于没清。
#[test]
fn a_successful_fetch_forgets_the_custom_provider_on_disk() {
    let (_dir, path, mut state) = seeded_cooldown("custom:a3f9c1", "对方限流了，稍后会自动重试");
    let ok: quota::ProviderFetch = Ok((Vec::new(), now().to_rfc3339()).into());

    fetch::record_backoff(&mut state, [("custom:a3f9c1", &ok)], now(), &path);

    assert!(
        backoff::cooldown_remaining(&backoff::load_state(&path), "custom:a3f9c1", now()).is_none()
    );
}
