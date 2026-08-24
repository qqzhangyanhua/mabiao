//! #89：备份带走自定义提供商配置，不带走凭证；恢复不动本机已有的密钥文件。

use super::provider;
use crate::official_quota::custom::store::{
    self as custom_store, CustomQuotaConfig, CustomQuotaCredentials, CONFIG_NAME, CREDENTIAL_NAME,
};
use crate::test_support::*;

fn live_paths(live: &std::path::Path) -> backup::AppDataPaths {
    backup::AppDataPaths {
        db_path: live.join("usage.sqlite"),
        prices_path: live.join("prices.json"),
        snapshot_path: live.join("litellm_prices.json"),
        budget_path: live.join("budget.json"),
        budget_notify_path: live.join("budget_notify_state.json"),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    }
}

fn write_provider_files(dir: &std::path::Path, secret: &str) {
    custom_store::save_config(
        &dir.join(CONFIG_NAME),
        &CustomQuotaConfig {
            providers: vec![provider("custom:a3f9c1", "公司的中转")],
        },
    )
    .unwrap();
    let mut credentials = CustomQuotaCredentials::default();
    credentials
        .secrets
        .insert("custom:a3f9c1".to_string(), secret.to_string());
    custom_store::save_credentials(&dir.join(CREDENTIAL_NAME), &credentials).unwrap();
}

#[test]
fn backup_keeps_custom_provider_config_and_drops_the_secret_file() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();
    write_provider_files(&live, "sk-live-secret");

    let paths = live_paths(&live);
    let conn = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    let manifest = backup::backup_to(&conn, &dest, &paths).unwrap();

    assert!(
        manifest.files.iter().any(|name| name == CONFIG_NAME),
        "备份清单应包含自定义提供商配置：{manifest:?}"
    );
    assert!(
        !manifest.files.iter().any(|name| name == CREDENTIAL_NAME),
        "备份清单不得包含凭证文件：{manifest:?}"
    );
    assert!(dest.join(CONFIG_NAME).exists(), "备份目录应有配置文件");
    assert!(
        !dest.join(CREDENTIAL_NAME).exists(),
        "备份目录不得出现凭证文件"
    );
    assert!(
        !std::fs::read_to_string(dest.join(CONFIG_NAME))
            .unwrap()
            .contains("sk-live-secret"),
        "配置文件里也不该写出密钥"
    );
    assert!(manifest.note.contains("密钥"), "{}", manifest.note);
}

#[test]
fn restore_leaves_the_local_secret_file_alone() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();

    let paths = live_paths(&live);
    let conn = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    custom_store::save_config(
        &live.join(CONFIG_NAME),
        &CustomQuotaConfig {
            providers: vec![provider("custom:a3f9c1", "备份里的名字")],
        },
    )
    .unwrap();
    backup::backup_to(&conn, &dest, &paths).unwrap();
    drop(conn);

    // 恢复前本机已经有另一套名称和密钥。配置应当被覆盖，密钥必须原样留下。
    custom_store::save_config(
        &live.join(CONFIG_NAME),
        &CustomQuotaConfig {
            providers: vec![provider("custom:local", "本机现有的")],
        },
    )
    .unwrap();
    let mut local = CustomQuotaCredentials::default();
    local
        .secrets
        .insert("custom:local".to_string(), "sk-machine-local".to_string());
    custom_store::save_credentials(&live.join(CREDENTIAL_NAME), &local).unwrap();
    // 即便有人把密钥文件塞进备份目录，恢复也不该把它写进本机。
    std::fs::write(
        dest.join(CREDENTIAL_NAME),
        r#"{"secrets":{"custom:a3f9c1":"sk-from-backup"}}"#,
    )
    .unwrap();

    backup::restore_from(&dest, &paths).unwrap();

    let restored_config = custom_store::load_config(&live.join(CONFIG_NAME));
    assert_eq!(restored_config.providers.len(), 1);
    assert_eq!(restored_config.providers[0].id, "custom:a3f9c1");
    assert_eq!(restored_config.providers[0].name, "备份里的名字");

    let restored_secrets = custom_store::load_credentials(&live.join(CREDENTIAL_NAME));
    assert_eq!(
        restored_secrets
            .secrets
            .get("custom:local")
            .map(String::as_str),
        Some("sk-machine-local")
    );
    assert!(
        !restored_secrets.secrets.contains_key("custom:a3f9c1"),
        "恢复不得把备份里的密钥写进本机"
    );
}
