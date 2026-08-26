//! #88：自定义提供商接入托盘额度面板与额度告警。
//!
//! 接缝是 #81 预定好的：`visible_rows`（托盘瘦身）、`notify::prepare_notifications`
//! （80% / 100% 告警）、`tightest_window`（标题最紧一档，断言在 `rows.rs`）。
//! 喂 DTO / 配置，断言行的有无和告警的有无，不联网。

use super::resolved;
use crate::domain::{
    OfficialQuotaConfig, OfficialQuotaDto, OfficialQuotaFreshness, OfficialQuotaProvider,
    OfficialQuotaRow, OfficialQuotaWindow,
};
use crate::official_quota::custom::{self, CustomQuotaPreset};
use crate::official_quota::notify;
use crate::official_quota::{self as quota};
use crate::store as db;

fn percent_window(percent: f64, resets_at: Option<&str>) -> OfficialQuotaWindow {
    OfficialQuotaWindow {
        kind: "billing_cycle".into(),
        label: "总量".into(),
        used_percent: Some(percent),
        resets_at: resets_at.map(str::to_string),
        used_amount: Some(45.0),
        limit_amount: Some(50.0),
        currency: Some("USD".into()),
        ..Default::default()
    }
}

fn amount_only_window() -> OfficialQuotaWindow {
    OfficialQuotaWindow {
        kind: "billing_cycle".into(),
        label: "总量".into(),
        used_percent: None,
        resets_at: None,
        used_amount: Some(12.34),
        limit_amount: None,
        currency: Some("USD".into()),
        ..Default::default()
    }
}

fn custom_row(windows: Vec<OfficialQuotaWindow>) -> OfficialQuotaRow {
    OfficialQuotaRow {
        provider: "custom:a3f9c1".into(),
        application: "公司的中转".into(),
        windows,
        freshness: OfficialQuotaFreshness::Official,
        captured_at: Some("2026-08-24T12:00:00+00:00".into()),
        error: None,
        todo: None,
        plan: None,
    }
}

fn dto(rows: Vec<OfficialQuotaRow>, alerts_enabled: bool) -> OfficialQuotaDto {
    OfficialQuotaDto {
        rows,
        alerts_enabled,
        stale_after_minutes: 10,
        undetected: Vec::new(),
        hidden_providers: Vec::new(),
    }
}

fn alerts_of(quota: &OfficialQuotaDto) -> Vec<notify::QuotaAlert> {
    notify::prepare_notifications(notify::NotifyState::default(), quota).1
}

// -------------------------------------------------- 托盘额度面板

/// 托盘按 `hidden_providers` 瘦身，那份名单只来自首页「配置显示」。
/// 自定义提供商刻意不进那个面板，因此把内置账号全藏起来，它仍然要出现。
#[test]
fn tray_keeps_enabled_custom_providers_when_every_builtin_account_is_hidden() {
    let conn = db::open_memory().unwrap();
    let now = chrono::Utc::now();
    quota::apply_fetch_results(
        &conn,
        [(
            "custom:a3f9c1".to_string(),
            Ok((vec![percent_window(38.0, None)], now.to_rfc3339()).into()),
        )],
    )
    .unwrap();

    let hidden: Vec<String> = OfficialQuotaProvider::ALL
        .iter()
        .map(|provider| provider.as_str().to_string())
        .collect();
    let snapshot = quota::load_dto(
        &conn,
        &OfficialQuotaConfig {
            alerts_enabled: true,
            hidden_providers: hidden.clone(),
        },
        &[resolved("custom:a3f9c1", "公司的中转")],
        now,
    );
    // `load_dto` 本身不过滤：设置页还要看到被藏的内置账号。托盘才走 `visible_rows`。
    let shown = quota::visible_rows(snapshot.rows, &hidden);
    assert_eq!(shown.len(), 1, "内置全藏之后托盘里应该只剩自定义那一行");
    assert_eq!(shown[0].provider, "custom:a3f9c1");
    assert_eq!(shown[0].application, "公司的中转");
}

/// 「启用的」才进托盘：关掉的那条连 DTO 行都不占，托盘自然看不见。
#[test]
fn tray_does_not_show_a_disabled_custom_provider() {
    let conn = db::open_memory().unwrap();
    let mut off = resolved("custom:a3f9c1", "备用中转");
    off.config.enabled = false;
    let snapshot = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        std::slice::from_ref(&off),
        chrono::Utc::now(),
    );
    let shown = quota::visible_rows(snapshot.rows, &[]);
    assert!(!shown.iter().any(|row| row.provider == "custom:a3f9c1"));
}

// -------------------------------------------------- 80% / 100% 告警

#[test]
fn custom_percent_windows_alert_at_80_and_dedupe_like_builtin_accounts() {
    let quota = dto(
        vec![custom_row(vec![percent_window(
            82.0,
            Some("2026-09-01T00:00:00+00:00"),
        )])],
        true,
    );
    let (state, alerts) = notify::prepare_notifications(notify::NotifyState::default(), &quota);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].provider, "公司的中转");
    assert_eq!(alerts[0].label, "总量");
    assert_eq!(alerts[0].threshold, 80);
    assert_eq!(alerts[0].used_percent, 82.0);

    let (_, again) = notify::prepare_notifications(state.clone(), &quota);
    assert!(again.is_empty(), "同一重置周期不该重复提醒");

    let mut hundred = quota.clone();
    hundred.rows[0].windows[0].used_percent = Some(100.0);
    let (_, crossed) = notify::prepare_notifications(state, &hundred);
    assert_eq!(crossed.len(), 1);
    assert_eq!(crossed[0].threshold, 100);
}

/// OpenAI 兼容计费给得出百分比、给不出重置时间。有百分比就该走 80%/100%。
#[test]
fn custom_percent_windows_without_a_reset_time_still_alert() {
    let conn = db::open_memory().unwrap();
    let now = chrono::Utc::now();
    let windows = custom::parse_quota(
        CustomQuotaPreset::OpenAiCompatible,
        &[r#"{"hard_limit_usd":50}"#, r#"{"total_usage":4500}"#],
    )
    .unwrap();
    assert_eq!(windows[0].used_percent, Some(90.0));
    assert_eq!(windows[0].resets_at, None);
    quota::apply_fetch_results(
        &conn,
        [(
            "custom:a3f9c1".to_string(),
            Ok((windows, now.to_rfc3339()).into()),
        )],
    )
    .unwrap();

    let snapshot = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[resolved("custom:a3f9c1", "公司的中转")],
        now,
    );
    let alerts = alerts_of(&snapshot);
    assert_eq!(alerts.len(), 1, "90% 的自定义窗口即使没有重置时间也该提醒");
    assert_eq!(alerts[0].provider, "公司的中转");
    assert_eq!(alerts[0].threshold, 80);
}

/// 已知缺陷（#81 / #88 不修）：没有重置时间时去重键里这一段为空，
/// 「报过 80% → 充值 → 又涨到 80%」不会二次提醒。等做余额阈值告警时一并处理。
#[test]
fn custom_percent_without_reset_does_not_alert_again_after_a_recharge() {
    let quota = dto(vec![custom_row(vec![percent_window(90.0, None)])], true);
    let (state, first) = notify::prepare_notifications(notify::NotifyState::default(), &quota);
    assert_eq!(first.len(), 1);

    let mut recharged = quota.clone();
    recharged.rows[0].windows[0].used_percent = Some(10.0);
    let (state, quiet) = notify::prepare_notifications(state, &recharged);
    assert!(quiet.is_empty());

    let mut back = recharged;
    back.rows[0].windows[0].used_percent = Some(90.0);
    let (_, again) = notify::prepare_notifications(state, &back);
    assert!(
        again.is_empty(),
        "没有重置时间就无法区分「同一周期」和「充值后再涨」"
    );
}

#[test]
fn amount_only_custom_windows_do_not_alert() {
    let quota = dto(vec![custom_row(vec![amount_only_window()])], true);
    assert!(
        alerts_of(&quota).is_empty(),
        "算不出百分比的窗口天然不参与 80%/100% 告警"
    );
}

/// 内置账号给不出重置时间的窗口（Cursor Auto 那种）仍然跳过——
/// 否则升级后会把一堆长期 100% 的 Auto 窗一次弹完。放宽只针对自定义。
#[test]
fn builtin_percent_windows_without_a_reset_time_still_do_not_alert() {
    let quota = dto(
        vec![OfficialQuotaRow {
            provider: "cursor".into(),
            application: "Cursor".into(),
            windows: vec![percent_window(100.0, None)],
            freshness: OfficialQuotaFreshness::Official,
            captured_at: Some("2026-08-24T12:00:00+00:00".into()),
            error: None,
            todo: None,
            plan: None,
        }],
        true,
    );
    assert!(alerts_of(&quota).is_empty());
}

#[test]
fn alerts_master_switch_also_gates_custom_providers() {
    let quota = dto(
        vec![custom_row(vec![percent_window(
            100.0,
            Some("2026-09-01T00:00:00+00:00"),
        )])],
        false,
    );
    assert!(
        alerts_of(&quota).is_empty(),
        "关掉额度告警总开关之后自定义提供商同样不该提醒"
    );
}

// -------------------------------------------------- 托盘标题最紧一档（补一条与告警对照的）

/// 95% 的自定义余额会告警，但不会抢走托盘标题。两件事必须同时成立：
/// 告警看的是「快断了」，标题看的是「最快撞线且会自己重置」。
#[test]
fn a_hot_custom_balance_alerts_but_does_not_steal_the_tray_title() {
    let conn = db::open_memory().unwrap();
    let now = chrono::Utc::now();
    quota::apply_fetch_results(
        &conn,
        [
            (
                OfficialQuotaProvider::Claude.as_str().to_string(),
                Ok((
                    vec![OfficialQuotaWindow {
                        kind: "session_5h".into(),
                        label: "5 小时".into(),
                        used_percent: Some(42.0),
                        resets_at: Some("2026-08-24T17:00:00+00:00".into()),
                        ..Default::default()
                    }],
                    now.to_rfc3339(),
                )
                    .into()),
            ),
            (
                "custom:a3f9c1".to_string(),
                Ok((
                    custom::parse_quota(
                        CustomQuotaPreset::OpenAiCompatible,
                        &[r#"{"hard_limit_usd":50}"#, r#"{"total_usage":4750}"#],
                    )
                    .unwrap(),
                    now.to_rfc3339(),
                )
                    .into()),
            ),
        ],
    )
    .unwrap();

    let snapshot = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[resolved("custom:a3f9c1", "公司的中转")],
        now,
    );
    let alerts = alerts_of(&snapshot);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].provider, "公司的中转");
    assert_eq!(alerts[0].threshold, 80);

    let tightest = quota::tightest_window(&snapshot).unwrap();
    assert_eq!(tightest.provider, "Claude");
    assert_eq!(tightest.used_percent, 42.0);
}
