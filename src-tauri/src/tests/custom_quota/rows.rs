//! DTO 合流：自定义行怎么进「官方额度」，以及缓存 / 告警 / 托盘的连带行为。

use super::{resolved, unresolved, SUBSCRIPTION, USAGE};
use crate::domain::{
    OfficialQuotaConfig, OfficialQuotaDto, OfficialQuotaFreshness, OfficialQuotaRow,
    OfficialQuotaWindow,
};
use crate::official_quota::custom::{self, CustomQuotaPreset};
use crate::official_quota::{self as quota, QuotaTarget};
use crate::store as db;
use crate::test_support::fixture;

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
        &[resolved("custom:a3f9c1", "公司的中转")],
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
    let mut off = resolved("custom:a3f9c1", "备用中转");
    off.config.enabled = false;
    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        std::slice::from_ref(&off),
        chrono::Utc::now(),
    );
    assert!(!dto.rows.iter().any(|row| row.provider == "custom:a3f9c1"));

    // 打开就占一行，即使还没取过数——用户自己登记的，看不到会以为没存上。
    let on = resolved("custom:a3f9c1", "备用中转");
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
        &[resolved("custom:a3f9c1", "公司的中转")],
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
        &[resolved("custom:a3f9c1", "老板的中转")],
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
        &[resolved("custom:a3f9c1", "公司的中转")],
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

fn quota_dto(rows: Vec<OfficialQuotaRow>) -> OfficialQuotaDto {
    OfficialQuotaDto {
        rows,
        alerts_enabled: true,
        stale_after_minutes: 10,
        undetected: Vec::new(),
        hidden_providers: Vec::new(),
    }
}

fn official_row(
    provider: &str,
    application: &str,
    windows: Vec<OfficialQuotaWindow>,
) -> OfficialQuotaRow {
    OfficialQuotaRow {
        provider: provider.into(),
        application: application.into(),
        windows,
        freshness: OfficialQuotaFreshness::Official,
        captured_at: Some("2026-08-24T12:00:00+00:00".into()),
        error: None,
        todo: None,
    }
}

fn claude_5h(percent: f64) -> OfficialQuotaWindow {
    OfficialQuotaWindow {
        kind: "session_5h".into(),
        label: "5 小时".into(),
        used_percent: Some(percent),
        resets_at: Some("2026-08-24T17:00:00+00:00".into()),
        ..Default::default()
    }
}

/// 「最紧」= 最快撞线、撞了会自己重置。充值制余额没有重置时间，仍跳过。
#[test]
fn tightest_window_skips_custom_windows_without_a_reset_time() {
    let custom = custom::parse_quota(
        CustomQuotaPreset::OpenAiCompatible,
        &[r#"{"hard_limit_usd":50}"#, r#"{"total_usage":4750}"#],
    )
    .unwrap();
    assert_eq!(custom[0].used_percent, Some(95.0));
    assert_eq!(custom[0].resets_at, None);

    let dto = quota_dto(vec![
        official_row("claude", "Claude", vec![claude_5h(42.0)]),
        official_row("custom:a3f9c1", "公司的中转", custom),
    ]);
    let tightest = quota::tightest_window(&dto).unwrap();
    assert_eq!(tightest.provider, "Claude");
    assert_eq!(tightest.used_percent, 42.0);
}

/// 带重置时间的自定义窗口是流量型，能进标题。短标签走窗口类型，
/// 预算窗叫「预算」而不是「总量」。
#[test]
fn tightest_window_includes_custom_windows_with_a_reset_time() {
    let budget = custom::parse_quota(
        CustomQuotaPreset::LiteLlmProxy,
        &[&fixture("litellm-proxy-nested.json")],
    )
    .unwrap();
    assert!(budget[0].resets_at.is_some());

    let dto = quota_dto(vec![
        official_row("claude", "Claude", vec![claude_5h(42.0)]),
        official_row(
            "cursor",
            "Cursor",
            vec![OfficialQuotaWindow {
                kind: "auto".into(),
                label: "Auto".into(),
                used_percent: Some(40.0),
                resets_at: None,
                ..Default::default()
            }],
        ),
        official_row("custom:lite1", "家里的网关", budget),
    ]);
    let tightest = quota::tightest_window(&dto).unwrap();
    assert_eq!(tightest.provider, "家里的网关");
    assert_eq!(tightest.label, "预算");
    assert_eq!(tightest.used_percent, 45.0);
    assert_eq!(
        crate::tray::format_title_with_quota(Some(1.23), false, Some(&tightest)),
        "$1.23 · 家里的网关 预算 45%"
    );
}

/// 内置账号没有重置时间的窗口（Cursor Auto）仍参与竞争，
/// 改判据不能把它们一并跳过。
#[test]
fn tightest_window_still_counts_builtin_windows_without_a_reset_time() {
    let budget = custom::parse_quota(
        CustomQuotaPreset::LiteLlmProxy,
        &[&fixture("litellm-proxy-nested.json")],
    )
    .unwrap();
    let dto = quota_dto(vec![
        official_row(
            "cursor",
            "Cursor",
            vec![OfficialQuotaWindow {
                kind: "auto".into(),
                label: "Auto".into(),
                used_percent: Some(80.0),
                resets_at: None,
                ..Default::default()
            }],
        ),
        official_row("custom:lite1", "家里的网关", budget),
    ]);
    let tightest = quota::tightest_window(&dto).unwrap();
    assert_eq!(tightest.provider, "Cursor");
    assert_eq!(tightest.label, "Auto");
    assert_eq!(tightest.used_percent, 80.0);
}

/// 恢复备份后的形状：配置在、密钥没了。首页这一行是待办，不是取数失败。
#[test]
fn missing_secret_is_a_todo_not_a_fetch_error() {
    let conn = db::open_memory().unwrap();
    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[unresolved("custom:a3f9c1", "公司的中转")],
        chrono::Utc::now(),
    );
    let row = dto
        .rows
        .iter()
        .find(|row| row.provider == "custom:a3f9c1")
        .unwrap();
    assert_eq!(row.todo.as_deref(), Some("未配置密钥，请在设置页重新填写"));
    assert_eq!(row.error, None, "缺密钥不该画成取数失败");
}

#[test]
fn missing_secret_keeps_the_last_good_window_and_still_shows_the_todo() {
    let conn = db::open_memory().unwrap();
    let now = chrono::Utc::now();
    let windows =
        custom::parse_quota(CustomQuotaPreset::OpenAiCompatible, &[SUBSCRIPTION, USAGE]).unwrap();
    quota::apply_fetch_results(
        &conn,
        [("custom:a3f9c1".to_string(), Ok((windows, now.to_rfc3339())))],
    )
    .unwrap();
    // 旧缓存里可能把这句话写成了 error；待办要把它挪走，窗口留下。
    quota::apply_fetch_results(
        &conn,
        [(
            "custom:a3f9c1".to_string(),
            Err("未配置密钥，请在设置页重新填写".to_string()) as quota::ProviderFetch,
        )],
    )
    .unwrap();

    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[unresolved("custom:a3f9c1", "公司的中转")],
        now,
    );
    let row = dto
        .rows
        .iter()
        .find(|row| row.provider == "custom:a3f9c1")
        .unwrap();
    assert_eq!(row.windows[0].used_percent, Some(38.0));
    assert_eq!(row.todo.as_deref(), Some("未配置密钥，请在设置页重新填写"));
    assert_eq!(row.error, None);
}

#[test]
fn providers_without_a_secret_are_not_fetched() {
    let mut disabled = resolved("custom:off", "关掉的");
    disabled.config.enabled = false;
    let targets = quota::custom_targets_for_fetch(&[
        unresolved("custom:a3f9c1", "公司的中转"),
        resolved("custom:b7e204", "有密钥的"),
        disabled,
    ]);
    let ids: Vec<&str> = targets.iter().map(|target| target.quota_id()).collect();
    assert_eq!(ids, vec!["custom:b7e204"]);
}

#[test]
fn filling_in_the_secret_clears_a_stale_missing_secret_message() {
    let conn = db::open_memory().unwrap();
    quota::apply_fetch_results(
        &conn,
        [(
            "custom:a3f9c1".to_string(),
            Err("未配置密钥，请在设置页重新填写".to_string()) as quota::ProviderFetch,
        )],
    )
    .unwrap();
    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[resolved("custom:a3f9c1", "公司的中转")],
        chrono::Utc::now(),
    );
    let row = dto
        .rows
        .iter()
        .find(|row| row.provider == "custom:a3f9c1")
        .unwrap();
    assert_eq!(row.todo, None);
    assert_eq!(row.error, None);
}
