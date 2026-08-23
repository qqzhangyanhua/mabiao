//! 自定义提供商：三个新接缝（加载 / 构造地址 / 解析响应）与 DTO 合流。
//!
//! 全部走 fixture 字符串与 tempfile，不联网、不读真实用户目录。

use crate::domain::{OfficialQuotaConfig, OfficialQuotaFreshness, OfficialQuotaProvider};
use crate::official_quota::custom::store::{
    CustomQuotaConfig, CustomQuotaCredentials, CustomQuotaProvider,
};
use crate::official_quota::custom::{self, panel, store, CustomQuotaPreset};
use crate::official_quota::{self as quota};
use crate::store as db;

const SUBSCRIPTION: &str = r#"{
    "object": "billing_subscription",
    "hard_limit_usd": 50.0,
    "system_hard_limit_usd": 100.0,
    "access_until": 0
}"#;
/// `total_usage` 是**美分**：这套接口上限用美元、已用用美分，是它的固有坑。
const USAGE: &str = r#"{"object":"list","total_usage":1900.0,"daily_costs":[]}"#;

fn provider(id: &str, name: &str) -> CustomQuotaProvider {
    CustomQuotaProvider {
        id: id.to_string(),
        name: name.to_string(),
        preset: CustomQuotaPreset::OpenAiCompatible,
        base_url: "https://relay.example.com".to_string(),
        enabled: true,
    }
}

fn today() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 8, 23).unwrap()
}

// ---------------------------------------------------------------- 标识

#[test]
fn custom_ids_never_collide_with_the_nine_builtin_accounts() {
    // 前缀是「不冲突」的全部依据：内置枚举认不出任何 `custom:` 开头的标识。
    for id in ["custom:a3f9c1", "custom:claude", "custom:cursor"] {
        assert!(OfficialQuotaProvider::parse(id).is_none());
        assert!(quota::parse_provider(id)
            .unwrap_err()
            .contains("未知的官方额度账号"));
        assert!(custom::is_custom_id(id));
    }
    for provider in OfficialQuotaProvider::ALL {
        assert!(!custom::is_custom_id(provider.as_str()));
    }

    let id = store::new_provider_id(&[]);
    assert!(id.starts_with("custom:"));
    assert!(OfficialQuotaProvider::parse(&id).is_none());
    // 撞上已有标识时换一个，而不是覆盖别人的缓存。
    let next = store::new_provider_id(std::slice::from_ref(&id));
    assert_ne!(next, id);
}

// -------------------------------------------------- 接缝一：加载配置与凭证

#[test]
fn loading_pairs_each_provider_with_its_secret() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());

    store::save_config(
        &paths.config,
        &CustomQuotaConfig {
            providers: vec![provider("custom:a3f9c1", "公司的中转")],
        },
    )
    .unwrap();
    let mut credentials = CustomQuotaCredentials::default();
    credentials
        .secrets
        .insert("custom:a3f9c1".to_string(), "sk-relay-123456".to_string());
    store::save_credentials(&paths.credentials, &credentials).unwrap();

    let loaded = store::load_providers(&paths);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].config.name, "公司的中转");
    assert_eq!(loaded[0].secret.as_deref(), Some("sk-relay-123456"));
}

#[test]
fn loading_keeps_config_when_the_credential_file_is_missing() {
    // 换机器恢复备份后的形状：配置跟过来了，密钥没有（它刻意不进备份）。
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    store::save_config(
        &paths.config,
        &CustomQuotaConfig {
            providers: vec![provider("custom:a3f9c1", "公司的中转")],
        },
    )
    .unwrap();

    let loaded = store::load_providers(&paths);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].config.base_url, "https://relay.example.com");
    assert_eq!(loaded[0].secret, None);
    // 没密钥就取不了数，但要说人话，而且不能 panic。
    assert_eq!(
        custom::fetch(&loaded[0]).unwrap_err(),
        "未配置密钥，请在设置页重新填写"
    );
}

#[test]
fn loading_survives_missing_and_corrupt_files() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    // 两份文件都不存在：空列表，不是错误。
    assert!(store::load_providers(&paths).is_empty());

    // 配置坏了不该让整个额度区块打不开。
    std::fs::write(&paths.config, "{not json").unwrap();
    std::fs::write(&paths.credentials, "also broken").unwrap();
    assert!(store::load_providers(&paths).is_empty());
}

#[test]
fn secrets_are_only_ever_handed_out_masked() {
    assert_eq!(
        store::mask_secret(Some("sk-relay-abcdef123456")).as_deref(),
        Some("••••••3456")
    );
    // 太短的连尾巴都不给，否则掩码就是原文。
    assert_eq!(store::mask_secret(Some("abc")).as_deref(), Some("••••••"));
    assert_eq!(store::mask_secret(None), None);
    assert_eq!(store::mask_secret(Some("   ")), None);
}

// -------------------------------------------------- 接缝二：构造请求地址

#[test]
fn base_url_normalizes_the_four_ways_people_type_it() {
    let expected = custom::request_urls(
        CustomQuotaPreset::OpenAiCompatible,
        "https://relay.example.com",
        today(),
    )
    .unwrap();
    for raw in [
        "https://relay.example.com/",
        "https://relay.example.com/v1",
        "https://relay.example.com/v1/",
        "  https://relay.example.com  ",
    ] {
        let actual =
            custom::request_urls(CustomQuotaPreset::OpenAiCompatible, raw, today()).unwrap();
        assert_eq!(actual, expected, "{raw} 应该和根地址落到同一个地址");
    }
    assert_eq!(
        expected[0],
        "https://relay.example.com/v1/dashboard/billing/subscription"
    );
    assert!(expected[1].starts_with("https://relay.example.com/v1/dashboard/billing/usage?"));
}

#[test]
fn base_url_rejects_shapes_that_can_never_work() {
    for (raw, hint) in [
        ("", "请填写 base URL"),
        ("   ", "请填写 base URL"),
        ("relay.example.com", "http:// 或 https://"),
        ("https://", "缺少域名"),
    ] {
        let error = custom::normalize_base_url(raw).unwrap_err();
        assert!(error.contains(hint), "{raw} 的报错读不懂：{error}");
    }
}

#[test]
fn unimplemented_presets_say_so_instead_of_panicking() {
    for preset in CustomQuotaPreset::ALL {
        if preset == CustomQuotaPreset::OpenAiCompatible {
            assert!(preset.implemented());
            continue;
        }
        assert!(!preset.implemented());
        let urls = custom::request_urls(preset, "https://relay.example.com", today());
        assert!(
            urls.as_ref().unwrap_err().contains("暂未支持"),
            "{preset:?}"
        );
        assert!(urls.unwrap_err().contains(preset.display_name()));
        assert!(custom::parse_quota(preset, &[SUBSCRIPTION, USAGE])
            .unwrap_err()
            .contains("暂未支持"));
    }
}

#[test]
fn preset_ids_round_trip_through_the_config_file() {
    for preset in CustomQuotaPreset::ALL {
        assert_eq!(CustomQuotaPreset::parse(preset.as_str()), Some(preset));
        let json = serde_json::to_string(&preset).unwrap();
        assert_eq!(json, format!("\"{}\"", preset.as_str()));
    }
    assert_eq!(CustomQuotaPreset::parse("nope"), None);
}

// -------------------------------------------------- 接缝三：解析额度响应

#[test]
fn openai_compatible_reads_percent_and_amount_together() {
    let windows =
        custom::parse_quota(CustomQuotaPreset::OpenAiCompatible, &[SUBSCRIPTION, USAGE]).unwrap();
    assert_eq!(windows.len(), 1);
    // 已用 $19（1900 美分）/ 共 $50 = 38%
    assert_eq!(windows[0].used_amount, Some(19.0));
    assert_eq!(windows[0].limit_amount, Some(50.0));
    assert_eq!(windows[0].used_percent, Some(38.0));
    assert_eq!(windows[0].currency.as_deref(), Some("USD"));
}

#[test]
fn openai_compatible_degrades_to_amount_only_without_a_limit() {
    for subscription in [
        r#"{"object":"billing_subscription"}"#,
        r#"{"hard_limit_usd":0}"#,
    ] {
        let windows =
            custom::parse_quota(CustomQuotaPreset::OpenAiCompatible, &[subscription, USAGE])
                .unwrap();
        assert_eq!(windows[0].used_amount, Some(19.0));
        assert_eq!(windows[0].limit_amount, None);
        // 画不出进度条，但这一行仍然有用。
        assert_eq!(windows[0].used_percent, None);
    }

    // 只有 system_hard_limit_usd 的站点也要认。
    let windows = custom::parse_quota(
        CustomQuotaPreset::OpenAiCompatible,
        &[r#"{"system_hard_limit_usd":"200"}"#, USAGE],
    )
    .unwrap();
    assert_eq!(windows[0].limit_amount, Some(200.0));
}

#[test]
fn openai_compatible_clamps_overspend_to_a_full_bar() {
    let windows = custom::parse_quota(
        CustomQuotaPreset::OpenAiCompatible,
        &[r#"{"hard_limit_usd":10}"#, r#"{"total_usage":5000}"#],
    )
    .unwrap();
    // 超支要满格，不是「算出 500% 所以干脆不画」。
    assert_eq!(windows[0].used_percent, Some(100.0));
    assert_eq!(windows[0].used_amount, Some(50.0));
}

#[test]
fn openai_compatible_turns_bad_responses_into_readable_chinese() {
    let cases: [(&str, &str, &str); 5] = [
        ("", USAGE, "空响应"),
        (SUBSCRIPTION, "", "空响应"),
        ("<html>login</html>", USAGE, "不是合法 JSON"),
        (SUBSCRIPTION, "{not json", "不是合法 JSON"),
        // 结构变更：字段整个消失。
        (SUBSCRIPTION, r#"{"object":"list"}"#, "total_usage"),
    ];
    for (subscription, usage, hint) in cases {
        let error =
            custom::parse_quota(CustomQuotaPreset::OpenAiCompatible, &[subscription, usage])
                .unwrap_err();
        assert!(error.contains(hint), "报错读不懂：{error}");
        // 一律是人话，不是裸的英文异常。
        assert!(!error.contains("panicked"));
    }
    // 少一个响应体也不能炸。
    assert!(custom::parse_quota(CustomQuotaPreset::OpenAiCompatible, &[]).is_err());
}

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
    // 带重置时间的窗口——告警去重键要的就是它。本版实现的 OpenAI 兼容计费给不出
    // 重置时间（见下一条测试），后续预设会给。
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

/// 已知缺口，本票不修：告警去重键含重置时间，而 OpenAI 兼容计费的窗口没有
/// 重置时间——现有 `prepare_notifications` 对这种窗口整条跳过，因此它不告警。
/// 这条测试把现状钉住，等做余额阈值告警时一并处理（见 #81 Further Notes）。
#[test]
fn amount_windows_without_a_reset_time_do_not_alert_yet() {
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
        [("custom:a3f9c1".to_string(), Ok((windows, now.to_rfc3339())))],
    )
    .unwrap();

    let dto = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[provider("custom:a3f9c1", "公司的中转")],
        now,
    );
    let (_, alerts) =
        quota::notify::prepare_notifications(quota::notify::NotifyState::default(), &dto);
    assert!(alerts.is_empty());
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

// -------------------------------------------------- 单条刷新与设置页命令

#[test]
fn single_refresh_resolves_builtin_first_then_falls_back_to_custom() {
    let resolved = vec![custom::ResolvedProvider {
        config: provider("custom:a3f9c1", "公司的中转"),
        secret: Some("sk-relay".to_string()),
    }];
    // 内置那 9 个照旧走枚举。
    match quota::resolve_target("claude", &resolved).unwrap() {
        quota::FetchTarget::Builtin(provider) => {
            assert_eq!(provider, OfficialQuotaProvider::Claude)
        }
        quota::FetchTarget::Custom(_) => panic!("claude 不该走自定义通道"),
    }
    // `custom:` 标识不再撞上「未知的官方额度账号」。
    match quota::resolve_target("custom:a3f9c1", &resolved).unwrap() {
        quota::FetchTarget::Custom(provider) => assert_eq!(provider.config.name, "公司的中转"),
        quota::FetchTarget::Builtin(_) => panic!("自定义标识不该被内置枚举吃掉"),
    }
    // 删掉之后再点刷新，报的是「不认识」，不是崩溃。
    assert!(quota::resolve_target("custom:deleted", &resolved)
        .unwrap_err()
        .contains("未知的官方额度账号"));
}

/// 关掉 = 不取数。手动刷新也不是例外，否则「关掉就不再消耗它的调用配额」是句空话。
#[test]
fn disabled_custom_providers_refuse_a_manual_refresh_by_name() {
    let mut config = provider("custom:a3f9c1", "备用中转");
    config.enabled = false;
    let resolved = vec![custom::ResolvedProvider {
        config,
        secret: Some("sk-relay".to_string()),
    }];
    let error = quota::resolve_target("custom:a3f9c1", &resolved).unwrap_err();
    // 说清楚是「停用了」而不是「不认识」——后者会让人以为配置丢了。
    assert!(error.contains("备用中转"), "{error}");
    assert!(error.contains("已停用"), "{error}");
}

/// 设置页现在没有启停开关，保存时不带 `enabled`。带上就会在改名时把用户
/// 手动关掉的那条悄悄打开。
#[test]
fn saving_without_the_enabled_flag_leaves_the_switch_alone() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    let mut off = provider("custom:a3f9c1", "备用中转");
    off.enabled = false;
    store::save_config(
        &paths.config,
        &CustomQuotaConfig {
            providers: vec![off],
        },
    )
    .unwrap();

    let after = panel::save(
        &paths,
        panel::SaveCustomQuotaProvider {
            id: Some("custom:a3f9c1".to_string()),
            name: "改了个名".to_string(),
            preset: CustomQuotaPreset::OpenAiCompatible,
            base_url: "https://relay.example.com".to_string(),
            enabled: None,
            secret: Some("sk-relay".to_string()),
        },
    )
    .unwrap();
    assert_eq!(after.panel.providers[0].name, "改了个名");
    assert!(
        !after.panel.providers[0].enabled,
        "改名不该把停用的那条打开"
    );

    // 显式带上就照办。
    let reopened = panel::save(
        &paths,
        panel::SaveCustomQuotaProvider {
            id: Some("custom:a3f9c1".to_string()),
            name: "改了个名".to_string(),
            preset: CustomQuotaPreset::OpenAiCompatible,
            base_url: "https://relay.example.com".to_string(),
            enabled: Some(true),
            secret: None,
        },
    )
    .unwrap();
    assert!(reopened.panel.providers[0].enabled);
}

#[test]
fn panel_saves_edits_and_deletes_without_ever_echoing_the_secret() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());

    let saved = panel::save(
        &paths,
        panel::SaveCustomQuotaProvider {
            id: None,
            name: "  公司的中转  ".to_string(),
            preset: CustomQuotaPreset::OpenAiCompatible,
            base_url: "https://relay.example.com/v1".to_string(),
            enabled: None,
            secret: Some("sk-relay-abcdef123456".to_string()),
        },
    )
    .unwrap();
    assert_eq!(saved.panel.providers.len(), 1);
    let created = &saved.panel.providers[0];
    assert_eq!(created.name, "公司的中转");
    assert!(created.id.starts_with("custom:"));
    // 存完就把标识交回去，界面据此立刻取一次这一条的额度。
    assert_eq!(saved.saved_id, created.id);
    assert_eq!(created.secret_mask.as_deref(), Some("••••••3456"));
    // 用户打的那串原样留着，不被应用悄悄改写。
    assert_eq!(created.base_url, "https://relay.example.com/v1");
    // 六种预设全部露出来，没实现的标好。
    assert_eq!(saved.panel.presets.len(), 6);
    assert!(saved.panel.presets.iter().filter(|p| p.supported).count() == 1);

    // 改名 + 换域名，密钥留空 = 不改。标识不动。
    let id = created.id.clone();
    let renamed = panel::save(
        &paths,
        panel::SaveCustomQuotaProvider {
            id: Some(id.clone()),
            name: "老板的中转".to_string(),
            preset: CustomQuotaPreset::OpenAiCompatible,
            base_url: "https://new.example.com".to_string(),
            enabled: None,
            secret: None,
        },
    )
    .unwrap();
    assert_eq!(renamed.saved_id, id);
    assert_eq!(renamed.panel.providers[0].id, id);
    assert_eq!(renamed.panel.providers[0].name, "老板的中转");
    assert_eq!(
        renamed.panel.providers[0].base_url,
        "https://new.example.com"
    );
    assert_eq!(
        renamed.panel.providers[0].secret_mask.as_deref(),
        Some("••••••3456")
    );

    // 轮换密钥。
    panel::save(
        &paths,
        panel::SaveCustomQuotaProvider {
            id: Some(id.clone()),
            name: "老板的中转".to_string(),
            preset: CustomQuotaPreset::OpenAiCompatible,
            base_url: "https://new.example.com".to_string(),
            enabled: None,
            secret: Some("sk-rotated-999999".to_string()),
        },
    )
    .unwrap();
    let loaded = store::load_providers(&paths);
    assert_eq!(loaded[0].secret.as_deref(), Some("sk-rotated-999999"));

    let after = panel::delete(&paths, &id).unwrap();
    assert!(after.providers.is_empty());
    // 密钥跟着一起走，不留在磁盘上。
    assert!(!store::load_credentials(&paths.credentials)
        .secrets
        .contains_key(&id));
    assert!(panel::delete(&paths, &id)
        .unwrap_err()
        .contains("已经不在了"));
}

#[test]
fn panel_blocks_only_the_things_the_user_can_fix_by_typing() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    let base = panel::SaveCustomQuotaProvider {
        id: None,
        name: "中转".to_string(),
        preset: CustomQuotaPreset::OpenAiCompatible,
        base_url: "https://relay.example.com".to_string(),
        enabled: None,
        secret: Some("sk-relay".to_string()),
    };

    let blank_name = panel::SaveCustomQuotaProvider {
        name: "  ".to_string(),
        ..base.clone()
    };
    assert_eq!(panel::save(&paths, blank_name).unwrap_err(), "请填写名称");

    let bad_url = panel::SaveCustomQuotaProvider {
        base_url: "relay.example.com".to_string(),
        ..base.clone()
    };
    assert!(panel::save(&paths, bad_url)
        .unwrap_err()
        .contains("http:// 或 https://"));

    let no_secret = panel::SaveCustomQuotaProvider {
        secret: None,
        ..base.clone()
    };
    assert_eq!(panel::save(&paths, no_secret).unwrap_err(), "请填写密钥");

    // 没有一条存进去：挡下的都是填不动的错，不该留下半条记录。
    assert!(store::load_providers(&paths).is_empty());

    // 没实现的预设照存不误——保存不做取数拦截，取数时才给「暂未支持」。
    let unsupported = panel::SaveCustomQuotaProvider {
        preset: CustomQuotaPreset::DeepSeek,
        ..base
    };
    let saved = panel::save(&paths, unsupported).unwrap();
    assert_eq!(saved.panel.providers[0].preset, CustomQuotaPreset::DeepSeek);
    let loaded = store::load_providers(&paths);
    assert!(custom::fetch(&loaded[0]).unwrap_err().contains("暂未支持"));
}

/// 没打网就失败的不进退避：否则恢复备份后刚填完密钥、或刚存下一个未实现的预设，
/// 再点刷新只会看到「刚取数失败，N 分钟后自动重试」，把真正的原因盖掉。
#[test]
fn failures_that_never_touched_the_network_do_not_trigger_a_cooldown() {
    let missing_secret = custom::ResolvedProvider {
        config: provider("custom:a3f9c1", "公司的中转"),
        secret: None,
    };
    let mut unsupported_preset = provider("custom:b7e204", "另一个中转");
    unsupported_preset.preset = CustomQuotaPreset::DeepSeek;
    let unsupported_preset = custom::ResolvedProvider {
        config: unsupported_preset,
        secret: Some("sk-relay".to_string()),
    };

    for target in [&missing_secret, &unsupported_preset] {
        let blocked = custom::precheck(target).expect("这两种都该在打网之前就被拦下");
        assert!(custom::is_precheck_error(&blocked), "{blocked}");
        // 走完整条 fetch 也是同一句话，而且不 panic。
        assert_eq!(custom::fetch(target).unwrap_err(), blocked);
    }

    // 真正打过网的失败照旧退避。
    assert!(!custom::is_precheck_error("网络不通，连不上这个地址"));
    assert!(!custom::is_precheck_error(
        "密钥无效或已失效，请在设置页更新密钥"
    ));
}
