use crate::domain::{OfficialQuotaConfig, OfficialQuotaConfigPatch};
use crate::official_quota;

#[test]
fn patch_config_preserves_untouched_fields_and_sanitizes_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(official_quota::CONFIG_NAME);

    official_quota::save_config(
        &path,
        &OfficialQuotaConfig {
            alerts_enabled: true,
            hidden_providers: vec!["grok".into()],
            alert_thresholds: vec![60, 90],
            reset_reminder_hours: 24,
            reset_reminder_max_used_percent: 40,
        },
    )
    .unwrap();

    // Patching only hidden_providers preserves custom alert thresholds and reminder settings,
    // strips custom:* IDs and blank entries, and deduplicates preserving order.
    let updated = official_quota::patch_config(
        &path,
        &OfficialQuotaConfigPatch {
            hidden_providers: Some(vec![
                " claude ".into(),
                "custom:abc123".into(),
                "".into(),
                "claude".into(),
                "codex".into(),
            ]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(updated.hidden_providers, vec!["claude", "codex"]);
    assert!(updated.alerts_enabled);
    assert_eq!(updated.alert_thresholds, vec![60, 90]);
    assert_eq!(updated.reset_reminder_hours, 24);
    assert_eq!(updated.reset_reminder_max_used_percent, 40);

    // Patching only alerts_enabled preserves hidden_providers and custom thresholds.
    let toggled = official_quota::patch_config(
        &path,
        &OfficialQuotaConfigPatch {
            alerts_enabled: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!toggled.alerts_enabled);
    assert_eq!(toggled.hidden_providers, vec!["claude", "codex"]);
    assert_eq!(toggled.alert_thresholds, vec![60, 90]);

    // Reloading from disk matches the returned value and enforces bounds on thresholds/reminders.
    let clamped = official_quota::patch_config(
        &path,
        &OfficialQuotaConfigPatch {
            alert_thresholds: Some(vec![0, 95, 50, 50, 120]),
            reset_reminder_hours: Some(500),
            reset_reminder_max_used_percent: Some(150),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(clamped.alert_thresholds, vec![50, 95]);
    assert_eq!(clamped.reset_reminder_hours, 168);
    assert_eq!(clamped.reset_reminder_max_used_percent, 100);
    assert_eq!(official_quota::load_config(&path), clamped);
}

#[test]
fn concurrent_patch_config_calls_do_not_clobber_disjoint_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(official_quota::CONFIG_NAME);
    official_quota::save_config(&path, &OfficialQuotaConfig::default()).unwrap();

    std::thread::scope(|scope| {
        let p1 = &path;
        let p2 = &path;
        let h1 = scope.spawn(move || {
            for _ in 0..20 {
                official_quota::patch_config(
                    p1,
                    &OfficialQuotaConfigPatch {
                        hidden_providers: Some(vec!["claude".into(), "devin".into()]),
                        ..Default::default()
                    },
                )
                .unwrap();
            }
        });
        let h2 = scope.spawn(move || {
            for _ in 0..20 {
                official_quota::patch_config(
                    p2,
                    &OfficialQuotaConfigPatch {
                        alerts_enabled: Some(false),
                        alert_thresholds: Some(vec![70, 95]),
                        ..Default::default()
                    },
                )
                .unwrap();
            }
        });
        h1.join().unwrap();
        h2.join().unwrap();
    });

    let final_config = official_quota::load_config(&path);
    assert_eq!(final_config.hidden_providers, vec!["claude", "devin"]);
    assert!(!final_config.alerts_enabled);
    assert_eq!(final_config.alert_thresholds, vec![70, 95]);
}

mod quota_env_tests {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use base64::Engine;
    use chrono::{DateTime, Utc};

    use crate::domain::OfficialQuotaProvider;
    use crate::official_quota::{
        self, accounts, detect, hook, HttpRequest, HttpResponse, QuotaEnv, QuotaPaths, QuotaTarget,
        QuotaTransport,
    };
    use crate::scan_paths::{ScanEnv, ScanPathConfig};

    fn fixed_time() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-10T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn write_file(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    fn seed_vscdb(dir: &Path, items: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        let conn = rusqlite::Connection::open(dir.join("state.vscdb")).unwrap();
        conn.execute(
            "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value TEXT)",
            [],
        )
        .unwrap();
        for (key, value) in items {
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                [*key, *value],
            )
            .unwrap();
        }
    }

    fn fake_jwt(payload: &str) -> String {
        let enc = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.as_bytes());
        format!("{}.{}.sig", enc(r#"{"alg":"none"}"#), enc(payload))
    }

    struct FakeTransport {
        requests: Mutex<Vec<HttpRequest>>,
        codex_app_server_response: Option<String>,
        droid_keychain: Option<String>,
        antigravity_keychain: Option<String>,
    }

    impl FakeTransport {
        fn new() -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                codex_app_server_response: None,
                droid_keychain: None,
                antigravity_keychain: None,
            }
        }

        fn header_of(&self, url_contains: &str, name: &str) -> Option<String> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .find(|req| req.url.contains(url_contains))
                .and_then(|req| {
                    req.headers
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(name))
                        .map(|(_, v)| v.clone())
                })
        }
    }

    impl QuotaTransport for FakeTransport {
        fn http(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
            self.requests.lock().unwrap().push(request.clone());
            let body = if request.url.contains("/api/oauth/usage") {
                r#"{"five_hour":{"utilization":25.0,"resets_at":"2026-09-10T17:00:00Z"}}"#
            } else if request.url.contains("/backend-api/wham/usage") {
                r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":30.0,"limit_window_seconds":18000,"reset_at":1789059600}}}"#
            } else if request.url.contains("/api/usage-summary") {
                r#"{"membershipType":"pro","billingCycleEnd":"2026-10-01T00:00:00Z","individualUsage":{"plan":{"enabled":true,"totalPercentUsed":42.0}}}"#
            } else if request.url.contains("/v1/billing?format=credits") {
                r#"{"config":{"creditUsagePercent":55.0,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","end":"2026-09-17T00:00:00Z"}}}"#
            } else if request.url.contains("/v1/billing") {
                r#"{"config":{}}"#
            } else if request.url.contains("/v1/settings") {
                r#"{"config":{"subscription_tier_display":"SuperGrok"}}"#
            } else if request.url.contains("/api/billing/limits") {
                r#"{"limits":{"standard":{"fiveHour":{"usedPercent":18.0,"windowEnd":"2026-09-10T17:00:00Z"}}}}"#
            } else if request
                .url
                .contains("/api/organization/subscription/schedule")
            {
                r#"{"schedule":[{"start_date":"2026-01-01T00:00:00Z","plan":{"name":"Factory Pro Plan"}}]}"#
            } else if request.url.contains("retrieveUserQuotaSummary") {
                r#"{"groups":[{"displayName":"Gemini","buckets":[{"bucketId":"gemini-weekly","window":"weekly","remainingFraction":0.65,"resetTime":"2026-09-17T00:00:00Z"}]}]}"#
            } else if request.url.contains("loadCodeAssist") {
                r#"{"paidTier":{"id":"google_ai_pro"}}"#
            } else if request.url.contains("/zen/go/v1/usage") {
                r#"{"usage":{"rolling":{"percent":12.5,"resetsAt":"2026-09-10T17:00:00Z"}}}"#
            } else if request.url.contains("/copilot_internal/user") {
                r#"{"copilot_plan":"pro","quota_reset_date":"2026-10-01","quota_snapshots":{"premium_interactions":{"percent_remaining":70.0}}}"#
            } else if request.url.contains("GetUserStatus") {
                r#"{"userStatus":{"planStatus":{"planInfo":{"planName":"Pro"},"weeklyQuotaRemainingPercent":80,"weeklyQuotaResetAtUnix":1789603200}}}"#
            } else {
                return Err(format!("unexpected url: {}", request.url));
            };
            Ok(HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: body.as_bytes().to_vec(),
            })
        }

        fn codex_app_server_rate_limits(&self) -> Result<String, String> {
            self.codex_app_server_response
                .clone()
                .ok_or_else(|| "未找到 Codex CLI，或无法启动 app-server".to_string())
        }

        fn droid_keychain_key(&self) -> Option<String> {
            self.droid_keychain.clone()
        }

        fn antigravity_keychain_blob(&self) -> Option<String> {
            self.antigravity_keychain.clone()
        }
    }

    #[test]
    fn quota_env_detects_and_fetches_all_nine_builtin_providers_in_isolated_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();

        let empty_env = QuotaEnv::isolated(
            ScanEnv::new(&home),
            QuotaPaths::in_dir(&app_data),
            fixed_time(),
        );
        for provider in OfficialQuotaProvider::ALL {
            assert!(
                !detect::has_local_credentials_in(&empty_env, provider),
                "empty tempdir must not detect host credentials for {}",
                provider.as_str()
            );
        }

        // Custom ScanEnv overrides with two roots for Claude/Codex/Grok/OpenCode:
        // first root has no credentials, second root has credentials -> second root wins.
        let claude_empty = dir.path().join("claude-empty");
        let claude_cred = dir.path().join("claude-cred");
        let codex_empty = dir.path().join("codex-empty");
        let codex_cred = dir.path().join("codex-cred");
        let grok_cred = dir.path().join("grok-cred");
        let opencode_cred = dir.path().join("opencode-cred");

        write_file(
            &claude_cred.join(".credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"claude-tok","scopes":["user:profile"],"expiresAt":0}}"#,
        );
        write_file(
            &codex_cred.join("auth.json"),
            r#"{"tokens":{"access_token":"codex-tok","account_id":"acct_codex_1"}}"#,
        );
        write_file(
            &grok_cred.join("auth.json"),
            r#"{"https://auth.x.ai::openid":{"key":"grok-tok","auth_mode":"oidc","user_id":"uid-grok"}}"#,
        );
        write_file(&grok_cred.join(".metadata_version"), "9.9.9\n");
        write_file(
            &opencode_cred.join("auth.json"),
            r#"{"opencode-go":{"key":"oc-key"}}"#,
        );

        let ui_config = ScanPathConfig {
            overrides: BTreeMap::from([
                (
                    "claude".to_string(),
                    vec![
                        claude_empty.to_string_lossy().into_owned(),
                        claude_cred.to_string_lossy().into_owned(),
                    ],
                ),
                (
                    "codex".to_string(),
                    vec![
                        codex_empty.to_string_lossy().into_owned(),
                        codex_cred.to_string_lossy().into_owned(),
                    ],
                ),
                (
                    "grok".to_string(),
                    vec![grok_cred.to_string_lossy().into_owned()],
                ),
                (
                    "opencode".to_string(),
                    vec![opencode_cred.to_string_lossy().into_owned()],
                ),
            ]),
        };

        let config_dir = dir.path().join("platform-config");
        let cursor_token = fake_jwt(r#"{"sub":"google-oauth|user_01CURSOR","exp":4102444800}"#);
        seed_vscdb(
            &config_dir.join("Cursor/User/globalStorage"),
            &[("cursorAuth/accessToken", &cursor_token)],
        );
        seed_vscdb(
            &config_dir.join("Antigravity/User/globalStorage"),
            &[(
                "antigravityAuthStatus",
                r#"{"apiKey":"ya29.agy-tok","email":"agy@example.com"}"#,
            )],
        );
        seed_vscdb(
            &config_dir.join("Windsurf/User/globalStorage"),
            &[("windsurfAuthStatus", r#"{"apiKey":"devin-key"}"#)],
        );
        write_file(
            &home.join(".factory/auth.json"),
            &format!(
                r#"{{"access_token":"{}"}}"#,
                fake_jwt(r#"{"external_org_id":"org_123"}"#)
            ),
        );
        write_file(
            &home.join(".config/github-copilot/apps.json"),
            r#"{"github.com:Iv1":{"oauth_token":"ghu_copilot_tok","user":"octocat"}}"#,
        );

        let transport = Arc::new(FakeTransport::new());
        let env = QuotaEnv::isolated(
            ScanEnv::new(&home).with_ui_config(ui_config),
            QuotaPaths::in_dir(&app_data),
            fixed_time(),
        )
        .with_config_dir(config_dir)
        .with_transport(transport.clone());

        // Hook inspection honors primary ScanEnv root for Claude.
        let hook_preview = hook::preview_in(&env, "\"mabiao\" statusline");
        assert_eq!(
            Path::new(&hook_preview.settings_path),
            claude_empty.join("settings.json")
        );

        for provider in OfficialQuotaProvider::ALL {
            assert!(
                detect::has_local_credentials_in(&env, provider),
                "expected credentials detected for {}",
                provider.as_str()
            );
            let snapshot = official_quota::fetch_provider_in(&env, provider)
                .unwrap_or_else(|e| panic!("{} failed: {e}", provider.as_str()));
            assert!(
                !snapshot.windows.is_empty(),
                "{} returned no windows",
                provider.as_str()
            );
            assert_eq!(snapshot.captured_at, fixed_time().to_rfc3339());
        }

        // Verify ScanEnv credentials & Grok metadata version reached HTTP transport.
        assert_eq!(
            transport
                .header_of("/api/oauth/usage", "Authorization")
                .as_deref(),
            Some("Bearer claude-tok")
        );
        assert_eq!(
            transport
                .header_of("/backend-api/wham/usage", "Authorization")
                .as_deref(),
            Some("Bearer codex-tok")
        );
        assert_eq!(
            transport
                .header_of("/v1/billing?format=credits", "x-grok-client-version")
                .as_deref(),
            Some("9.9.9")
        );
    }

    #[test]
    fn quota_env_codex_falls_back_to_injected_app_server_transport() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        // API-key-only auth.json causes wham/usage to decline -> falls back to app-server.
        write_file(
            &home.join(".codex/auth.json"),
            r#"{"OPENAI_API_KEY":"sk-x"}"#,
        );

        let mut transport = FakeTransport::new();
        transport.codex_app_server_response = Some(
            r#"{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":44.0,"windowDurationMins":300,"resetsAt":1789059600}}}}"#
                .to_string(),
        );
        let env = QuotaEnv::isolated(
            ScanEnv::new(&home),
            QuotaPaths::in_dir(&app_data),
            fixed_time(),
        )
        .with_transport(Arc::new(transport));

        let snap = official_quota::fetch_provider_in(&env, OfficialQuotaProvider::Codex).unwrap();
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].used_percent, Some(44.0));
        assert_eq!(snap.captured_at, fixed_time().to_rfc3339());
    }

    #[test]
    fn quota_env_profile_dir_account_resolves_strictly_from_assigned_directory() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        let profile_dir = dir.path().join("work-codex");

        write_file(
            &home.join(".codex/auth.json"),
            r#"{"tokens":{"access_token":"default-home-tok","account_id":"acct_default"}}"#,
        );
        write_file(
            &profile_dir.join("auth.json"),
            r#"{"tokens":{"access_token":"profile-only-tok","account_id":"acct_work"}}"#,
        );

        let paths = QuotaPaths::in_dir(&app_data);
        let saved = accounts::save_profile_dir(
            &paths.accounts,
            accounts::SaveProfileDirAccount {
                id: None,
                provider: "codex".into(),
                name: "Work".into(),
                profile_dir: profile_dir.to_string_lossy().into_owned(),
            },
            &paths.backoff,
        )
        .unwrap();

        let transport = Arc::new(FakeTransport::new());
        let env = QuotaEnv::isolated(ScanEnv::new(&home), paths.clone(), fixed_time())
            .with_transport(transport.clone());

        let resolved = accounts::load_resolved(&paths.accounts);
        let logins = accounts::probe_default_logins_in(&env, &resolved);
        let target = official_quota::resolve_target_with_accounts_in(
            &env,
            &saved.saved_id,
            &[],
            &resolved,
            &logins,
        )
        .unwrap();
        let snap = official_quota::fetch_target_in(&env, &target).unwrap();
        assert_eq!(snap.windows[0].used_percent, Some(30.0));
        assert_eq!(snap.captured_at, fixed_time().to_rfc3339());
        assert_eq!(
            transport
                .header_of("/backend-api/wham/usage", "Authorization")
                .as_deref(),
            Some("Bearer profile-only-tok")
        );
        assert_eq!(
            transport
                .header_of("/backend-api/wham/usage", "ChatGPT-Account-Id")
                .as_deref(),
            Some("acct_work")
        );
    }

    #[test]
    fn snapshot_over_readonly_sqlite_overlays_newer_claude_capture_without_writing_or_notifying() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();

        let db_path = app_data.join("usage.sqlite");
        let db_str = db_path.to_string_lossy().to_string();
        {
            let conn = crate::store::open_db(&db_str).unwrap();
            official_quota::apply_success(
                &conn,
                OfficialQuotaProvider::Claude,
                vec![crate::domain::OfficialQuotaWindow {
                    kind: "session_5h".into(),
                    label: "5 小时".into(),
                    used_percent: Some(50.0),
                    resets_at: Some("2099-01-01T05:00:00Z".into()),
                    ..Default::default()
                }],
                "2020-01-01T00:00:00Z",
            )
            .unwrap();
        }

        let paths = QuotaPaths::in_dir(&app_data);
        write_file(
            &paths.claude_capture,
            r#"{"rate_limits":{"five_hour":{"used_percentage":92.0,"resets_at":"2099-01-01T05:00:00Z"}}}"#,
        );
        let capture_ts = official_quota::claude::file_captured_at(&paths.claude_capture).unwrap();
        let now = DateTime::parse_from_rfc3339(&capture_ts)
            .unwrap()
            .with_timezone(&Utc)
            + chrono::Duration::seconds(30);

        let env = QuotaEnv::isolated(ScanEnv::new(&home), paths.clone(), now);
        let ro_conn = crate::store::open_readonly(&db_str).unwrap();
        let dto = official_quota::snapshot(&ro_conn, &env);

        let claude = dto.rows.iter().find(|r| r.provider == "claude").unwrap();
        assert_eq!(claude.windows[0].used_percent, Some(92.0));
        assert_eq!(
            claude.freshness,
            crate::domain::OfficialQuotaFreshness::Official
        );
        assert_eq!(claude.captured_at.as_deref(), Some(capture_ts.as_str()));

        // Read purity: notify state file not touched, SQLite row still holds old cached value.
        assert!(!paths.notify_state.exists());
        let stored = crate::store::load_official_quota_row(&ro_conn, "claude")
            .unwrap()
            .unwrap();
        assert_eq!(stored.windows[0].used_percent, Some(50.0));
        assert_eq!(stored.captured_at, "2020-01-01T00:00:00Z");

        // Reverse check: newer cached OAuth snapshot is NOT downgraded by an older capture file.
        {
            let conn = crate::store::open_db(&db_str).unwrap();
            official_quota::apply_success(
                &conn,
                OfficialQuotaProvider::Claude,
                vec![crate::domain::OfficialQuotaWindow {
                    kind: "session_5h".into(),
                    label: "5 小时".into(),
                    used_percent: Some(15.0),
                    resets_at: Some("2099-01-01T05:00:00Z".into()),
                    ..Default::default()
                }],
                "2099-01-01T00:00:00Z",
            )
            .unwrap();
        }
        let future_now = DateTime::parse_from_rfc3339("2099-01-01T00:02:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let future_env = QuotaEnv::isolated(ScanEnv::new(&home), paths, future_now);
        let dto2 = official_quota::snapshot(&ro_conn, &future_env);
        let claude2 = dto2.rows.iter().find(|r| r.provider == "claude").unwrap();
        assert_eq!(claude2.windows[0].used_percent, Some(15.0));
        assert_eq!(claude2.captured_at.as_deref(), Some("2099-01-01T00:00:00Z"));
    }

    #[test]
    fn refresh_and_notify_persists_notifies_once_and_emits_updated_events() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        std::fs::create_dir_all(&app_data).unwrap();

        write_file(
            &home.join(".claude/.credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"claude-tok","scopes":["user:profile"],"expiresAt":0}}"#,
        );
        write_file(
            &home.join(".codex/auth.json"),
            r#"{"tokens":{"access_token":"codex-tok","account_id":"acct_1"}}"#,
        );

        struct AlertingTransport;
        impl QuotaTransport for AlertingTransport {
            fn http(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
                let body = if request.url.contains("/api/oauth/usage") {
                    r#"{"five_hour":{"utilization":88.0,"resets_at":"2026-09-10T17:00:00Z"}}"#
                } else if request.url.contains("/backend-api/wham/usage") {
                    r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":22.0,"limit_window_seconds":18000,"reset_at":1789059600}}}"#
                } else {
                    return Err(format!("unexpected: {}", request.url));
                };
                Ok(HttpResponse {
                    status: 200,
                    headers: Vec::new(),
                    body: body.as_bytes().to_vec(),
                })
            }
        }

        let paths = QuotaPaths::in_dir(&app_data);
        let env = QuotaEnv::isolated(ScanEnv::new(&home), paths.clone(), fixed_time())
            .with_transport(Arc::new(AlertingTransport));
        let conns = crate::test_support::TestConnection::new(crate::store::open_memory().unwrap());

        let alerts = Mutex::new(Vec::<official_quota::notify::QuotaAlert>::new());
        let emitted = Mutex::new(Vec::<crate::domain::OfficialQuotaDto>::new());
        let on_alert = |alert: &official_quota::notify::QuotaAlert, _title: &str, _body: &str| {
            alerts.lock().unwrap().push(alert.clone());
        };
        let on_updated = |dto: &crate::domain::OfficialQuotaDto| {
            emitted.lock().unwrap().push(dto.clone());
        };

        let dto = official_quota::refresh_and_notify(
            &conns,
            &env,
            official_quota::RefreshOptions {
                notify: Some(&on_alert),
                on_updated: Some(&on_updated),
                stream_progress: false,
            },
        )
        .unwrap();

        assert_eq!(dto.rows.len(), 2);
        assert_eq!(alerts.lock().unwrap().len(), 1);
        assert_eq!(alerts.lock().unwrap()[0].threshold, 80);
        assert_eq!(emitted.lock().unwrap().len(), 1);
        assert!(paths.notify_state.exists());

        // Persisted in SQLite.
        let stored = crate::store::load_official_quota_row(&conns.get(), "claude")
            .unwrap()
            .unwrap();
        assert_eq!(stored.windows[0].used_percent, Some(88.0));

        // Second refresh in same reset window emits updated DTO again but does NOT duplicate OS alert.
        official_quota::refresh_and_notify(
            &conns,
            &env,
            official_quota::RefreshOptions {
                notify: Some(&on_alert),
                on_updated: Some(&on_updated),
                stream_progress: true,
            },
        )
        .unwrap();
        assert_eq!(alerts.lock().unwrap().len(), 1);
        assert!(emitted.lock().unwrap().len() >= 2);
    }

    #[test]
    fn cli_without_refresh_is_readonly_and_with_refresh_persists_capture_without_notifying() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        std::fs::create_dir_all(&app_data).unwrap();
        write_file(
            &home.join(".codex/auth.json"),
            r#"{"tokens":{"access_token":"codex-tok"}}"#,
        );

        let db_path = app_data.join("usage.sqlite");
        let db_str = db_path.to_string_lossy().to_string();
        drop(crate::store::open_db(&db_str).unwrap());

        let paths = QuotaPaths::in_dir(&app_data);
        write_file(
            &paths.claude_capture,
            r#"{"rate_limits":{"five_hour":{"used_percentage":95.0,"resets_at":"2099-01-01T05:00:00Z"}}}"#,
        );
        let capture_ts = official_quota::claude::file_captured_at(&paths.claude_capture).unwrap();
        let now = DateTime::parse_from_rfc3339(&capture_ts)
            .unwrap()
            .with_timezone(&Utc)
            + chrono::Duration::seconds(10);

        let transport = Arc::new(FakeTransport::new());
        let env =
            QuotaEnv::isolated(ScanEnv::new(&home), paths.clone(), now).with_transport(transport);

        // CLI without --refresh: pure read over open_readonly, overlays capture in memory.
        let readonly_dto = official_quota::run_cli_in(&db_path, &env, false).unwrap();
        let claude_ro = readonly_dto
            .rows
            .iter()
            .find(|r| r.provider == "claude")
            .unwrap();
        assert_eq!(claude_ro.windows[0].used_percent, Some(95.0));
        assert!(!paths.notify_state.exists());
        {
            let check = crate::store::open_readonly(&db_str).unwrap();
            assert!(crate::store::load_official_quota_row(&check, "claude")
                .unwrap()
                .is_none());
        }

        // CLI with --refresh: fetches Codex, persists both Codex and Claude capture, suppresses alerts.
        let refreshed_dto = official_quota::run_cli_in(&db_path, &env, true).unwrap();
        assert_eq!(
            refreshed_dto
                .rows
                .iter()
                .find(|r| r.provider == "claude")
                .unwrap()
                .windows[0]
                .used_percent,
            Some(95.0)
        );
        assert_eq!(
            refreshed_dto
                .rows
                .iter()
                .find(|r| r.provider == "codex")
                .unwrap()
                .windows[0]
                .used_percent,
            Some(30.0)
        );
        assert!(!paths.notify_state.exists());
        let check = crate::store::open_readonly(&db_str).unwrap();
        assert_eq!(
            crate::store::load_official_quota_row(&check, "claude")
                .unwrap()
                .unwrap()
                .windows[0]
                .used_percent,
            Some(95.0)
        );
        assert_eq!(
            crate::store::load_official_quota_row(&check, "codex")
                .unwrap()
                .unwrap()
                .windows[0]
                .used_percent,
            Some(30.0)
        );
    }

    #[test]
    fn snapshot_visible_respects_official_quota_config_hidden_providers() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();

        let paths = QuotaPaths::in_dir(&app_data);
        official_quota::patch_config(
            &paths.config,
            &crate::domain::OfficialQuotaConfigPatch {
                hidden_providers: Some(vec!["claude".into()]),
                ..Default::default()
            },
        )
        .unwrap();

        let conn = crate::store::open_memory().unwrap();
        official_quota::apply_success(
            &conn,
            OfficialQuotaProvider::Claude,
            vec![crate::domain::OfficialQuotaWindow {
                kind: "session_5h".into(),
                label: "5 小时".into(),
                used_percent: Some(40.0),
                ..Default::default()
            }],
            "2026-09-10T11:58:00Z",
        )
        .unwrap();
        official_quota::apply_success(
            &conn,
            OfficialQuotaProvider::Codex,
            vec![crate::domain::OfficialQuotaWindow {
                kind: "session_5h".into(),
                label: "5 小时".into(),
                used_percent: Some(20.0),
                ..Default::default()
            }],
            "2026-09-10T11:58:00Z",
        )
        .unwrap();

        let env = QuotaEnv::isolated(ScanEnv::new(&home), paths, fixed_time());
        let full = official_quota::snapshot(&conn, &env);
        assert_eq!(full.hidden_providers, vec!["claude"]);
        assert!(full.rows.iter().any(|r| r.provider == "claude"));
        assert!(full.rows.iter().any(|r| r.provider == "codex"));

        let visible = official_quota::snapshot_visible(&conn, &env);
        assert!(!visible.rows.iter().any(|r| r.provider == "claude"));
        assert!(visible.rows.iter().any(|r| r.provider == "codex"));
    }

    #[test]
    fn throttled_cooldown_overlays_without_writing_and_forced_bypasses_cooldown() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        std::fs::create_dir_all(&app_data).unwrap();
        write_file(
            &home.join(".codex/auth.json"),
            r#"{"tokens":{"access_token":"codex-tok"}}"#,
        );

        let paths = QuotaPaths::in_dir(&app_data);
        let mut backoff = official_quota::backoff::BackoffState::default();
        official_quota::backoff::record_failure(&mut backoff, "codex", "HTTP 429", fixed_time());
        official_quota::backoff::save_state(&paths.backoff, &backoff).unwrap();

        let transport = Arc::new(FakeTransport::new());
        let env = QuotaEnv::isolated(ScanEnv::new(&home), paths.clone(), fixed_time())
            .with_transport(transport);
        let conns = crate::test_support::TestConnection::new(crate::store::open_memory().unwrap());

        let emitted = Mutex::new(0usize);
        let on_updated = |_: &crate::domain::OfficialQuotaDto| {
            *emitted.lock().unwrap() += 1;
        };

        // Throttled refresh during cooldown: overlays cooldown error without writing SQLite or emitting.
        let throttled = official_quota::refresh_target_throttled(
            &conns,
            &env,
            "codex",
            official_quota::RefreshOptions {
                notify: None,
                on_updated: Some(&on_updated),
                stream_progress: false,
            },
        )
        .unwrap();
        assert_eq!(conns.write_locks(), 0);
        assert_eq!(*emitted.lock().unwrap(), 0);
        let codex = throttled
            .rows
            .iter()
            .find(|r| r.provider == "codex")
            .unwrap();
        assert!(codex
            .error
            .as_deref()
            .is_some_and(|msg| msg.contains("分钟后自动重试")));

        // Forced refresh bypasses cooldown, writes SQLite, clears backoff, and emits updated snapshot.
        let forced = official_quota::refresh_target_forced(
            &conns,
            &env,
            "codex",
            official_quota::RefreshOptions {
                notify: None,
                on_updated: Some(&on_updated),
                stream_progress: false,
            },
        )
        .unwrap();
        assert_eq!(conns.write_locks(), 1);
        assert_eq!(*emitted.lock().unwrap(), 1);
        assert_eq!(
            forced
                .rows
                .iter()
                .find(|r| r.provider == "codex")
                .unwrap()
                .windows[0]
                .used_percent,
            Some(30.0)
        );
        assert!(official_quota::backoff::cooldown_remaining(
            &official_quota::backoff::load_state(&paths.backoff),
            "codex",
            fixed_time()
        )
        .is_none());
    }

    #[test]
    fn quota_env_supports_injected_keychain_for_antigravity_and_computes_exhaust_on_capture_overlay(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();

        let blob = format!(
            "go-keyring-base64:{}",
            base64::engine::general_purpose::STANDARD.encode(
                r#"{"token":{"access_token":"ya29.from-keychain","email":"kc@example.com"}}"#
            )
        );
        let mut transport = FakeTransport::new();
        transport.antigravity_keychain = Some(blob);
        let transport = Arc::new(transport);

        let paths = QuotaPaths::in_dir(&app_data);
        let env = QuotaEnv::isolated(ScanEnv::new(&home), paths.clone(), fixed_time())
            .with_transport(transport.clone());
        assert!(detect::has_local_credentials_in(
            &env,
            OfficialQuotaProvider::Antigravity
        ));
        let snap =
            official_quota::fetch_provider_in(&env, OfficialQuotaProvider::Antigravity).unwrap();
        assert_eq!(snap.windows[0].used_percent, Some(35.0));
        assert_eq!(
            transport
                .header_of("retrieveUserQuotaSummary", "Authorization")
                .as_deref(),
            Some("Bearer ya29.from-keychain")
        );

        // In-memory capture overlay preserves previous SQLite snapshot for exhaust rate estimation.
        write_file(
            &paths.claude_capture,
            r#"{"rate_limits":{"five_hour":{"used_percentage":60.0,"resets_at":"2099-01-01T10:00:00Z"}}}"#,
        );
        let capture_ts = official_quota::claude::file_captured_at(&paths.claude_capture).unwrap();
        let capture_dt = DateTime::parse_from_rfc3339(&capture_ts)
            .unwrap()
            .with_timezone(&Utc);
        let prev_dt = capture_dt - chrono::Duration::minutes(10);
        let conn = crate::store::open_memory().unwrap();
        official_quota::apply_success(
            &conn,
            OfficialQuotaProvider::Claude,
            vec![crate::domain::OfficialQuotaWindow {
                kind: "session_5h".into(),
                label: "5 小时".into(),
                used_percent: Some(50.0),
                resets_at: Some("2099-01-01T10:00:00Z".into()),
                ..Default::default()
            }],
            &prev_dt.to_rfc3339(),
        )
        .unwrap();

        let overlay_env = QuotaEnv::isolated(
            ScanEnv::new(&home),
            paths,
            capture_dt + chrono::Duration::seconds(10),
        );
        let dto = official_quota::snapshot(&conn, &overlay_env);
        let claude = dto.rows.iter().find(|r| r.provider == "claude").unwrap();
        assert_eq!(claude.windows[0].used_percent, Some(60.0));
        assert_eq!(
            claude.windows[0].exhaust.as_ref().map(|e| e.kind),
            Some(crate::domain::QuotaExhaustKind::Hits)
        );
    }

    fn antigravity_nested_refresh_blob(refresh: &str) -> String {
        let engine = base64::engine::general_purpose::STANDARD;
        fn field1(bytes: &[u8]) -> Vec<u8> {
            let mut out = vec![0x0a];
            let mut len = bytes.len();
            while len >= 0x80 {
                out.push((len as u8) | 0x80);
                len >>= 7;
            }
            out.push(len as u8);
            out.extend_from_slice(bytes);
            out
        }
        let mut inner = field1(b"ya29.inner-access-ignored");
        inner.extend(field1(refresh.as_bytes()));
        engine.encode(field1(&engine.encode(&inner).into_bytes()))
    }

    #[test]
    fn active_local_antigravity_fetch_via_transport_uses_live_access_token_and_skips_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        let source = dir.path().join("antigravity_storage");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();

        let refresh = "1//antigravity-refresh-STABLE-should-not-leak-t9k2";
        let stored_access = "ya29.antigravity-STORED-ACCESS-should-not-use";
        let live_access = "ya29.antigravity-LIVE-ACCESS-should-use";

        let write_ag = |access: &str| {
            let _ = std::fs::remove_file(source.join("state.vscdb"));
            seed_vscdb(
                &source,
                &[
                    (
                        "antigravityAuthStatus",
                        &format!(r#"{{"apiKey":"{access}"}}"#),
                    ),
                    (
                        "antigravityUnifiedStateSync.oauthToken",
                        &antigravity_nested_refresh_blob(refresh),
                    ),
                ],
            );
        };

        write_ag(stored_access);
        let paths = QuotaPaths::in_dir(&app_data);
        let saved = accounts::import_local_login(
            &paths.accounts,
            OfficialQuotaProvider::Antigravity,
            &source,
            &paths.backoff,
        )
        .unwrap();

        write_ag(live_access);
        let identity =
            official_quota::read_identity(OfficialQuotaProvider::Antigravity, &source).unwrap();
        let resolved = accounts::load_resolved(&paths.accounts);
        let login = accounts::LocalLogin {
            provider: OfficialQuotaProvider::Antigravity,
            fingerprint: identity.fingerprint,
            root: Some(source.clone()),
        };

        let transport = Arc::new(FakeTransport::new());
        let env = QuotaEnv::isolated(ScanEnv::new(&home), paths.clone(), fixed_time())
            .with_transport(transport.clone());

        let targets = official_quota::targets_for_fetch_in(&env, &[], &resolved, &[login]);
        let official_quota::FetchTarget::Account(target) = targets
            .into_iter()
            .find(|t| t.quota_id() == saved.saved_id)
            .expect("active local antigravity account should be a fetch target")
        else {
            panic!("expected FetchTarget::Account");
        };
        assert!(target.live_root.is_some());
        assert!(target.refresh.is_none());

        let snapshot = accounts::fetch_managed_account_in(&env, &target).unwrap();
        assert_eq!(snapshot.windows[0].used_percent, Some(35.0));
        assert_eq!(snapshot.captured_at, fixed_time().to_rfc3339());

        let requests = transport.requests.lock().unwrap();
        assert!(requests.iter().any(|req| {
            req.url.contains("retrieveUserQuotaSummary")
                && req.headers.iter().any(|(k, v)| {
                    k.eq_ignore_ascii_case("authorization") && v == &format!("Bearer {live_access}")
                })
        }));
        assert!(requests
            .iter()
            .all(|req| { req.headers.iter().all(|(_, v)| !v.contains(stored_access)) }));
        assert!(requests.iter().all(|req| !req.url.contains("/token")));

        let secrets_raw = std::fs::read_to_string(&paths.accounts.secrets).unwrap();
        assert!(secrets_raw.contains(refresh));
        assert!(!secrets_raw.contains(live_access));
    }

    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        while value >= 0x80 {
            out.push(((value as u8) & 0x7f) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
        out
    }

    fn grok_grpc_credits_frame(fraction: f32, reset_secs: u64) -> Vec<u8> {
        let mut ts = vec![0x08];
        ts.extend(encode_varint(reset_secs));
        let mut config = Vec::new();
        config.push(0x0d); // field 1 fixed32
        config.extend_from_slice(&fraction.to_le_bytes());
        config.push(0x2a); // field 5 length-delimited
        config.extend(encode_varint(ts.len() as u64));
        config.extend(ts);
        let mut payload = vec![0x0a]; // field 1 length-delimited
        payload.extend(encode_varint(config.len() as u64));
        payload.extend(config);
        let mut frame = vec![0u8];
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend(payload);
        frame
    }

    #[test]
    fn grok_managed_account_5xx_fallback_uses_injected_transport_and_clock() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();

        let injected_now = DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let reset_at = DateTime::parse_from_rfc3339("2020-01-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let grpc_body = grok_grpc_credits_frame(0.42, reset_at.timestamp() as u64);

        struct Grok5xxFallbackTransport {
            grpc_body: Vec<u8>,
            seen_urls: Mutex<Vec<String>>,
        }
        impl QuotaTransport for Grok5xxFallbackTransport {
            fn http(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
                self.seen_urls.lock().unwrap().push(request.url.clone());
                if request.url.contains("/v1/billing?format=credits") {
                    return Ok(HttpResponse {
                        status: 500,
                        headers: Vec::new(),
                        body: br#"{"error":"Failed to serialize billing response"}"#.to_vec(),
                    });
                }
                if request.url.contains("GetGrokCreditsConfig") {
                    return Ok(HttpResponse {
                        status: 200,
                        headers: vec![("grpc-status".into(), "0".into())],
                        body: self.grpc_body.clone(),
                    });
                }
                if request.url.contains("/v1/billing") {
                    return Ok(HttpResponse {
                        status: 200,
                        headers: Vec::new(),
                        body: br#"{"config":{}}"#.to_vec(),
                    });
                }
                if request.url.contains("/v1/settings") {
                    return Ok(HttpResponse {
                        status: 200,
                        headers: Vec::new(),
                        body: br#"{"config":{"subscription_tier_display":"SuperGrok"}}"#.to_vec(),
                    });
                }
                Err(format!("unexpected url: {}", request.url))
            }
        }

        let transport = Arc::new(Grok5xxFallbackTransport {
            grpc_body,
            seen_urls: Mutex::new(Vec::new()),
        });
        let paths = QuotaPaths::in_dir(&app_data);
        let env = QuotaEnv::isolated(ScanEnv::new(&home), paths, injected_now)
            .with_transport(transport.clone());

        let target = accounts::AccountTarget {
            id: "acct:grok:0000000000000001".into(),
            name: "Grok Managed".into(),
            provider: OfficialQuotaProvider::Grok,
            access_token: "grok-managed-access-token".into(),
            account_id: Some("user-42".into()),
            live_root: None,
            live_default: false,
            refresh: None,
            secrets_path: None,
        };

        let snapshot = accounts::fetch_managed_account_in(&env, &target).unwrap();
        assert_eq!(snapshot.captured_at, injected_now.to_rfc3339());
        assert_eq!(snapshot.plan.as_deref(), Some("SuperGrok"));
        assert_eq!(snapshot.windows.len(), 1);
        assert!((snapshot.windows[0].used_percent.unwrap() - 42.0).abs() < 0.01);
        assert_eq!(
            snapshot.windows[0].resets_at.as_deref(),
            Some(reset_at.to_rfc3339().as_str())
        );
        assert!(transport
            .seen_urls
            .lock()
            .unwrap()
            .iter()
            .any(|u| u.contains("GetGrokCreditsConfig")));
    }

    #[test]
    fn import_default_in_respects_injected_clock_for_grok_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let app_data = dir.path().join("app_data");
        std::fs::create_dir_all(&app_data).unwrap();

        // Expires in 2020-06-01: expired against wall-clock Utc::now(), valid at injected_now (2020-01-01).
        write_file(
            &home.join(".grok/auth.json"),
            r#"{"https://auth.x.ai::openid":{"key":"grok-clock-token","auth_mode":"oidc","user_id":"user-clock-1","expires_at":"2020-06-01T00:00:00Z"}}"#,
        );

        let injected_now = DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let paths = QuotaPaths::in_dir(&app_data);
        let env = QuotaEnv::isolated(ScanEnv::new(&home), paths, injected_now);

        let saved = accounts::import_default_in(&env, OfficialQuotaProvider::Grok).unwrap();
        assert_eq!(saved.panel.accounts.len(), 1);
        assert_eq!(saved.panel.accounts[0].hint, "user-clock-1");
    }
}
