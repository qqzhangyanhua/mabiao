//! 单条刷新与设置页命令：解析标识、保存 / 编辑 / 删除、退避。

use super::provider;
use crate::domain::OfficialQuotaProvider;
use crate::official_quota::custom::store::CustomQuotaConfig;
use crate::official_quota::custom::{self, panel, store, CustomQuotaPreset};
use crate::official_quota::{self as quota};

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

/// 保存之后必须能立刻重试。密钥失效会连着失败几次、把这一条压进退避；
/// 用户轮换密钥正是为了修好它，此时若还拿旧的冷却拦着，保存后那次刷新
/// 只会回「刚取数失败，N 分钟后自动重试」——用户会以为改了没用。
#[test]
fn saving_a_provider_forgets_its_cooldown_so_a_rotated_key_takes_effect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backoff.json");
    let now = chrono::Utc::now();

    let mut state = quota::backoff::BackoffState::default();
    for _ in 0..3 {
        quota::backoff::record_failure(
            &mut state,
            "custom:a3f9c1",
            "密钥无效或已失效，请在设置页更新密钥",
            now,
        );
    }
    quota::backoff::save_state(&path, &state).unwrap();
    assert!(
        quota::backoff::cooldown_remaining(
            &quota::backoff::load_state(&path),
            "custom:a3f9c1",
            now
        )
        .is_some(),
        "前提没成立：这一条本该正在冷却"
    );

    quota::backoff::clear(&path, "custom:a3f9c1");

    assert!(
        quota::backoff::cooldown_remaining(
            &quota::backoff::load_state(&path),
            "custom:a3f9c1",
            now
        )
        .is_none(),
        "保存之后这一条还在冷却，轮换密钥就白做了"
    );
    // 只忘掉这一条，别人的冷却不受牵连。
    let mut other = quota::backoff::BackoffState::default();
    quota::backoff::record_failure(&mut other, "claude", "网络不通", now);
    quota::backoff::save_state(&path, &other).unwrap();
    quota::backoff::clear(&path, "custom:a3f9c1");
    assert!(
        quota::backoff::cooldown_remaining(&quota::backoff::load_state(&path), "claude", now)
            .is_some()
    );
}
