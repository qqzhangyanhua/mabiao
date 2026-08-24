//! DTO 合流：自定义行怎么进「官方额度」，以及缓存 / 告警 / 托盘的连带行为。

use super::{provider, SUBSCRIPTION, USAGE};
use crate::domain::{OfficialQuotaConfig, OfficialQuotaFreshness, OfficialQuotaProvider};
use crate::official_quota::custom::{self, CustomQuotaPreset};
use crate::official_quota::{self as quota};
use crate::store as db;

// -------------------------------------------------- DTO 合流

#[test]
fn custom_rows_join_the_dto_after_the_builtin_ones() {
    let conn = db::open_memory().unwrap();
    let windows =
        custom::parse_quota(CustomQuotaPreset::OpenAiCompatible, &[SUBSCRIPTION, USAGE]).unwrap();
    let now = chrono::Utc::now();
    quota::apply_fetch_results(
        &conn,
        [("custom:a3f9c1".to_string(), Ok((windows, now.to_rfc3339())))],
    )
    .unwrap();

    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[provider("custom:a3f9c1", "公司的中转")],
        now,
    );
    let row = dto
        .rows
        .iter()
        .find(|row| row.provider == "custom:a3f9c1")
        .expect("自定义行应该出现在官方额度里");
    assert_eq!(row.application, "公司的中转");
    assert_eq!(row.freshness, OfficialQuotaFreshness::Official);
    // 金额口径过一趟 sqlite 也不能丢。
    assert_eq!(row.windows[0].used_amount, Some(19.0));
    assert_eq!(row.windows[0].limit_amount, Some(50.0));
    assert_eq!(row.windows[0].used_percent, Some(38.0));
    // 自定义行排在内置行之后，且不算进「未检测到登录态」那份名单。
    assert_eq!(dto.rows.last().unwrap().provider, "custom:a3f9c1");
    assert!(!dto.undetected.iter().any(|name| name == "公司的中转"));
}

#[test]
fn disabled_custom_providers_take_no_row() {
    let conn = db::open_memory().unwrap();
    let mut off = provider("custom:a3f9c1", "备用中转");
    off.enabled = false;
    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        std::slice::from_ref(&off),
        chrono::Utc::now(),
    );
    assert!(!dto.rows.iter().any(|row| row.provider == "custom:a3f9c1"));

    // 打开就占一行，即使还没取过数——用户自己登记的，看不到会以为没存上。
    let on = provider("custom:a3f9c1", "备用中转");
    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        std::slice::from_ref(&on),
        chrono::Utc::now(),
    );
    let row = dto
        .rows
        .iter()
        .find(|row| row.provider == "custom:a3f9c1")
        .unwrap();
    assert_eq!(row.freshness, OfficialQuotaFreshness::Unavailable);
    assert!(row.windows.is_empty());
}

#[test]
fn renaming_keeps_the_cache_and_does_not_alert_twice() {
    let conn = db::open_memory().unwrap();
    let now = chrono::Utc::now();
    // 带重置时间的窗口——告警去重键要的就是它。OpenAI 兼容计费给不出重置时间
    // 的那条路在 `tray_and_alerts` 里另测。
    let window = crate::domain::OfficialQuotaWindow {
        kind: "billing_cycle".into(),
        label: "总量".into(),
        used_percent: Some(90.0),
        resets_at: Some("2026-09-01T00:00:00+00:00".into()),
        used_amount: Some(45.0),
        limit_amount: Some(50.0),
        currency: Some("USD".into()),
        ..Default::default()
    };
    quota::apply_fetch_results(
        &conn,
        [(
            "custom:a3f9c1".to_string(),
            Ok((vec![window], now.to_rfc3339())),
        )],
    )
    .unwrap();

    let before = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[provider("custom:a3f9c1", "公司的中转")],
        now,
    );
    let (state, alerts) =
        quota::notify::prepare_notifications(quota::notify::NotifyState::default(), &before);
    assert_eq!(alerts.len(), 1, "90% 应该触发一次 80% 告警");
    assert_eq!(alerts[0].provider, "公司的中转");

    // 改名：标识不变，因此缓存照旧命中、告警去重记录也照旧命中。
    let after = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[provider("custom:a3f9c1", "老板的中转")],
        now,
    );
    let row = after
        .rows
        .iter()
        .find(|row| row.provider == "custom:a3f9c1")
        .unwrap();
    assert_eq!(row.application, "老板的中转");
    assert_eq!(row.windows[0].used_percent, Some(90.0));
    assert_eq!(row.windows[0].used_amount, Some(45.0));
    let (_, again) = quota::notify::prepare_notifications(state, &after);
    assert!(again.is_empty(), "改个名字不该重复告警");
}

#[test]
fn custom_failures_keep_the_last_good_window_and_only_swap_the_message() {
    let conn = db::open_memory().unwrap();
    let now = chrono::Utc::now();
    let windows =
        custom::parse_quota(CustomQuotaPreset::OpenAiCompatible, &[SUBSCRIPTION, USAGE]).unwrap();
    quota::apply_fetch_results(
        &conn,
        [("custom:a3f9c1".to_string(), Ok((windows, now.to_rfc3339())))],
    )
    .unwrap();
    quota::apply_fetch_results(
        &conn,
        [(
            "custom:a3f9c1".to_string(),
            Err("密钥无效或已失效，请在设置页更新密钥".to_string()) as quota::ProviderFetch,
        )],
    )
    .unwrap();

    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[provider("custom:a3f9c1", "公司的中转")],
        now,
    );
    let row = dto
        .rows
        .iter()
        .find(|row| row.provider == "custom:a3f9c1")
        .unwrap();
    assert_eq!(row.windows[0].used_percent, Some(38.0));
    assert_eq!(
        row.error.as_deref(),
        Some("密钥无效或已失效，请在设置页更新密钥")
    );
}

/// 旧缓存里的窗口没有金额字段。这条独立成测，不允许靠「serde 应该会这样」推断放行。
#[test]
fn quota_windows_cached_before_the_amount_fields_still_deserialize() {
    let conn = db::open_memory().unwrap();
    let legacy = r#"[{"kind":"session_5h","label":"5 小时","used_percent":40.0,
        "resets_at":"2026-08-18T12:00:00+00:00"}]"#;
    conn.execute(
        "INSERT INTO official_quota(provider, windows_json, captured_at, error)
         VALUES('claude', ?1, '2026-08-18T11:00:00+00:00', NULL)",
        [legacy],
    )
    .unwrap();

    let row = db::load_official_quota_row(&conn, "claude")
        .unwrap()
        .unwrap();
    assert_eq!(row.0[0].used_percent, Some(40.0));
    assert_eq!(row.0[0].used_amount, None);
    assert_eq!(row.0[0].limit_amount, None);
    assert_eq!(row.0[0].currency, None);
}

/// 「最紧一档」的语义是「最快撞线、撞了会自己重置」。充值制余额是存量不是流量，
/// 不充值就永远不回落，会把托盘标题钉死。
#[test]
fn tightest_window_skips_custom_providers() {
    let conn = db::open_memory().unwrap();
    let now = chrono::Utc::now();
    quota::apply_fetch_results(
        &conn,
        [
            (
                OfficialQuotaProvider::Claude.as_str().to_string(),
                Ok((
                    vec![crate::domain::OfficialQuotaWindow {
                        kind: "session_5h".into(),
                        label: "5 小时".into(),
                        used_percent: Some(42.0),
                        ..Default::default()
                    }],
                    now.to_rfc3339(),
                )),
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
                )),
            ),
        ],
    )
    .unwrap();

    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[provider("custom:a3f9c1", "公司的中转")],
        now,
    );
    let tightest = quota::tightest_window(&dto).unwrap();
    // 95% 的余额没有抢走标题，每天真正在动的 5 小时窗还在。
    assert_eq!(tightest.provider, "Claude");
    assert_eq!(tightest.used_percent, 42.0);
}
