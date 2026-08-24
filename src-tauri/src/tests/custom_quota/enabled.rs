//! #87：自定义提供商停用开关。关掉就不取数、不占行、不告警；配置和密钥都留着。

use super::{provider, resolved};
use crate::domain::{OfficialQuotaConfig, OfficialQuotaWindow};
use crate::official_quota::custom::store::CustomQuotaConfig;
use crate::official_quota::custom::{panel, store, CustomQuotaPreset};
use crate::official_quota::notify;
use crate::official_quota::{self as quota, QuotaTarget};
use crate::store as db;

fn hot_window() -> OfficialQuotaWindow {
    OfficialQuotaWindow {
        kind: "billing_cycle".into(),
        label: "总量".into(),
        used_percent: Some(90.0),
        resets_at: Some("2026-09-01T00:00:00+00:00".into()),
        used_amount: Some(45.0),
        limit_amount: Some(50.0),
        currency: Some("USD".into()),
        ..Default::default()
    }
}

/// 整体刷新的取数目标集合。关掉的那条就算有密钥，也不能出现在里面。
#[test]
fn disabled_custom_providers_are_not_in_the_fetch_set() {
    let mut off = resolved("custom:a3f9c1", "备用中转");
    off.config.enabled = false;
    let targets = quota::custom_targets_for_fetch(&[off, resolved("custom:b7e204", "在用的")]);
    let ids: Vec<&str> = targets.iter().map(|target| target.quota_id()).collect();
    assert_eq!(ids, vec!["custom:b7e204"]);
}

#[test]
fn disabling_keeps_name_preset_url_and_secret() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    let created = panel::save(
        &paths,
        panel::SaveCustomQuotaProvider {
            id: None,
            name: "备用中转".to_string(),
            preset: CustomQuotaPreset::OpenAiCompatible,
            base_url: "https://relay.example.com/v1".to_string(),
            enabled: None,
            secret: Some("sk-relay-abcdef123456".to_string()),
        },
    )
    .unwrap();
    let id = created.saved_id.clone();

    let off = panel::save(
        &paths,
        panel::SaveCustomQuotaProvider {
            id: Some(id.clone()),
            name: "备用中转".to_string(),
            preset: CustomQuotaPreset::OpenAiCompatible,
            base_url: "https://relay.example.com/v1".to_string(),
            enabled: Some(false),
            secret: None,
        },
    )
    .unwrap();
    let listed = &off.panel.providers[0];
    assert!(!listed.enabled);
    assert_eq!(listed.id, id);
    assert_eq!(listed.name, "备用中转");
    assert_eq!(listed.preset, CustomQuotaPreset::OpenAiCompatible);
    assert_eq!(listed.base_url, "https://relay.example.com/v1");
    assert_eq!(listed.secret_mask.as_deref(), Some("••••••3456"));

    let loaded = store::load_providers(&paths);
    assert_eq!(loaded.len(), 1);
    assert!(!loaded[0].config.enabled);
    assert_eq!(loaded[0].secret.as_deref(), Some("sk-relay-abcdef123456"));

    let on = panel::save(
        &paths,
        panel::SaveCustomQuotaProvider {
            id: Some(id),
            name: "备用中转".to_string(),
            preset: CustomQuotaPreset::OpenAiCompatible,
            base_url: "https://relay.example.com/v1".to_string(),
            enabled: Some(true),
            secret: None,
        },
    )
    .unwrap();
    assert!(on.panel.providers[0].enabled);
    assert_eq!(on.panel.providers[0].name, "备用中转");
    assert_eq!(
        on.panel.providers[0].base_url,
        "https://relay.example.com/v1"
    );
    assert_eq!(
        on.panel.providers[0].secret_mask.as_deref(),
        Some("••••••3456")
    );
    assert_eq!(
        store::load_providers(&paths)[0].secret.as_deref(),
        Some("sk-relay-abcdef123456")
    );
}

/// 关掉之后连 DTO 行都没有，告警扫不到它。缓存里哪怕是 90% 也不该再弹。
#[test]
fn disabled_custom_providers_do_not_alert() {
    let conn = db::open_memory().unwrap();
    let now = chrono::Utc::now();
    quota::apply_fetch_results(
        &conn,
        [(
            "custom:a3f9c1".to_string(),
            Ok((vec![hot_window()], now.to_rfc3339())),
        )],
    )
    .unwrap();

    let on = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        &[resolved("custom:a3f9c1", "备用中转")],
        now,
    );
    let alerts = notify::prepare_notifications(notify::NotifyState::default(), &on).1;
    assert_eq!(
        alerts.len(),
        1,
        "启用时 90% 应该告警，否则这条测的前提不成立"
    );

    let mut off = resolved("custom:a3f9c1", "备用中转");
    off.config.enabled = false;
    let snapshot = quota::load_dto(
        &conn,
        &OfficialQuotaConfig::default(),
        std::slice::from_ref(&off),
        now,
    );
    assert!(
        !snapshot
            .rows
            .iter()
            .any(|row| row.provider == "custom:a3f9c1"),
        "关掉就不占行"
    );
    let alerts = notify::prepare_notifications(notify::NotifyState::default(), &snapshot).1;
    assert!(alerts.is_empty(), "关掉之后不该再按缓存里的 90% 告警");
}

/// 开关落在配置文件里，不是只活在这次进程的内存里。
#[test]
fn enabled_flag_round_trips_through_the_config_file() {
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

    let loaded = store::load_config(&paths.config);
    assert!(!loaded.providers[0].enabled);
    assert_eq!(loaded.providers[0].name, "备用中转");
    assert_eq!(loaded.providers[0].base_url, "https://relay.example.com");
}
