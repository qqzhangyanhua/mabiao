//! LiteLLM Proxy 预设：打网关 `/key/info`（不带 key 查询参数），
//! 把当前预算窗口的花费 / 上限 / 下次重置读成额度窗口。
//!
//! 两版响应形状（嵌套 `info` 与顶层平铺）各一份脱敏 fixture，覆盖
//! `max_budget` 有值 / 为 null。不含真实 key、不含真实域名。

use super::{provider, today};
use crate::official_quota::custom::store::{
    CustomQuotaConfig, CustomQuotaCredentials, CustomQuotaProvider,
};
use crate::official_quota::custom::{self, panel, store, CustomQuotaPreset};
use crate::official_quota::{self as quota};
use crate::store as db;
use crate::test_support::fixture;

const BASE: &str = "https://gateway.example.com";
const KEY_INFO: &str = "https://gateway.example.com/key/info";

fn nested() -> String {
    fixture("litellm-proxy-nested.json")
}

fn flat() -> String {
    fixture("litellm-proxy-flat.json")
}

fn nested_no_budget() -> String {
    fixture("litellm-proxy-nested-no-budget.json")
}

fn flat_no_budget() -> String {
    fixture("litellm-proxy-flat-no-budget.json")
}

fn litellm_provider(id: &str, name: &str) -> CustomQuotaProvider {
    let mut config = provider(id, name);
    config.preset = CustomQuotaPreset::LiteLlmProxy;
    config.base_url = BASE.to_string();
    config
}

fn expected_budget_window() -> crate::domain::OfficialQuotaWindow {
    crate::domain::OfficialQuotaWindow {
        kind: "budget_window".into(),
        label: "预算 30d".into(),
        used_percent: Some(45.0),
        resets_at: Some("2026-09-01T00:00:00Z".into()),
        used_amount: Some(4.5),
        limit_amount: Some(10.0),
        currency: Some("USD".into()),
        ..Default::default()
    }
}

#[test]
fn litellm_proxy_is_implemented_and_named() {
    assert!(
        CustomQuotaPreset::LiteLlmProxy.implemented(),
        "下拉里不该再标「暂未支持」"
    );
    assert_eq!(
        CustomQuotaPreset::LiteLlmProxy.display_name(),
        "LiteLLM Proxy"
    );
    assert_eq!(CustomQuotaPreset::LiteLlmProxy.as_str(), "litellm_proxy");
}

/// 单条必需请求，指向 `/key/info`，查询串里不能出现 key——设置页「将请求」
/// 回显画的就是这个地址，密钥进 URL 就会明文出现在界面上。
#[test]
fn litellm_proxy_requests_key_info_without_a_key_query() {
    let requests = custom::request_urls(CustomQuotaPreset::LiteLlmProxy, BASE, today()).unwrap();
    assert_eq!(requests.len(), 1, "只打 key 信息这一条");
    assert_eq!(requests[0].url, KEY_INFO);
    assert!(requests[0].required, "拿不到就没有任何可显示的东西");
    assert!(
        !requests[0].url.contains('?'),
        "不能带查询参数，否则密钥会进回显：{}",
        requests[0].url
    );
    assert!(
        !requests[0].url.contains("key="),
        "密钥不得出现在请求地址里：{}",
        requests[0].url
    );
}

#[test]
fn litellm_proxy_four_base_url_spellings_hit_the_same_address() {
    let expected = custom::request_urls(CustomQuotaPreset::LiteLlmProxy, BASE, today()).unwrap();
    for raw in [
        "https://gateway.example.com/",
        "https://gateway.example.com/v1",
        "https://gateway.example.com/v1/",
        "  https://gateway.example.com/v1/  ",
    ] {
        let actual = custom::request_urls(CustomQuotaPreset::LiteLlmProxy, raw, today()).unwrap();
        assert_eq!(actual, expected, "{raw} 应该和根地址落到同一个地址");
    }
    assert_eq!(expected[0].url, KEY_INFO);
}

/// 回显走的就是 `request_urls`。选这档时设置页那行「将请求」必须和真正取数
/// 的地址一致，且地址里看不到密钥。
#[test]
fn litellm_proxy_echo_matches_the_fetch_and_does_not_leak_the_key() {
    let preview = panel::preview_requests(CustomQuotaPreset::LiteLlmProxy, BASE, today());
    assert_eq!(preview.error, None);
    assert_eq!(
        preview.requests,
        custom::request_urls(CustomQuotaPreset::LiteLlmProxy, BASE, today()).unwrap()
    );
    assert_eq!(preview.requests[0].url, KEY_INFO);
    assert!(!preview.requests[0].url.contains("sk-"));
    assert!(!preview.requests[0].url.contains("key="));
}

#[test]
fn nested_and_flat_key_info_parse_to_the_same_window() {
    let nested = custom::parse_quota(CustomQuotaPreset::LiteLlmProxy, &[&nested()]).unwrap();
    let flat = custom::parse_quota(CustomQuotaPreset::LiteLlmProxy, &[&flat()]).unwrap();
    assert_eq!(nested, flat, "两版形状必须读出完全相同的额度窗口");
    assert_eq!(nested, vec![expected_budget_window()]);
}

#[test]
fn max_budget_null_degrades_to_amount_only() {
    let nested =
        custom::parse_quota(CustomQuotaPreset::LiteLlmProxy, &[&nested_no_budget()]).unwrap();
    let flat = custom::parse_quota(CustomQuotaPreset::LiteLlmProxy, &[&flat_no_budget()]).unwrap();
    assert_eq!(nested, flat);
    assert_eq!(nested[0].used_amount, Some(4.5));
    assert_eq!(nested[0].limit_amount, None);
    assert_eq!(nested[0].used_percent, None);
    assert_eq!(nested[0].currency.as_deref(), Some("USD"));
    assert_eq!(nested[0].kind, "budget_window");
    // 上限没有，重置时间与周期标签仍然在——降级的是百分比，不是整扇窗口。
    assert_eq!(nested[0].label, "预算 30d");
    assert_eq!(nested[0].resets_at.as_deref(), Some("2026-09-01T00:00:00Z"));
}

#[test]
fn budget_reset_at_maps_to_the_window_and_null_omits_it() {
    let with_reset = custom::parse_quota(CustomQuotaPreset::LiteLlmProxy, &[&nested()]).unwrap();
    assert_eq!(
        with_reset[0].resets_at.as_deref(),
        Some("2026-09-01T00:00:00Z")
    );

    let without = custom::parse_quota(
        CustomQuotaPreset::LiteLlmProxy,
        &[r#"{"spend":4.5,"max_budget":10.0,"budget_reset_at":null}"#],
    )
    .unwrap();
    assert_eq!(without[0].resets_at, None);
    assert_eq!(without[0].used_percent, Some(45.0));
}

/// 窗口类型是告警去重键的一部分。用户改预算周期只该改标签，
/// 否则同一扇窗会被当成另一扇，80% 再弹一次。
#[test]
fn window_kind_stays_budget_window_when_duration_changes() {
    let monthly = custom::parse_quota(
        CustomQuotaPreset::LiteLlmProxy,
        &[r#"{"spend":4.5,"max_budget":10.0,"budget_duration":"30d"}"#],
    )
    .unwrap();
    let weekly = custom::parse_quota(
        CustomQuotaPreset::LiteLlmProxy,
        &[r#"{"spend":4.5,"max_budget":10.0,"budget_duration":"7d"}"#],
    )
    .unwrap();
    assert_eq!(monthly[0].kind, "budget_window");
    assert_eq!(weekly[0].kind, monthly[0].kind);
    assert_eq!(monthly[0].label, "预算 30d");
    assert_eq!(weekly[0].label, "预算 7d");

    let none = custom::parse_quota(
        CustomQuotaPreset::LiteLlmProxy,
        &[r#"{"spend":4.5,"max_budget":10.0}"#],
    )
    .unwrap();
    assert_eq!(none[0].kind, "budget_window");
    assert_eq!(none[0].label, "预算");
}

#[test]
fn bad_key_info_responses_become_readable_chinese() {
    let cases: [(&str, &str); 3] = [
        ("", "空响应"),
        ("{not json", "不是合法 JSON"),
        (r#"{"key":"sk-example"}"#, "spend"),
    ];
    for (body, hint) in cases {
        let error = custom::parse_quota(CustomQuotaPreset::LiteLlmProxy, &[body]).unwrap_err();
        assert!(error.contains(hint), "报错读不懂：{error}");
        assert!(!error.contains("panicked"));
    }
    assert!(custom::parse_quota(CustomQuotaPreset::LiteLlmProxy, &[]).is_err());
}

/// 保存这档之后，下拉不再标「暂未支持」，首页官方额度能画出解析到的窗口。
#[test]
fn saving_a_litellm_proxy_provider_joins_official_quota() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    let saved = panel::save(
        &paths,
        panel::SaveCustomQuotaProvider {
            id: None,
            name: "自建网关".to_string(),
            preset: CustomQuotaPreset::LiteLlmProxy,
            base_url: BASE.to_string(),
            enabled: None,
            secret: Some("sk-virt-123456".to_string()),
        },
    )
    .unwrap();

    let listed = saved
        .panel
        .presets
        .iter()
        .find(|preset| preset.value == "litellm_proxy")
        .expect("下拉里应有 LiteLLM Proxy");
    assert!(listed.supported, "下拉里不该再标「暂未支持」");
    assert_eq!(listed.label, "LiteLLM Proxy");
    assert_eq!(
        saved.panel.providers[0].preset,
        CustomQuotaPreset::LiteLlmProxy
    );
    assert_eq!(
        saved.panel.providers[0].secret_mask.as_deref(),
        Some("••••••3456")
    );

    let id = saved.saved_id.clone();
    let windows = custom::parse_quota(CustomQuotaPreset::LiteLlmProxy, &[&nested()]).unwrap();
    let now = chrono::Utc::now();
    let conn = db::open_memory().unwrap();
    quota::apply_fetch_results(&conn, [(id.clone(), Ok((windows, now.to_rfc3339())))]).unwrap();

    let dto = quota::load_dto(
        &conn,
        &crate::domain::OfficialQuotaConfig::default(),
        &[custom::ResolvedProvider {
            config: litellm_provider(&id, "自建网关"),
            secret: Some("sk-virt-123456".to_string()),
        }],
        now,
    );
    let row = dto
        .rows
        .iter()
        .find(|row| row.provider == id)
        .expect("保存后首页官方额度应出现对应行");
    assert_eq!(row.application, "自建网关");
    assert_eq!(row.windows[0], expected_budget_window());
}

#[test]
fn a_disabled_litellm_proxy_keeps_config_and_takes_no_row() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    let saved = panel::save(
        &paths,
        panel::SaveCustomQuotaProvider {
            id: None,
            name: "备用网关".to_string(),
            preset: CustomQuotaPreset::LiteLlmProxy,
            base_url: BASE.to_string(),
            enabled: Some(false),
            secret: Some("sk-virt-123456".to_string()),
        },
    )
    .unwrap();
    let listed = &saved.panel.providers[0];
    assert!(!listed.enabled);
    assert_eq!(listed.preset, CustomQuotaPreset::LiteLlmProxy);
    assert_eq!(listed.secret_mask.as_deref(), Some("••••••3456"));

    let loaded = store::load_providers(&paths);
    assert!(!loaded[0].config.enabled);
    assert_eq!(loaded[0].secret.as_deref(), Some("sk-virt-123456"));

    let dto = quota::load_dto(
        &db::open_memory().unwrap(),
        &crate::domain::OfficialQuotaConfig::default(),
        &loaded,
        chrono::Utc::now(),
    );
    assert!(!dto.rows.iter().any(|row| row.provider == listed.id));
}

/// 有密钥就不该再拦在「暂未支持」里，这样限流 / 网络失败才能走进既有退避。
#[test]
fn a_litellm_proxy_with_a_secret_is_tried_instead_of_blocked_as_unsupported() {
    let resolved = custom::ResolvedProvider {
        config: litellm_provider("custom:a3f9c1", "自建网关"),
        secret: Some("sk-virt".to_string()),
    };
    assert_eq!(
        custom::precheck(&resolved),
        None,
        "有密钥的 LiteLLM Proxy 档现在不该在打网之前被拦下"
    );
    assert!(!custom::is_precheck_error("对方限流了，稍后会自动重试"));
}

#[test]
fn missing_secret_is_a_todo_for_litellm_proxy() {
    let resolved = custom::ResolvedProvider {
        config: litellm_provider("custom:a3f9c1", "自建网关"),
        secret: None,
    };
    assert_eq!(
        custom::fetch(&resolved).unwrap_err(),
        "未配置密钥，请在设置页重新填写"
    );

    let dto = quota::load_dto(
        &db::open_memory().unwrap(),
        &crate::domain::OfficialQuotaConfig::default(),
        std::slice::from_ref(&resolved),
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

/// 有百分比的预算窗口走既有的 80% 告警，并复用同一个总开关。
#[test]
fn litellm_proxy_percent_windows_alert_like_other_custom_providers() {
    let windows = custom::parse_quota(
        CustomQuotaPreset::LiteLlmProxy,
        &[r#"{"spend":9.0,"max_budget":10.0,"budget_duration":"30d","budget_reset_at":"2026-09-01T00:00:00Z"}"#],
    )
    .unwrap();
    assert_eq!(windows[0].used_percent, Some(90.0));
    assert_eq!(windows[0].kind, "budget_window");

    let now = chrono::Utc::now();
    let conn = db::open_memory().unwrap();
    quota::apply_fetch_results(
        &conn,
        [("custom:a3f9c1".to_string(), Ok((windows, now.to_rfc3339())))],
    )
    .unwrap();

    let snapshot = quota::load_dto(
        &conn,
        &crate::domain::OfficialQuotaConfig::default(),
        &[custom::ResolvedProvider {
            config: litellm_provider("custom:a3f9c1", "自建网关"),
            secret: Some("sk-virt".to_string()),
        }],
        now,
    );
    let (state, alerts) =
        quota::notify::prepare_notifications(quota::notify::NotifyState::default(), &snapshot);
    assert_eq!(alerts.len(), 1, "90% 应该触发一次 80% 告警");
    assert_eq!(alerts[0].provider, "自建网关");
    assert_eq!(alerts[0].label, "预算 30d");
    assert_eq!(alerts[0].threshold, 80);

    let muted = quota::load_dto(
        &conn,
        &crate::domain::OfficialQuotaConfig {
            alerts_enabled: false,
            ..Default::default()
        },
        &[custom::ResolvedProvider {
            config: litellm_provider("custom:a3f9c1", "自建网关"),
            secret: Some("sk-virt".to_string()),
        }],
        now,
    );
    let (_, silent) = quota::notify::prepare_notifications(state, &muted);
    assert!(silent.is_empty(), "关掉额度告警总开关之后这档同样不该提醒");
}

/// 配置文件写下预设标识，密钥只在凭证文件里。备份带走前者、丢掉后者。
#[test]
fn litellm_proxy_config_serializes_without_the_secret() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    store::save_config(
        &paths.config,
        &CustomQuotaConfig {
            providers: vec![litellm_provider("custom:a3f9c1", "自建网关")],
        },
    )
    .unwrap();
    let mut credentials = CustomQuotaCredentials::default();
    credentials
        .secrets
        .insert("custom:a3f9c1".to_string(), "sk-virt-secret".to_string());
    store::save_credentials(&paths.credentials, &credentials).unwrap();

    let config_text = std::fs::read_to_string(&paths.config).unwrap();
    assert!(config_text.contains("litellm_proxy"));
    assert!(
        !config_text.contains("sk-virt-secret"),
        "配置文件里不该写出密钥"
    );
    let loaded = store::load_providers(&paths);
    assert_eq!(loaded[0].config.preset, CustomQuotaPreset::LiteLlmProxy);
    assert_eq!(loaded[0].secret.as_deref(), Some("sk-virt-secret"));
}
