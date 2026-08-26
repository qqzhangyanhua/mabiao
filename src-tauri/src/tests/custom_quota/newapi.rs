//! NewAPI / OneAPI 是 OpenAI 兼容计费的别名。
//!
//! 请求构造与响应解析两处分派都指向后者，地址与解析一字不改。
//! 外壳变化（已实现标记、显示名、密钥提示）另由前端 vitest 与下面几条钉住。

use super::{provider, today, SUBSCRIPTION, USAGE};
use crate::official_quota::custom::store::{
    CustomQuotaConfig, CustomQuotaCredentials, CustomQuotaProvider,
};
use crate::official_quota::custom::{self, panel, store, CustomQuotaPreset};
use crate::official_quota::{self as quota};
use crate::store as db;

const BASE: &str = "https://relay.example.com";

fn newapi_provider(id: &str, name: &str) -> CustomQuotaProvider {
    let mut config = provider(id, name);
    config.preset = CustomQuotaPreset::NewApi;
    config
}

/// 「别名」的可执行定义：同一份 base URL 必须产出完全相同的请求地址列表。
/// 哪天 NewAPI 档悄悄分出自己的路径，这一条会先红。
#[test]
fn newapi_requests_the_same_urls_as_openai_compatible() {
    let openai = custom::request_urls(CustomQuotaPreset::OpenAiCompatible, BASE, today()).unwrap();
    let newapi = custom::request_urls(CustomQuotaPreset::NewApi, BASE, today()).unwrap();
    assert_eq!(
        openai, newapi,
        "NewAPI / OneAPI 必须与 OpenAI 兼容计费请求同一组地址"
    );
}

/// 同一份响应体必须解析出完全相同的额度窗口。
#[test]
fn newapi_parses_the_same_windows_as_openai_compatible() {
    let openai =
        custom::parse_quota(CustomQuotaPreset::OpenAiCompatible, &[SUBSCRIPTION, USAGE]).unwrap();
    let newapi = custom::parse_quota(CustomQuotaPreset::NewApi, &[SUBSCRIPTION, USAGE]).unwrap();
    assert_eq!(openai, newapi);
}

#[test]
fn newapi_is_implemented_and_does_not_imply_a_user_api() {
    assert!(
        CustomQuotaPreset::NewApi.implemented(),
        "下拉里不该再标「暂未支持」"
    );
    assert_eq!(CustomQuotaPreset::NewApi.display_name(), "NewAPI / OneAPI");
    assert!(
        !CustomQuotaPreset::NewApi
            .display_name()
            .contains("用户接口"),
        "显示名不该再暗示会打站点的用户接口"
    );
}

/// 回显走的就是 `request_urls`。别名落地后，选这档时设置页那行「将请求」
/// 必须和真正取数的地址一致，也必须和旁边那档 OpenAI 兼容计费一致。
#[test]
fn newapi_echo_matches_the_addresses_the_fetch_will_request() {
    let preview = panel::preview_requests(CustomQuotaPreset::NewApi, BASE, today());
    assert_eq!(preview.error, None);
    assert_eq!(
        preview.requests,
        custom::request_urls(CustomQuotaPreset::NewApi, BASE, today()).unwrap()
    );
    assert_eq!(
        preview.requests,
        custom::request_urls(CustomQuotaPreset::OpenAiCompatible, BASE, today()).unwrap()
    );
}

/// 老用户在「暂未支持」期间按旧提示存过系统访问令牌。变体必须还能反序列化，
/// 否则加载失败会被吞成空列表，**所有**自定义提供商一起消失。
#[test]
fn a_legacy_newapi_config_still_loads_alongside_other_providers() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    std::fs::write(
        &paths.config,
        r#"{
            "providers": [
                {
                    "id": "custom:a3f9c1",
                    "name": "公司的中转",
                    "preset": "openai_compatible",
                    "base_url": "https://relay.example.com",
                    "enabled": true
                },
                {
                    "id": "custom:b7e204",
                    "name": "自建 NewAPI",
                    "preset": "newapi",
                    "base_url": "https://newapi.example.com",
                    "enabled": true
                }
            ]
        }"#,
    )
    .unwrap();

    let loaded = store::load_providers(&paths);
    assert_eq!(loaded.len(), 2, "加载失败不该把别的提供商一起吞掉");
    assert_eq!(loaded[0].config.preset, CustomQuotaPreset::OpenAiCompatible);
    assert_eq!(loaded[1].config.preset, CustomQuotaPreset::NewApi);
    assert_eq!(loaded[1].config.name, "自建 NewAPI");
}

/// 不为旧密钥写迁移：现在会真去打网，好让 401 翻成「请在设置页更新密钥」。
/// 再拦在「暂未支持」里，用户看不到该换哪把钥匙。
#[test]
fn a_legacy_newapi_secret_is_tried_instead_of_blocked_as_unsupported() {
    let resolved = custom::ResolvedProvider {
        config: newapi_provider("custom:b7e204", "自建 NewAPI"),
        secret: Some("system-access-token".to_string()),
    };
    assert_eq!(
        custom::precheck(&resolved),
        None,
        "有密钥的 NewAPI 档现在不该在打网之前被拦下"
    );
    assert!(!custom::is_precheck_error(
        "密钥无效或已失效，请在设置页更新密钥"
    ));
}

/// 保存这档之后，下拉不再标「暂未支持」，首页官方额度能画出解析到的窗口。
#[test]
fn saving_a_newapi_provider_joins_official_quota_with_parsed_windows() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    let saved = panel::save(
        &paths,
        panel::SaveCustomQuotaProvider {
            id: None,
            name: "自建 NewAPI".to_string(),
            preset: CustomQuotaPreset::NewApi,
            base_url: BASE.to_string(),
            enabled: None,
            secret: Some("sk-relay-123456".to_string()),
        },
    )
    .unwrap();

    let listed = saved
        .panel
        .presets
        .iter()
        .find(|preset| preset.value == "newapi")
        .expect("下拉里应有 NewAPI / OneAPI");
    assert!(listed.supported, "下拉里不该再标「暂未支持」");
    assert_eq!(listed.label, "NewAPI / OneAPI");
    assert_eq!(saved.panel.providers[0].preset, CustomQuotaPreset::NewApi);

    let id = saved.saved_id.clone();
    let windows = custom::parse_quota(CustomQuotaPreset::NewApi, &[SUBSCRIPTION, USAGE]).unwrap();
    let now = chrono::Utc::now();
    let conn = db::open_memory().unwrap();
    quota::apply_fetch_results(
        &conn,
        [(id.clone(), Ok((windows, now.to_rfc3339()).into()))],
    )
    .unwrap();

    let dto = quota::load_dto(
        &conn,
        &crate::domain::OfficialQuotaConfig::default(),
        &[custom::ResolvedProvider {
            config: newapi_provider(&id, "自建 NewAPI"),
            secret: Some("sk-relay-123456".to_string()),
        }],
        now,
    );
    let row = dto
        .rows
        .iter()
        .find(|row| row.provider == id)
        .expect("保存后首页官方额度应出现对应行");
    assert_eq!(row.application, "自建 NewAPI");
    assert_eq!(row.windows[0].used_amount, Some(19.0));
    assert_eq!(row.windows[0].limit_amount, Some(50.0));
    assert_eq!(row.windows[0].used_percent, Some(38.0));
}

/// 凭证文件按标识索引：老用户存过的系统访问令牌必须还能读出来，
/// 不能因为这次改了预设语义就把密钥弄丢。
#[test]
fn a_legacy_newapi_row_keeps_its_stored_secret() {
    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    store::save_config(
        &paths.config,
        &CustomQuotaConfig {
            providers: vec![newapi_provider("custom:b7e204", "自建 NewAPI")],
        },
    )
    .unwrap();
    let mut credentials = CustomQuotaCredentials::default();
    credentials.secrets.insert(
        "custom:b7e204".to_string(),
        "system-access-token".to_string(),
    );
    store::save_credentials(&paths.credentials, &credentials).unwrap();

    let loaded = store::load_providers(&paths);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].config.preset, CustomQuotaPreset::NewApi);
    assert_eq!(loaded[0].secret.as_deref(), Some("system-access-token"));
}
