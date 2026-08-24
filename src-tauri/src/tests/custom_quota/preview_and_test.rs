//! 填对地址：base URL 回显与「测试连接」。
//!
//! 两个新接缝都刻意不打网：回显是纯计算，测试连接的组装（表单草稿 → 可取数的
//! 一条）与真正的 HTTP 请求分开，因此这里能把「用哪份配置、用哪把密钥」全部测掉，
//! 剩下的只有 `custom::fetch` 里那一次请求。

use super::{provider, today};
use crate::official_quota::custom::store::{CustomQuotaConfig, CustomQuotaCredentials};
use crate::official_quota::custom::{self, panel, store, CustomQuotaPreset};

fn draft(base_url: &str, secret: Option<&str>) -> panel::TestCustomQuotaProvider {
    panel::TestCustomQuotaProvider {
        id: None,
        preset: CustomQuotaPreset::OpenAiCompatible,
        base_url: base_url.to_string(),
        secret: secret.map(str::to_string),
    }
}

fn stored(dir: &std::path::Path) -> store::CustomQuotaPaths {
    let paths = store::CustomQuotaPaths::in_dir(dir);
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
        .insert("custom:a3f9c1".to_string(), "sk-stored-123456".to_string());
    store::save_credentials(&paths.credentials, &credentials).unwrap();
    paths
}

// -------------------------------------------------- 接缝一：回显完整请求地址

/// 回显存在的唯一目的是「点保存之前就知道填对没有」。因此它必须**就是**
/// 取数时真正请求的那几条地址——两份实现哪天只改了一边，回显就开始骗人。
#[test]
fn the_echoed_address_is_the_one_the_fetch_will_actually_request() {
    let preset = CustomQuotaPreset::OpenAiCompatible;
    let base = "https://relay.example.com";
    let preview = panel::preview_requests(preset, base, today());
    assert_eq!(preview.error, None);
    assert_eq!(
        preview.requests,
        custom::request_urls(preset, base, today()).unwrap(),
        "回显与取数必须是同一份归一化，不能各写一遍"
    );
    assert_eq!(
        preview.requests[0].url,
        "https://relay.example.com/v1/dashboard/billing/subscription"
    );
}

/// 用户会把根地址打成带 `/v1`、带结尾斜杠、或者两者都带。四种写法归到同一个
/// 地址，否则这个回显只是把用户的笔误原样念一遍。
#[test]
fn the_four_ways_people_type_a_base_url_echo_the_same_address() {
    let preset = CustomQuotaPreset::OpenAiCompatible;
    let expected = panel::preview_requests(preset, "https://relay.example.com", today());
    for raw in [
        "https://relay.example.com/",
        "https://relay.example.com/v1",
        "https://relay.example.com/v1/",
        "  https://relay.example.com/v1/  ",
    ] {
        assert_eq!(
            panel::preview_requests(preset, raw, today()),
            expected,
            "{raw} 应该和根地址回显同一个地址"
        );
    }
}

/// 边打边回显，必然会路过「还没打完」的中间态。那时该给一句提示，
/// 而不是把半截地址当成一个能用的地址念出来。
#[test]
fn a_half_typed_address_echoes_a_hint_instead_of_a_fake_address() {
    for (raw, hint) in [
        ("", "请填写 base URL"),
        ("   ", "请填写 base URL"),
        ("relay.example.com", "http:// 或 https://"),
        ("https://", "缺少域名"),
    ] {
        let preview = panel::preview_requests(CustomQuotaPreset::OpenAiCompatible, raw, today());
        assert!(preview.requests.is_empty(), "{raw} 不该回显出地址");
        let error = preview.error.expect("半截地址要给提示");
        assert!(error.contains(hint), "{raw} 的提示读不懂：{error}");
    }
}

/// 没实现的预设根本拼不出地址。回显要说的是这件事，而不是空着让用户以为
/// 自己地址填错了。
#[test]
fn an_unimplemented_preset_says_so_in_the_echo_too() {
    let preview = panel::preview_requests(
        CustomQuotaPreset::DeepSeek,
        "https://relay.example.com",
        today(),
    );
    assert!(preview.requests.is_empty());
    assert!(preview.error.unwrap().contains("暂未支持"));
}

// -------------------------------------------------- 接缝二：测试连接

/// 「测试连接」用的是表单里**尚未保存**的配置——用户点它正是为了在保存之前
/// 确认填对没有。读磁盘上那份旧的等于测了个寂寞。
#[test]
fn testing_uses_the_unsaved_form_config_not_what_is_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let paths = stored(dir.path());

    let secret = panel::resolve_secret(
        &paths,
        &panel::TestCustomQuotaProvider {
            id: Some("custom:a3f9c1".to_string()),
            secret: Some("sk-typed-just-now".to_string()),
            ..draft("https://new.example.com/v1/", None)
        },
    )
    .unwrap();
    assert_eq!(secret, "sk-typed-just-now");

    // 磁盘一个字没动：测试连接不是保存，失败了也不该在磁盘上留下半条记录。
    let on_disk = store::load_providers(&paths);
    assert_eq!(on_disk[0].config.base_url, "https://relay.example.com");
    assert_eq!(on_disk[0].secret.as_deref(), Some("sk-stored-123456"));
}

/// 编辑已存的那条时密钥框是空的——界面上只有掩码，用户重打不出来。空着就沿用
/// 已存的那把，否则「改完域名点一下测试」这个最常见的用法直接不成立。
#[test]
fn a_blank_secret_falls_back_to_the_stored_one_when_editing() {
    let dir = tempfile::tempdir().unwrap();
    let paths = stored(dir.path());

    for secret in [None, Some("   ")] {
        let resolved = panel::resolve_secret(
            &paths,
            &panel::TestCustomQuotaProvider {
                id: Some("custom:a3f9c1".to_string()),
                secret: secret.map(str::to_string),
                ..draft("https://relay.example.com", None)
            },
        )
        .unwrap();
        assert_eq!(resolved, "sk-stored-123456");
    }
}

/// 新建时没有可回落的那把钥匙。说「请填写密钥」，不要拿一个空密钥去打网、
/// 再把对方回的 401 当成「密钥无效」——那是两回事。
#[test]
fn testing_a_brand_new_provider_without_a_secret_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    assert_eq!(
        panel::resolve_secret(&paths, &draft("https://relay.example.com", None)).unwrap_err(),
        "请填写密钥"
    );
    // 标识存在但磁盘上没有它（多半是恢复备份后密钥没跟过来）也是同一句话。
    assert_eq!(
        panel::resolve_secret(
            &paths,
            &panel::TestCustomQuotaProvider {
                id: Some("custom:a3f9c1".to_string()),
                ..draft("https://relay.example.com", None)
            }
        )
        .unwrap_err(),
        "请填写密钥"
    );
}

/// 填不动的错在打网之前就该认出来，报的是「地址不成形」「这个类型没实现」，
/// 而不是等对方回一个读不懂的 HTTP 码。
///
/// 顺带钉住取数入口的形状：草稿没有标识、没有名称、也还没保存，`fetch_quota`
/// 照样能打——它只认预设类型、地址、密钥这三样。
#[test]
fn testing_a_shape_that_can_never_work_fails_before_touching_the_network() {
    let malformed = custom::fetch_quota(
        CustomQuotaPreset::OpenAiCompatible,
        "relay.example.com",
        Some("sk-relay"),
    );
    assert!(malformed.unwrap_err().contains("http:// 或 https://"));

    let error = custom::fetch_quota(
        CustomQuotaPreset::Moonshot,
        "https://relay.example.com",
        Some("sk-relay"),
    )
    .unwrap_err();
    assert!(error.contains("暂未支持"), "{error}");
    // 这两种都没碰到对方，因此也不该被记进退避。
    assert!(custom::is_precheck_error(&error));

    // 密钥缺席同样在打网之前就拦下。
    assert!(custom::is_precheck_error(
        &custom::fetch_quota(
            CustomQuotaPreset::OpenAiCompatible,
            "https://relay.example.com",
            None,
        )
        .unwrap_err()
    ));
}
