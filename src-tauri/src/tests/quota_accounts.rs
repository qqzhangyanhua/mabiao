//! 额度账号的前置缝：零个账号时官方额度仍是内置单行；身份只从显式目录读取。

use std::path::{Path, PathBuf};

use base64::Engine;

use crate::domain::{
    OfficialQuotaDto, OfficialQuotaFreshness, OfficialQuotaProvider, OfficialQuotaRow,
    OfficialQuotaWindow,
};
use crate::official_quota::notify::{self, NotifyState};
use crate::official_quota::{self, QuotaIdentity};
use crate::store;

const CLAUDE_SECRET: &str = "sk-ant-oat01-SUPER-SECRET-TOKEN-9f3a";
/// `echo -n sk-ant-oat01-SUPER-SECRET-TOKEN-9f3a | sha256sum`
const CLAUDE_FINGERPRINT: &str = "b21474965ffb573735a716b68d9a111cb674e69d7868307a7b78eaf049b2c7b1";
const MASKED_TAIL: &str = "••••••9f3a";

fn assert_secret_absent(identity: &QuotaIdentity, secrets: &[&str]) {
    for secret in secrets {
        assert!(
            !identity.fingerprint.contains(secret),
            "fingerprint leaked {secret}"
        );
        assert!(!identity.label.contains(secret), "label leaked {secret}");
        assert_ne!(identity.fingerprint, *secret);
        assert_ne!(identity.label, *secret);
    }
}

fn assert_opaque(identity: &QuotaIdentity, secret: &str) {
    assert_secret_absent(identity, &[secret]);
    assert_eq!(identity.fingerprint.len(), 64);
    assert!(identity
        .fingerprint
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_eq!(identity.label, MASKED_TAIL);
}

fn jwt(payload: &str) -> String {
    let encode =
        |raw: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes());
    format!(
        "{}.{}.cursor-signature-SECRET",
        encode(r#"{"alg":"none"}"#),
        encode(payload)
    )
}

fn write_file(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, body).unwrap();
    path
}

fn seed_state_db(dir: &Path, items: &[(&str, &str)]) {
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

fn read(provider: OfficialQuotaProvider, root: &Path) -> QuotaIdentity {
    official_quota::read_identity(provider, root)
        .unwrap_or_else(|error| panic!("{} identity failed: {error}", provider.as_str()))
}

#[test]
fn old_quota_row_json_still_parses_and_omits_default_account_fields() {
    let raw = r#"{
        "provider": "claude",
        "application": "Claude",
        "windows": [],
        "freshness": "unavailable",
        "captured_at": null,
        "error": null,
        "todo": null,
        "plan": null
    }"#;
    let row: OfficialQuotaRow = serde_json::from_str(raw).unwrap();
    assert!(row.base_provider.is_none());
    assert!(row.account_label.is_none());
    assert!(!row.is_active_local);
    let back = serde_json::to_value(&row).unwrap();
    assert!(back.get("base_provider").is_none());
    assert!(back.get("account_label").is_none());
    assert!(back.get("is_active_local").is_none());
    assert!(back.get("todo").unwrap().is_null());
    assert!(back.get("plan").unwrap().is_null());

    let mut labeled = row;
    labeled.base_provider = Some("claude".into());
    labeled.account_label = Some("工作号".into());
    labeled.is_active_local = true;
    let value = serde_json::to_value(&labeled).unwrap();
    assert_eq!(value["base_provider"], "claude");
    assert_eq!(value["account_label"], "工作号");
    assert_eq!(value["is_active_local"], true);
}

#[test]
fn load_dto_without_quota_accounts_keeps_builtin_single_rows() {
    let conn = store::open_memory().unwrap();
    let dto = official_quota::load_dto(
        &conn,
        &crate::domain::OfficialQuotaConfig::default(),
        &[],
        chrono::Utc::now(),
    );
    assert_eq!(OfficialQuotaProvider::ALL.len(), 9);
    assert!(OfficialQuotaProvider::ALL
        .iter()
        .all(|provider| !provider.as_str().contains("acct:")));
    assert!(dto.rows.iter().all(|row| {
        OfficialQuotaProvider::parse(&row.provider).is_some() && !row.provider.starts_with("acct:")
    }));
    for row in &dto.rows {
        let provider = OfficialQuotaProvider::parse(&row.provider).unwrap();
        assert_eq!(row.application, provider.display_name());
        assert_eq!(row.provider, provider.as_str());
        assert!(row.base_provider.is_none());
        assert!(row.account_label.is_none());
        assert!(!row.is_active_local);
    }
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("base_provider"));
    assert!(!json.contains("account_label"));
    assert!(!json.contains("is_active_local"));
    assert!(!json.contains("acct:"));
}

#[test]
fn quota_row_title_uses_the_account_label_only_when_present() {
    assert_eq!(
        official_quota::quota_row_application("Codex", None),
        "Codex"
    );
    assert_eq!(
        official_quota::quota_row_application("Codex", Some("  ")),
        "Codex"
    );
    assert_eq!(
        official_quota::quota_row_application("Codex", Some("工作号")),
        "Codex · 工作号"
    );
    assert_eq!(official_quota::quota_active_local_mark(false), None);
    assert_eq!(
        official_quota::quota_active_local_mark(true),
        Some("当前本机")
    );
}

fn window(kind: &str, percent: f64, resets_at: Option<&str>) -> OfficialQuotaWindow {
    OfficialQuotaWindow {
        kind: kind.into(),
        label: kind.into(),
        used_percent: Some(percent),
        resets_at: resets_at.map(str::to_string),
        ..Default::default()
    }
}

fn row(provider: &str, application: &str, windows: Vec<OfficialQuotaWindow>) -> OfficialQuotaRow {
    OfficialQuotaRow {
        provider: provider.into(),
        application: application.into(),
        windows,
        freshness: OfficialQuotaFreshness::Official,
        captured_at: Some("2026-08-18T12:00:00+00:00".into()),
        error: None,
        todo: None,
        plan: None,
        base_provider: None,
        account_label: None,
        is_active_local: false,
    }
}

fn dto(rows: Vec<OfficialQuotaRow>) -> OfficialQuotaDto {
    OfficialQuotaDto {
        rows,
        alerts_enabled: true,
        stale_after_minutes: 10,
        undetected: Vec::new(),
        hidden_providers: Vec::new(),
    }
}

#[test]
fn visible_rows_hides_acct_rows_by_base_provider_and_keeps_custom() {
    let mut attributed = row("acct:codex:ee56", "Claude · 另一号", vec![]);
    attributed.base_provider = Some("claude".into());
    let mut padded = row("acct:codex:ff78", "Codex · 空格", vec![]);
    padded.base_provider = Some(" codex ".into());
    let rows = vec![
        row("claude", "Claude", vec![]),
        row("codex", "Codex", vec![]),
        row("acct:codex:ab12", "Codex · 工作号", vec![]),
        row("acct:codex:cd34", "Codex", vec![]),
        attributed,
        padded,
        row("custom:a3f9c1", "公司的中转", vec![]),
    ];

    let hidden_codex = official_quota::visible_rows(rows.clone(), &["codex".into()]);
    let ids: Vec<&str> = hidden_codex
        .iter()
        .map(|row| row.provider.as_str())
        .collect();
    assert_eq!(ids, vec!["claude", "acct:codex:ee56", "custom:a3f9c1"]);

    let hidden_claude = official_quota::visible_rows(rows.clone(), &["claude".into()]);
    let ids: Vec<&str> = hidden_claude
        .iter()
        .map(|row| row.provider.as_str())
        .collect();
    assert_eq!(
        ids,
        vec![
            "codex",
            "acct:codex:ab12",
            "acct:codex:cd34",
            "acct:codex:ff78",
            "custom:a3f9c1",
        ]
    );

    let all_hidden: Vec<String> = OfficialQuotaProvider::ALL
        .iter()
        .map(|provider| provider.as_str().to_string())
        .collect();
    let only_custom = official_quota::visible_rows(rows, &all_hidden);
    assert_eq!(only_custom.len(), 1);
    assert_eq!(only_custom[0].provider, "custom:a3f9c1");
}

#[test]
fn tightest_window_lets_acct_rows_compete_without_a_reset_time() {
    let quota = dto(vec![
        row("claude", "Claude", vec![window("session_5h", 10.0, None)]),
        row(
            "custom:a3f9c1",
            "公司的中转",
            vec![window("budget_window", 99.0, None)],
        ),
        row(
            "acct:codex:ab12",
            "Codex · 工作号",
            vec![window("session_5h", 96.0, None)],
        ),
    ]);
    let tightest = official_quota::tightest_window(&quota).unwrap();
    assert_eq!(tightest.provider, "Codex · 工作号");
    assert_eq!(tightest.label, "5h");
    assert_eq!(tightest.used_percent, 96.0);
    assert_eq!(
        crate::tray::format_title_with_quota(Some(1.23), false, Some(&tightest)),
        "$1.23 · Codex · 工作号 5h 96%"
    );
}

#[test]
fn notifications_dedupe_per_quota_id_and_use_the_labeled_application() {
    let quota = dto(vec![
        row(
            "acct:codex:aa11",
            "Codex · 工作号",
            vec![window("weekly", 90.0, Some("2026-12-01T00:00:00+00:00"))],
        ),
        row(
            "acct:codex:bb22",
            "Codex · 个人号",
            vec![window("weekly", 91.0, Some("2026-12-01T00:00:00+00:00"))],
        ),
    ]);
    let (state, alerts) = notify::prepare_notifications(NotifyState::default(), &quota);
    assert_eq!(alerts.len(), 2);
    assert_eq!(alerts[0].provider, "Codex · 工作号");
    assert_eq!(alerts[1].provider, "Codex · 个人号");
    let keys: Vec<&str> = state
        .entries
        .iter()
        .map(|entry| entry.provider.as_str())
        .collect();
    assert_eq!(keys, vec!["acct:codex:aa11", "acct:codex:bb22"]);
    let (_, again) = notify::prepare_notifications(state, &quota);
    assert!(again.is_empty());
}

#[test]
fn acct_rows_without_a_reset_time_do_not_take_the_custom_alert_exception() {
    let quota = dto(vec![
        row(
            "acct:codex:ab12",
            "Codex · 工作号",
            vec![window("weekly", 90.0, None)],
        ),
        row(
            "custom:a3f9c1",
            "公司的中转",
            vec![window("weekly", 90.0, None)],
        ),
    ]);
    let (state, alerts) = notify::prepare_notifications(NotifyState::default(), &quota);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].provider, "公司的中转");
    assert_eq!(state.entries.len(), 1);
    assert_eq!(state.entries[0].provider, "custom:a3f9c1");
}

#[test]
fn claude_identity_hashes_the_access_token_even_when_expiry_is_zero() {
    let dir = tempfile::tempdir().unwrap();
    let body = format!(r#"{{"claudeAiOauth":{{"accessToken":"{CLAUDE_SECRET}","expiresAt":0}}}}"#);
    let file = write_file(dir.path(), ".credentials.json", &body);
    let from_dir = read(OfficialQuotaProvider::Claude, dir.path());
    let from_file = read(OfficialQuotaProvider::Claude, &file);
    assert_eq!(from_dir, from_file);
    assert_eq!(from_dir.fingerprint, CLAUDE_FINGERPRINT);
    assert_opaque(&from_dir, CLAUDE_SECRET);
}

#[test]
fn codex_identity_prefers_account_id_and_stays_stable_across_tokens() {
    let token_a = "codex-SECRET-TOKEN-9f3a";
    let token_b = "codex-OTHER-SECRET-TOKEN-9f3a";
    let refresh = "codex-refresh-SHOULD-NOT-LEAK";
    let body = |token: &str| {
        format!(
            r#"{{"tokens":{{"access_token":"{token}","account_id":"acct_01HZwork","refresh_token":"{refresh}"}}}}"#
        )
    };
    let dir_a = tempfile::tempdir().unwrap();
    let file = write_file(dir_a.path(), "auth.json", &body(token_a));
    let from_dir = read(OfficialQuotaProvider::Codex, dir_a.path());
    let from_file = read(OfficialQuotaProvider::Codex, &file);
    assert_eq!(from_dir, from_file);
    assert_eq!(from_dir.fingerprint, "acct_01HZwork");
    assert_eq!(from_dir.label, "acct_01HZwork");
    assert_secret_absent(&from_dir, &[token_a, refresh]);

    let dir_b = tempfile::tempdir().unwrap();
    write_file(dir_b.path(), "auth.json", &body(token_b));
    let other = read(OfficialQuotaProvider::Codex, dir_b.path());
    assert_eq!(other.fingerprint, from_dir.fingerprint);
    assert_secret_absent(&other, &[token_b, refresh]);
}

#[test]
fn codex_identity_hashes_when_the_account_id_is_the_secret() {
    let secret = "same-secret-TOKEN-9f3a";
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "auth.json",
        &format!(r#"{{"tokens":{{"access_token":"{secret}","account_id":"{secret}"}}}}"#),
    );
    let identity = read(OfficialQuotaProvider::Codex, dir.path());
    assert_opaque(&identity, secret);
}

#[test]
fn cursor_identity_uses_the_workos_user_id_and_cached_email() {
    let token = jwt(r#"{"sub":"google-oauth|user_01JABC","exp":4102444800}"#);
    let dir = tempfile::tempdir().unwrap();
    seed_state_db(
        dir.path(),
        &[
            ("cursorAuth/accessToken", token.as_str()),
            ("cursorAuth/cachedEmail", "ada@example.com"),
        ],
    );
    let db = dir.path().join("state.vscdb");
    let from_dir = read(OfficialQuotaProvider::Cursor, dir.path());
    let from_file = read(OfficialQuotaProvider::Cursor, &db);
    assert_eq!(from_dir, from_file);
    assert_eq!(from_dir.fingerprint, "user_01JABC");
    assert_eq!(from_dir.label, "ada@example.com");
    let session = format!("user_01JABC%3A%3A{token}");
    assert_secret_absent(
        &from_dir,
        &[&token, "cursor-signature-SECRET", "%3A%3A", &session],
    );
}

#[test]
fn grok_identity_uses_user_id_and_hashes_when_it_is_missing() {
    let token = "grok-SECRET-TOKEN-9f3a";
    let refresh = "grok-refresh-SHOULD-NOT-LEAK-zzzz";
    let with_user = format!(
        r#"{{"https://auth.x.ai::openid":{{"key":"{token}","auth_mode":"oidc","user_id":"user-123","refresh_token":"{refresh}"}}}}"#
    );
    let dir = tempfile::tempdir().unwrap();
    let file = write_file(dir.path(), "auth.json", &with_user);
    let from_dir = read(OfficialQuotaProvider::Grok, dir.path());
    let from_file = read(OfficialQuotaProvider::Grok, &file);
    assert_eq!(from_dir, from_file);
    assert_eq!(from_dir.fingerprint, "user-123");
    assert_eq!(from_dir.label, "user-123");
    assert_secret_absent(&from_dir, &[token, refresh]);

    let opaque = format!(
        r#"{{"https://auth.x.ai::openid":{{"key":"{token}","auth_mode":"oidc","refresh_token":"{refresh}"}}}}"#
    );
    let other = tempfile::tempdir().unwrap();
    write_file(other.path(), "auth.json", &opaque);
    let hashed = read(OfficialQuotaProvider::Grok, other.path());
    assert_opaque(&hashed, token);
    assert_secret_absent(&hashed, &[refresh, "••••••zzzz"]);
}

#[test]
fn droid_identity_prefers_org_id_over_the_access_token() {
    let access = jwt(
        r#"{"sub":"user_sub_SHOULD_NOT_WIN","external_org_id":"org_from_jwt","email":"ada@factory.ai"}"#,
    );
    let refresh = "droid-refresh-SHOULD-NOT-LEAK";
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "auth.json",
        &format!(
            r#"{{"access_token":"{access}","active_organization_id":"org_factory_9","refresh_token":"{refresh}"}}"#
        ),
    );
    let identity = read(OfficialQuotaProvider::Droid, dir.path());
    assert_eq!(identity.fingerprint, "org_factory_9");
    assert_eq!(identity.label, "org_factory_9");
    assert_secret_absent(
        &identity,
        &[&access, refresh, "user_sub_SHOULD_NOT_WIN", "org_from_jwt"],
    );

    let jwt_only = tempfile::tempdir().unwrap();
    write_file(
        jwt_only.path(),
        "auth.json",
        &format!(r#"{{"access_token":"{access}","refresh_token":"{refresh}"}}"#),
    );
    let from_jwt = read(OfficialQuotaProvider::Droid, jwt_only.path());
    assert_eq!(from_jwt.fingerprint, "org_from_jwt");
    assert_eq!(from_jwt.label, "org_from_jwt");
    assert_secret_absent(&from_jwt, &[&access, refresh, "user_sub_SHOULD_NOT_WIN"]);

    let sub_only = jwt(r#"{"sub":"user_droid_sub","email":"ada@factory.ai"}"#);
    let sub_dir = tempfile::tempdir().unwrap();
    write_file(
        sub_dir.path(),
        "auth.json",
        &format!(r#"{{"access_token":"{sub_only}"}}"#),
    );
    let from_sub = read(OfficialQuotaProvider::Droid, sub_dir.path());
    assert_eq!(from_sub.fingerprint, "user_droid_sub");
    assert_eq!(from_sub.label, "ada@factory.ai");
    assert_secret_absent(&from_sub, &[&sub_only, "cursor-signature-SECRET"]);
}

#[test]
fn droid_identity_does_not_fall_through_when_keyfile_decrypt_fails() {
    let secret = "auth-json-SECRET-TOKEN-9f3a";
    let dir = tempfile::tempdir().unwrap();
    write_file(dir.path(), "auth.v2.file", "not-a-payload");
    write_file(dir.path(), "auth.v2.key", "not-a-key");
    write_file(
        dir.path(),
        "auth.json",
        &format!(r#"{{"access_token":"{secret}","active_organization_id":"org_factory_9"}}"#),
    );
    let error =
        official_quota::read_identity(OfficialQuotaProvider::Droid, dir.path()).unwrap_err();
    assert!(!error.contains(secret));
    assert!(!error.contains("org_factory_9"));
}

#[test]
fn antigravity_identity_hashes_the_local_access_token() {
    let secret = "ya29.antigravity-SECRET-TOKEN-9f3a";
    let dir = tempfile::tempdir().unwrap();
    seed_state_db(
        dir.path(),
        &[(
            "antigravityAuthStatus",
            &format!(r#"{{"apiKey":"{secret}"}}"#),
        )],
    );
    let identity = read(
        OfficialQuotaProvider::Antigravity,
        &dir.path().join("state.vscdb"),
    );
    assert_opaque(&identity, secret);
}

#[test]
fn opencode_identity_hashes_only_the_zen_key() {
    let zen = "zen-SECRET-TOKEN-9f3a";
    let other = "other-provider-SECRET-zzzz";
    let dir = tempfile::tempdir().unwrap();
    let file = write_file(
        dir.path(),
        "auth.json",
        &format!(
            r#"{{"opencode-go":{{"type":"api","key":"{zen}"}},"anthropic":{{"type":"api","key":"{other}"}}}}"#
        ),
    );
    let from_dir = read(OfficialQuotaProvider::OpenCode, dir.path());
    let from_file = read(OfficialQuotaProvider::OpenCode, &file);
    assert_eq!(from_dir, from_file);
    assert_opaque(&from_dir, zen);
    assert_secret_absent(&from_dir, &[other]);
}

#[test]
fn copilot_identity_uses_the_user_on_the_same_entry_as_the_token() {
    let token = "gho_COPILOT-SECRET-TOKEN-9f3a";
    let other_token = "gho_hosts-SECRET-SHOULD-NOT-WIN";
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "github-copilot/apps.json",
        &format!(
            r#"{{"github.com:client":{{"oauth_token":"{token}","user":"octocat"}},"ignored":{{"user":"someone-else"}}}}"#
        ),
    );
    write_file(
        dir.path(),
        "gh/hosts.yml",
        &format!("github.com:\n  oauth_token: {other_token}\n  user: other-user\n"),
    );
    let identity = read(OfficialQuotaProvider::Copilot, dir.path());
    assert_eq!(identity.fingerprint, "octocat");
    assert_eq!(identity.label, "octocat");
    assert_secret_absent(&identity, &[token, other_token, "other-user"]);

    let hosts = tempfile::tempdir().unwrap();
    let hosts_token = "gho_hosts-SECRET-TOKEN-9f3a";
    write_file(
        hosts.path(),
        "hosts.yml",
        &format!("ghe.example.com:\n  oauth_token: gho_enterprise\ngithub.com:\n  oauth_token: \"{hosts_token}\"\n"),
    );
    let hashed = read(
        OfficialQuotaProvider::Copilot,
        &hosts.path().join("hosts.yml"),
    );
    assert_opaque(&hashed, hosts_token);
    assert_secret_absent(&hashed, &["gho_enterprise"]);
}

#[test]
fn devin_identity_hashes_the_api_key() {
    let secret = "devin-SECRET-TOKEN-9f3a";
    let dir = tempfile::tempdir().unwrap();
    seed_state_db(
        dir.path(),
        &[(
            "windsurfAuthStatus",
            &format!(r#"{{"apiKey":"{secret}","userStatusProtoBinaryBase64":"not-decoded"}}"#),
        )],
    );
    let identity = read(OfficialQuotaProvider::Devin, dir.path());
    assert_opaque(&identity, secret);
    assert_secret_absent(&identity, &["not-decoded"]);
}
