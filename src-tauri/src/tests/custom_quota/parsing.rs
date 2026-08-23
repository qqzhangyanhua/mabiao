//! 三个新接缝：标识、加载配置与凭证、构造请求地址、解析额度响应。

use super::{provider, today, SUBSCRIPTION, USAGE};
use crate::domain::OfficialQuotaProvider;
use crate::official_quota::custom::store::{CustomQuotaConfig, CustomQuotaCredentials};
use crate::official_quota::custom::{self, store, CustomQuotaPreset};
use crate::official_quota::{self as quota};

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
        expected[0].url,
        "https://relay.example.com/v1/dashboard/billing/subscription"
    );
    // 上限接口是可选的、已用那条不是：只实现了用量接口的中转站仍然该显示金额。
    assert!(!expected[0].required);
    assert!(expected[1]
        .url
        .starts_with("https://relay.example.com/v1/dashboard/billing/usage?"));
    assert!(expected[1].required);
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
    // 只列用量那侧：上限那侧坏掉是降级、不是失败，见下一条测试。
    let cases: [(&str, &str, &str); 3] = [
        (SUBSCRIPTION, "", "空响应"),
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

/// 上限接口在 `urls()` 里标着 `required: false`，取数时它失败会被换成空串。
/// 因此「上限拿不到」必须一路降级成只报金额——否则那个标记等于没有，
/// 只实现了用量接口的中转站会整行取不到数。
#[test]
fn a_missing_limit_endpoint_degrades_instead_of_failing() {
    for subscription in ["", "<html>login</html>"] {
        let windows =
            custom::parse_quota(CustomQuotaPreset::OpenAiCompatible, &[subscription, USAGE])
                .expect("上限接口拿不到不该让整次取数失败");
        assert_eq!(windows[0].used_amount, Some(19.0));
        assert_eq!(windows[0].limit_amount, None);
        assert_eq!(windows[0].used_percent, None);
    }
}
