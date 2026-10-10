use super::today;
use crate::official_quota::custom::{self, volcengine_ark, CustomQuotaPreset};

#[test]
fn volcengine_sign_matches_volc_sdk_golang_vectors() {
    // magpie TestVolcSignMatchesSDK：volc-sdk-golang (06f9ef66) GetSignRequest
    // 对 POST open.volcengineapi.com/、body {}、服务 ark、2026-10-09T08:30:15Z 算出的签名。
    // 算法与官方文档 https://www.volcengine.com/docs/6369/67269 同一套 HMAC-SHA256。
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-09T08:30:15Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let cases = [
        (
            "cn-beijing",
            "GetCodingPlanUsage",
            "deb80c0cd8bd77768ca40f6d47c7204621c0994428f6ab77e23b76e15dc4c047",
        ),
        (
            "cn-shanghai",
            "GetAFPUsage",
            "8a241be0996c93de4dfcd9e8b1db37494bf63170804fcaebe8380ff7dca2bb23",
        ),
    ];
    for (region, action, signature) in cases {
        let (auth, x_date, sha) = volcengine_ark::sign(
            "AKLTexample",
            "c2VjcmV0LWV4YW1wbGU=",
            region,
            action,
            b"{}",
            now,
        );
        assert_eq!(
            auth,
            format!(
                "HMAC-SHA256 Credential=AKLTexample/20261009/{region}/ark/request, SignedHeaders=content-type;host;x-content-sha256;x-date, Signature={signature}"
            )
        );
        assert_eq!(x_date, "20261009T083015Z");
        assert_eq!(
            sha,
            "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
    }
}

#[test]
fn volcengine_urls_follow_coding_or_agent_path() {
    let coding = custom::request_urls(
        CustomQuotaPreset::VolcengineArk,
        "https://ark.cn-beijing.volces.com/api/coding/v3",
        today(),
    )
    .unwrap();
    assert_eq!(
        coding[0].url,
        "https://open.volcengineapi.com/?Action=GetCodingPlanUsage&Version=2024-01-01"
    );
    assert!(coding[0].required);

    let agent = custom::request_urls(
        CustomQuotaPreset::VolcengineArk,
        "https://ark.cn-shanghai.volces.com/api/plan",
        today(),
    )
    .unwrap();
    assert_eq!(
        agent[0].url,
        "https://open.volcengineapi.com/?Action=GetAFPUsage&Version=2024-01-01"
    );

    let payg = custom::request_urls(
        CustomQuotaPreset::VolcengineArk,
        "https://ark.cn-beijing.volces.com/api/v3",
        today(),
    )
    .unwrap_err();
    assert!(payg.contains("按量"), "{payg}");

    let foreign = custom::request_urls(
        CustomQuotaPreset::VolcengineArk,
        "https://relay.example.com/api/coding",
        today(),
    )
    .unwrap_err();
    assert!(foreign.contains("火山方舟"), "{foreign}");
}

#[test]
fn volcengine_never_accepts_remote_http() {
    let error = custom::request_urls(
        CustomQuotaPreset::VolcengineArk,
        "http://ark.cn-beijing.volces.com/api/coding",
        today(),
    )
    .unwrap_err();
    assert!(error.contains("https://"), "{error}");
}

#[test]
fn volcengine_reads_coding_plan_windows() {
    let body = r#"{
        "ResponseMetadata":{"RequestId":"r1"},
        "Result":{"Status":"ok","QuotaUsage":[
          {"Level":"session","Percent":10.5,"ResetTimestamp":1758100000},
          {"Level":"weekly","Percent":20.25,"ResetTimestamp":1758200000},
          {"Level":"monthly","Percent":"30.75","ResetTimestamp":1758300000},
          {"Level":"daily","Percent":1}
        ]}
    }"#;
    let windows = custom::parse_quota(CustomQuotaPreset::VolcengineArk, &[body]).unwrap();
    assert_eq!(windows.len(), 3);
    assert_eq!(windows[0].kind, "hours_5");
    assert_eq!(windows[0].label, "5 小时");
    assert_eq!(windows[0].used_percent, Some(10.5));
    assert_eq!(
        windows[0].resets_at.as_deref(),
        Some("2025-09-17T09:06:40+00:00")
    );
    assert_eq!(windows[1].kind, "days_7");
    assert_eq!(windows[2].kind, "days_30");
    assert_eq!(windows[2].used_percent, Some(30.75));
}

#[test]
fn volcengine_reads_afp_used_of_quota_and_percent() {
    let quota_form = r#"{
        "Result":{
          "AFPFiveHour":{"Quota":100,"Used":42,"ResetTime":1790000000000},
          "AFPWeekly":{"Quota":0,"Used":5},
          "AFPMonthly":{"Quota":200,"Used":50}
        }
    }"#;
    let windows = custom::parse_quota(CustomQuotaPreset::VolcengineArk, &[quota_form]).unwrap();
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].kind, "hours_5");
    assert!((windows[0].used_percent.unwrap() - 42.0).abs() < 1e-9);
    assert_eq!(windows[0].used_amount, Some(42.0));
    assert_eq!(windows[0].limit_amount, Some(100.0));
    assert_eq!(windows[1].kind, "days_30");
    assert!((windows[1].used_percent.unwrap() - 25.0).abs() < 1e-9);

    let percent_form = r#"{
        "Result":{
          "Tier":"Pro",
          "AFPFiveHour":{"Percent":12.5,"ResetTimestamp":1790000000},
          "AFPWeekly":{"Used":"30","Total":"120","ResetTime":"2026-10-12T00:00:00Z"}
        }
    }"#;
    let windows = custom::parse_quota(CustomQuotaPreset::VolcengineArk, &[percent_form]).unwrap();
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].used_percent, Some(12.5));
    assert_eq!(windows[1].kind, "days_7");
    assert!((windows[1].used_percent.unwrap() - 25.0).abs() < 1e-9);
}

#[test]
fn volcengine_rejects_empty_or_refused_bodies() {
    assert!(custom::parse_quota(CustomQuotaPreset::VolcengineArk, &[""]).is_err());
    let refused = custom::parse_quota(
        CustomQuotaPreset::VolcengineArk,
        &[r#"{"ResponseMetadata":{"Error":{"Code":"SignatureDoesNotMatch","Message":"no"}}}"#],
    )
    .unwrap_err();
    assert!(refused.contains("AccessKey"), "{refused}");
}

#[test]
fn volcengine_needs_both_secrets_and_uses_hmac_not_bearer() {
    assert!(CustomQuotaPreset::VolcengineArk.implemented());
    assert!(CustomQuotaPreset::VolcengineArk.needs_access_key_id());
    assert!(!CustomQuotaPreset::KimiCode.needs_access_key_id());
    let missing_ak = custom::fetch_quota(
        CustomQuotaPreset::VolcengineArk,
        "https://ark.cn-beijing.volces.com/api/coding",
        Some("sk-secret"),
        None,
    )
    .unwrap_err();
    assert!(missing_ak.contains("AccessKey ID"), "{missing_ak}");
    assert!(custom::is_precheck_error(&missing_ak));
}

#[test]
fn old_single_secret_credential_files_still_load() {
    use crate::official_quota::custom::store::{self, CustomQuotaConfig, CustomQuotaProvider};

    let dir = tempfile::tempdir().unwrap();
    let paths = store::CustomQuotaPaths::in_dir(dir.path());
    store::save_config(
        &paths.config,
        &CustomQuotaConfig {
            providers: vec![CustomQuotaProvider {
                id: "custom:a3f9c1".to_string(),
                name: "旧中转".to_string(),
                preset: CustomQuotaPreset::OpenAiCompatible,
                base_url: "https://relay.example.com".to_string(),
                enabled: true,
            }],
        },
    )
    .unwrap();
    std::fs::write(
        &paths.credentials,
        r#"{"secrets":{"custom:a3f9c1":"sk-old-secret"}}"#,
    )
    .unwrap();
    let loaded = store::load_providers(&paths);
    assert_eq!(loaded[0].secret.as_deref(), Some("sk-old-secret"));
    assert_eq!(loaded[0].access_key_id, None);

    let saved = store::load_credentials(&paths.credentials);
    store::save_credentials(&paths.credentials, &saved).unwrap();
    let rewritten = std::fs::read_to_string(&paths.credentials).unwrap();
    assert!(rewritten.contains("sk-old-secret"));
    assert!(
        !rewritten.contains("access_key"),
        "单密钥文件重写后不得冒出 access_key_ids：{rewritten}"
    );
}
