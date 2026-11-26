//! 隔离目录与已分离快照续期。
//!
//! 目录只存路径，取数时现场读。续期只改码表自己的凭证文件。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use base64::Engine;

use crate::domain::{OfficialQuotaConfig, OfficialQuotaProvider, OfficialQuotaWindow};
use crate::official_quota::accounts::{
    self, QuotaAccountPaths, SaveProfileDirAccount, LOGIN_EXPIRED,
};
use crate::official_quota::fetch::{self, ThrottledFetch};
use crate::official_quota::{self, FetchTarget, QuotaTarget};
use crate::store;

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

fn jwt(payload: &str) -> String {
    let encode =
        |raw: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes());
    format!(
        "{}.{}.cursor-signature-SECRET",
        encode(r#"{"alg":"none"}"#),
        encode(payload)
    )
}

fn paths(dir: &Path) -> QuotaAccountPaths {
    QuotaAccountPaths::in_dir(dir)
}

fn backoff(dir: &Path) -> PathBuf {
    dir.join("official_quota_backoff.json")
}

fn save_dir(
    app: &Path,
    provider: OfficialQuotaProvider,
    name: &str,
    directory: &Path,
) -> accounts::SavedQuotaAccountDto {
    accounts::save_profile_dir(
        &paths(app),
        SaveProfileDirAccount {
            id: None,
            provider: provider.as_str().to_string(),
            name: name.to_string(),
            profile_dir: directory.to_string_lossy().into_owned(),
        },
        &backoff(app),
    )
    .unwrap_or_else(|error| panic!("{} profile save failed: {error}", provider.as_str()))
}

fn seed_provider(provider: OfficialQuotaProvider, dir: &Path, marker: &str) {
    match provider {
        OfficialQuotaProvider::Claude => {
            write_file(
                dir,
                ".credentials.json",
                &format!(r#"{{"claudeAiOauth":{{"accessToken":"{marker}","expiresAt":0}}}}"#),
            );
        }
        OfficialQuotaProvider::Codex => {
            write_file(
                dir,
                "auth.json",
                &format!(
                    r#"{{"tokens":{{"access_token":"{marker}","account_id":"acct_01HZwork","refresh_token":"refresh-DIRECTORY-ONLY"}}}}"#
                ),
            );
        }
        OfficialQuotaProvider::Cursor => {
            let token = jwt(r#"{"sub":"google-oauth|user_01JABC","exp":4102444800}"#);
            assert!(token.contains(marker));
            seed_state_db(
                dir,
                &[
                    ("cursorAuth/accessToken", token.as_str()),
                    ("cursorAuth/cachedEmail", "ada@example.com"),
                ],
            );
        }
        OfficialQuotaProvider::Grok => {
            write_file(
                dir,
                "auth.json",
                &format!(
                    r#"{{"https://auth.x.ai::openid":{{"key":"{marker}","auth_mode":"oidc","user_id":"user-123"}}}}"#
                ),
            );
        }
        OfficialQuotaProvider::Droid => {
            write_file(
                dir,
                "auth.json",
                &format!(
                    r#"{{"access_token":"{marker}","active_organization_id":"org_factory_9"}}"#
                ),
            );
        }
        OfficialQuotaProvider::Antigravity => {
            seed_state_db(
                dir,
                &[(
                    "antigravityAuthStatus",
                    &format!(r#"{{"apiKey":"{marker}"}}"#),
                )],
            );
        }
        OfficialQuotaProvider::OpenCode => {
            write_file(
                dir,
                "auth.json",
                &format!(r#"{{"opencode-go":{{"type":"api","key":"{marker}"}}}}"#),
            );
        }
        OfficialQuotaProvider::Copilot => {
            write_file(
                dir,
                "github-copilot/apps.json",
                &format!(
                    r#"{{"github.com:client":{{"oauth_token":"{marker}","user":"octocat"}}}}"#
                ),
            );
        }
        OfficialQuotaProvider::Devin => {
            seed_state_db(
                dir,
                &[("windsurfAuthStatus", &format!(r#"{{"apiKey":"{marker}"}}"#))],
            );
        }
    }
}

fn credential_file(provider: OfficialQuotaProvider, dir: &Path) -> PathBuf {
    match provider {
        OfficialQuotaProvider::Claude => dir.join(".credentials.json"),
        OfficialQuotaProvider::Codex
        | OfficialQuotaProvider::Grok
        | OfficialQuotaProvider::Droid
        | OfficialQuotaProvider::OpenCode => dir.join("auth.json"),
        OfficialQuotaProvider::Cursor
        | OfficialQuotaProvider::Antigravity
        | OfficialQuotaProvider::Devin => dir.join("state.vscdb"),
        OfficialQuotaProvider::Copilot => dir.join("github-copilot").join("apps.json"),
    }
}

fn file_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

struct Hit {
    raw: String,
}

struct Stub {
    base: String,
    hits: Arc<Mutex<Vec<Hit>>>,
    fail_token: Arc<AtomicBool>,
    old_access: Arc<Mutex<Vec<String>>>,
    new_access: String,
    new_refresh: String,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Stub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let hits = Arc::new(Mutex::new(Vec::new()));
        let fail_token = Arc::new(AtomicBool::new(false));
        let old_access = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let new_access = "access-NEW".to_string();
        let new_refresh = "refresh-NEW".to_string();
        let hits_thread = Arc::clone(&hits);
        let fail_thread = Arc::clone(&fail_token);
        let old_thread = Arc::clone(&old_access);
        let shutdown_thread = Arc::clone(&shutdown);
        let base_thread = base.clone();
        let access_thread = new_access.clone();
        let refresh_thread = new_refresh.clone();
        let thread = std::thread::spawn(move || {
            while !shutdown_thread.load(Ordering::SeqCst) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if shutdown_thread.load(Ordering::SeqCst) {
                    break;
                }
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let Some(raw) = read_http(&mut stream) else {
                    continue;
                };
                hits_thread.lock().unwrap().push(Hit { raw: raw.clone() });
                let path = raw
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("");
                let (status, body) = if path.contains("openid-configuration") {
                    (
                        200,
                        format!(r#"{{"token_endpoint":"{base_thread}/oauth/token"}}"#),
                    )
                } else if path.contains("/token") {
                    if fail_thread.load(Ordering::SeqCst) {
                        (401, r#"{"error":"invalid_grant"}"#.to_string())
                    } else {
                        (
                            200,
                            format!(
                                r#"{{"access_token":"{access_thread}","refresh_token":"{refresh_thread}"}}"#
                            ),
                        )
                    }
                } else if old_thread
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|token| raw.contains(token))
                {
                    (401, r#"{"error":"unauthorized"}"#.to_string())
                } else {
                    (200, quota_body(path))
                };
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            base,
            hits,
            fail_token,
            old_access,
            new_access,
            new_refresh,
            shutdown,
            thread: Some(thread),
        }
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.base.trim_start_matches("http://"));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_http(stream: &mut TcpStream) -> Option<String> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 2048];
    let (head, mut body) = loop {
        let read = stream.read(&mut chunk).unwrap_or(0);
        if read == 0 && raw.is_empty() {
            return None;
        }
        raw.extend_from_slice(&chunk[..read]);
        if let Some(at) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&raw[..at]).into_owned();
            break (head, raw[at + 4..].to_vec());
        }
        if read == 0 {
            return None;
        }
    };
    let length = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    while body.len() < length {
        let read = stream.read(&mut chunk).unwrap_or(0);
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    let mut text = head;
    text.push_str("\r\n\r\n");
    text.push_str(&String::from_utf8_lossy(&body));
    Some(text)
}

fn quota_body(path: &str) -> String {
    if path.contains("/api/oauth/usage") {
        return r#"{"five_hour":{"utilization":50}}"#.to_string();
    }
    if path.contains("/backend-api/wham/usage") {
        return r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":50,"limit_window_seconds":18000,"reset_at":1893456000}}}"#.to_string();
    }
    if path.contains("/api/usage-summary") {
        return r#"{"membershipType":"pro","individualUsage":{"plan":{"totalPercentUsed":50}}}"#
            .to_string();
    }
    if path.contains("/v1/billing") || path.contains("/v1/settings") || path.contains("/v1/user") {
        return r#"{"config":{"creditUsagePercent":50}}"#.to_string();
    }
    if path.contains("/api/billing/limits") {
        return r#"{"limits":{"standard":{"fiveHour":{"usedPercent":50}}}}"#.to_string();
    }
    if path.contains("retrieveUserQuotaSummary") {
        return r#"{"groups":[{"displayName":"Pro","buckets":[{"bucketId":"gemini","remainingFraction":0.5}]}],"paidTier":{"id":"pro"}}"#.to_string();
    }
    if path.contains("/zen/go/v1/usage") {
        return r#"{"usage":{"weekly":{"percent":50}}}"#.to_string();
    }
    if path.contains("/copilot_internal/user") {
        return r#"{"copilot_plan":"pro","quota_snapshots":{"premium_interactions":{"percent_remaining":50}}}"#.to_string();
    }
    if path.contains("GetUserStatus") {
        return r#"{"userStatus":{"planStatus":{"weeklyQuotaRemainingPercent":50,"planInfo":{"planName":"Pro"}}}}"#.to_string();
    }
    "{}".to_string()
}

fn account_target<'a>(targets: &'a [FetchTarget], id: &str) -> &'a accounts::AccountTarget {
    match targets.iter().find(|target| target.quota_id() == id) {
        Some(FetchTarget::Account(target)) => target.as_ref(),
        other => panic!("missing account target {id}: {other:?}"),
    }
}

#[test]
fn profile_dir_rejects_a_bad_path_and_keeps_the_directory_token_out_of_the_secret_file() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    seed_provider(
        OfficialQuotaProvider::Codex,
        source.path(),
        "codex-SECRET-TOKEN-9f3a",
    );
    let client = credential_file(OfficialQuotaProvider::Codex, source.path());
    let before = file_bytes(&client);
    let relative = accounts::save_profile_dir(
        &paths(app.path()),
        SaveProfileDirAccount {
            id: None,
            provider: "codex".into(),
            name: "工作号".into(),
            profile_dir: "relative/codex".into(),
        },
        &backoff(app.path()),
    );
    assert_eq!(relative.unwrap_err(), "配置目录要用绝对路径");
    let missing = source.path().join("no-such-dir");
    let missing = accounts::save_profile_dir(
        &paths(app.path()),
        SaveProfileDirAccount {
            id: None,
            provider: "codex".into(),
            name: "工作号".into(),
            profile_dir: missing.to_string_lossy().into_owned(),
        },
        &backoff(app.path()),
    );
    assert_eq!(missing.unwrap_err(), "配置目录不存在");
    let file = source.path().join("auth.json");
    let not_dir = accounts::save_profile_dir(
        &paths(app.path()),
        SaveProfileDirAccount {
            id: None,
            provider: "codex".into(),
            name: "工作号".into(),
            profile_dir: file.to_string_lossy().into_owned(),
        },
        &backoff(app.path()),
    );
    assert_eq!(not_dir.unwrap_err(), "配置目录必须是文件夹");
    let empty_name = accounts::save_profile_dir(
        &paths(app.path()),
        SaveProfileDirAccount {
            id: None,
            provider: "codex".into(),
            name: "  ".into(),
            profile_dir: source.path().to_string_lossy().into_owned(),
        },
        &backoff(app.path()),
    );
    assert_eq!(empty_name.unwrap_err(), "请填写名称");
    let empty_dir = accounts::save_profile_dir(
        &paths(app.path()),
        SaveProfileDirAccount {
            id: None,
            provider: "codex".into(),
            name: "工作号".into(),
            profile_dir: "  ".into(),
        },
        &backoff(app.path()),
    );
    assert_eq!(empty_dir.unwrap_err(), "请填写配置目录");
    let unknown = accounts::save_profile_dir(
        &paths(app.path()),
        SaveProfileDirAccount {
            id: None,
            provider: "nope".into(),
            name: "工作号".into(),
            profile_dir: source.path().to_string_lossy().into_owned(),
        },
        &backoff(app.path()),
    );
    assert_eq!(unknown.unwrap_err(), "未知的官方额度账号：nope");

    let saved = save_dir(
        app.path(),
        OfficialQuotaProvider::Codex,
        "工作号",
        source.path(),
    );
    let mut state = official_quota::backoff::BackoffState::default();
    official_quota::backoff::record_failure(
        &mut state,
        &saved.saved_id,
        "boom",
        chrono::Utc::now(),
    );
    official_quota::backoff::save_state(&backoff(app.path()), &state).unwrap();
    let again = save_dir(
        app.path(),
        OfficialQuotaProvider::Codex,
        "工作号改名",
        source.path(),
    );
    assert_eq!(again.saved_id, saved.saved_id);
    assert_eq!(again.panel.accounts.len(), 1);
    assert_eq!(again.panel.accounts[0].name, "工作号改名");
    assert_eq!(
        again.panel.accounts[0].source,
        accounts::QuotaAccountSource::ProfileDir
    );
    let canonical = std::fs::canonicalize(source.path()).unwrap();
    assert_eq!(
        again.panel.accounts[0].profile_dir.as_deref(),
        Some(canonical.to_str().unwrap())
    );
    let edited = accounts::save_profile_dir(
        &paths(app.path()),
        SaveProfileDirAccount {
            id: Some(saved.saved_id.clone()),
            provider: "claude".into(),
            name: "换提供方".into(),
            profile_dir: source.path().to_string_lossy().into_owned(),
        },
        &backoff(app.path()),
    )
    .unwrap();
    assert_eq!(edited.saved_id, saved.saved_id);
    assert_eq!(edited.panel.accounts[0].provider, "claude");
    let cleared = official_quota::backoff::load_state(&backoff(app.path()));
    assert!(!cleared.entries.contains_key(&saved.saved_id));

    let config = std::fs::read_to_string(paths(app.path()).config).unwrap();
    assert!(config.contains("profile_dir"));
    assert!(!config.contains("codex-SECRET-TOKEN-9f3a"));
    assert!(!config.contains("refresh-DIRECTORY-ONLY"));
    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap_or_default();
    assert!(!secret.contains("codex-SECRET-TOKEN-9f3a"));
    assert!(!secret.contains("refresh-DIRECTORY-ONLY"));
    let panel_json = serde_json::to_string(&edited.panel).unwrap();
    assert!(!panel_json.contains("codex-SECRET-TOKEN-9f3a"));
    assert_eq!(file_bytes(&client), before);

    let unknown_id = accounts::save_profile_dir(
        &paths(app.path()),
        SaveProfileDirAccount {
            id: Some("acct:codex:missing".into()),
            provider: "codex".into(),
            name: "工作号".into(),
            profile_dir: source.path().to_string_lossy().into_owned(),
        },
        &backoff(app.path()),
    );
    assert_eq!(
        unknown_id.unwrap_err(),
        "未知的官方额度账号：acct:codex:missing"
    );
}

#[test]
fn adding_a_profile_does_not_replace_a_local_snapshot_of_the_same_login() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    seed_provider(
        OfficialQuotaProvider::Codex,
        source.path(),
        "codex-SECRET-TOKEN-9f3a",
    );
    let imported = accounts::import_local_login(
        &paths(app.path()),
        OfficialQuotaProvider::Codex,
        source.path(),
        &backoff(app.path()),
    )
    .unwrap();
    let profile = save_dir(
        app.path(),
        OfficialQuotaProvider::Codex,
        "隔离",
        source.path(),
    );
    assert_ne!(profile.saved_id, imported.saved_id);
    assert_eq!(profile.panel.accounts.len(), 2);
    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap();
    assert!(secret.contains("codex-SECRET-TOKEN-9f3a"));
    assert!(secret.contains(&imported.saved_id));
    assert!(!secret.contains(&profile.saved_id));
}

#[test]
fn reimporting_a_profile_row_becomes_a_local_snapshot_again() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    seed_provider(
        OfficialQuotaProvider::Codex,
        source.path(),
        "codex-SECRET-TOKEN-9f3a",
    );
    let saved = save_dir(
        app.path(),
        OfficialQuotaProvider::Codex,
        "隔离",
        source.path(),
    );
    let imported = accounts::import_local_login(
        &paths(app.path()),
        OfficialQuotaProvider::Codex,
        source.path(),
        &backoff(app.path()),
    )
    .unwrap();
    assert_eq!(imported.saved_id, saved.saved_id);
    assert_eq!(imported.panel.accounts.len(), 1);
    assert_eq!(
        imported.panel.accounts[0].source,
        accounts::QuotaAccountSource::LocalSnapshot
    );
    assert!(imported.panel.accounts[0].profile_dir.is_none());
    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap();
    assert!(secret.contains("codex-SECRET-TOKEN-9f3a"));
}

#[test]
fn each_profile_dir_reads_quota_from_that_directory() {
    let stub = Stub::start();
    let cases: [(OfficialQuotaProvider, &str); 9] = [
        (
            OfficialQuotaProvider::Claude,
            "sk-ant-oat01-SUPER-SECRET-TOKEN-9f3a",
        ),
        (OfficialQuotaProvider::Codex, "codex-SECRET-TOKEN-9f3a"),
        (OfficialQuotaProvider::Cursor, "cursor-signature-SECRET"),
        (OfficialQuotaProvider::Grok, "grok-SECRET-TOKEN-9f3a"),
        (OfficialQuotaProvider::Droid, "droid-SECRET-TOKEN-9f3a"),
        (
            OfficialQuotaProvider::Antigravity,
            "ya29.antigravity-SECRET-TOKEN-9f3a",
        ),
        (OfficialQuotaProvider::OpenCode, "zen-SECRET-TOKEN-9f3a"),
        (
            OfficialQuotaProvider::Copilot,
            "gho_COPILOT-SECRET-TOKEN-9f3a",
        ),
        (OfficialQuotaProvider::Devin, "devin-SECRET-TOKEN-9f3a"),
    ];
    for (provider, marker) in cases {
        let app = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        seed_provider(provider, source.path(), marker);
        let client = credential_file(provider, source.path());
        let before = file_bytes(&client);
        let saved = save_dir(app.path(), provider, "隔离号", source.path());
        let accounts = accounts::load_resolved(&paths(app.path()));
        let targets = official_quota::targets_for_fetch(&[], &accounts, &[]);
        let target = account_target(&targets, &saved.saved_id);
        assert_eq!(
            target.live_root.as_deref(),
            Some(std::fs::canonicalize(source.path()).unwrap().as_path())
        );
        assert!(target.refresh.is_none());
        stub.hits.lock().unwrap().clear();
        let snapshot = accounts::fetch_managed_account(target, Some(&stub.base))
            .unwrap_or_else(|error| panic!("{} fetch failed: {error}", provider.as_str()));
        assert!(
            snapshot
                .windows
                .iter()
                .any(|window| window.used_percent == Some(50.0)),
            "{} windows: {:?}",
            provider.as_str(),
            snapshot.windows
        );
        let hits = stub.hits.lock().unwrap();
        assert!(
            hits.iter().any(|hit| hit.raw.contains(marker)),
            "{} request did not carry the directory credential",
            provider.as_str()
        );
        assert!(
            hits.iter().all(|hit| !hit.raw.contains("/token") && !hit.raw.contains("openid-configuration")),
            "{}",
            provider.as_str()
        );
        drop(hits);
        let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap_or_default();
        assert!(!secret.contains(marker), "{}", provider.as_str());
        assert_eq!(file_bytes(&client), before, "{}", provider.as_str());
        let dto = official_quota::load_dto_with_accounts(
            &store::open_memory().unwrap(),
            &OfficialQuotaConfig::default(),
            &[],
            &accounts,
            &[],
            chrono::Utc::now(),
        );
        let row = dto
            .rows
            .iter()
            .find(|row| row.provider == saved.saved_id)
            .unwrap();
        assert_eq!(row.base_provider.as_deref(), Some(provider.as_str()));
        assert!(row.todo.is_none());
        assert!(!row.is_active_local);
        if provider == OfficialQuotaProvider::Claude {
            accounts::save(
                &paths(app.path()),
                accounts::SaveQuotaAccount {
                    id: saved.saved_id.clone(),
                    name: None,
                    enabled: Some(false),
                },
                &backoff(app.path()),
            )
            .unwrap();
            let accounts = accounts::load_resolved(&paths(app.path()));
            let targets = official_quota::targets_for_fetch(&[], &accounts, &[]);
            assert!(targets
                .iter()
                .all(|target| target.quota_id() != saved.saved_id));
        }
    }
}

#[test]
fn active_local_profile_still_reads_the_live_login_and_does_not_refresh() {
    let stub = Stub::start();
    let app = tempfile::tempdir().unwrap();
    let profile = tempfile::tempdir().unwrap();
    let live = tempfile::tempdir().unwrap();
    write_file(
        profile.path(),
        "auth.json",
        r#"{"tokens":{"access_token":"profile-ACCESS","account_id":"acct_same","refresh_token":"refresh-DIRECTORY-ONLY"}}"#,
    );
    write_file(
        live.path(),
        "auth.json",
        r#"{"tokens":{"access_token":"live-ACCESS","account_id":"acct_same","refresh_token":"refresh-LIVE"}}"#,
    );
    let saved = save_dir(
        app.path(),
        OfficialQuotaProvider::Codex,
        "隔离",
        profile.path(),
    );
    let accounts = accounts::load_resolved(&paths(app.path()));
    let login = accounts::LocalLogin {
        provider: OfficialQuotaProvider::Codex,
        fingerprint: "acct_same".into(),
        root: live.path().to_path_buf(),
    };
    let targets = official_quota::targets_for_fetch(&[], &accounts, &[login]);
    let target = account_target(&targets, &saved.saved_id);
    assert_eq!(target.live_root.as_deref(), Some(live.path()));
    assert!(target.refresh.is_none());
    let before = file_bytes(&credential_file(
        OfficialQuotaProvider::Codex,
        profile.path(),
    ));
    accounts::fetch_managed_account(target, Some(&stub.base)).unwrap();
    let hits = stub.hits.lock().unwrap();
    assert!(hits.iter().any(|hit| hit.raw.contains("live-ACCESS")));
    assert!(hits.iter().all(|hit| !hit.raw.contains("profile-ACCESS")));
    assert!(hits.iter().all(|hit| !hit.raw.contains("/token")));
    drop(hits);
    assert_eq!(
        file_bytes(&credential_file(
            OfficialQuotaProvider::Codex,
            profile.path()
        )),
        before
    );
}

fn patch_secret(app: &Path, id: &str, updates: serde_json::Value) {
    let path = paths(app).secrets;
    let mut root: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let entry = root
        .get_mut("secrets")
        .and_then(|secrets| secrets.get_mut(id))
        .and_then(|entry| entry.as_object_mut())
        .unwrap();
    for (key, value) in updates.as_object().unwrap() {
        entry.insert(key.clone(), value.clone());
    }
    std::fs::write(&path, serde_json::to_string_pretty(&root).unwrap()).unwrap();
}

fn import_codex(app: &Path, source: &Path, token: &str, refresh: &str) -> String {
    write_file(
        source,
        "auth.json",
        &format!(
            r#"{{"tokens":{{"access_token":"{token}","account_id":"acct_01HZwork","refresh_token":"{refresh}"}}}}"#
        ),
    );
    accounts::import_local_login(
        &paths(app),
        OfficialQuotaProvider::Codex,
        source,
        &backoff(app),
    )
    .unwrap()
    .saved_id
}

#[test]
fn separated_refresh_writes_only_the_app_secret_file() {
    let stub = Stub::start();
    let app = tempfile::tempdir().unwrap();
    let codex_dir = tempfile::tempdir().unwrap();
    let grok_dir = tempfile::tempdir().unwrap();
    let anti_dir = tempfile::tempdir().unwrap();
    let codex_id = import_codex(
        app.path(),
        codex_dir.path(),
        "codex-OLD-ACCESS",
        "refresh-CODEX-OLD",
    );
    write_file(
        grok_dir.path(),
        "auth.json",
        &format!(
            r#"{{"https://auth.x.ai::openid":{{"key":"grok-OLD-ACCESS","auth_mode":"oidc","user_id":"user-stable-1","expires_at":"2020-01-01T00:00:00Z","refresh_token":"refresh-GROK-OLD","oidc_issuer":"{}","oidc_client_id":"client-1"}}}}"#,
            stub.base
        ),
    );
    let grok_id = accounts::import_local_login(
        &paths(app.path()),
        OfficialQuotaProvider::Grok,
        grok_dir.path(),
        &backoff(app.path()),
    )
    .unwrap()
    .saved_id;
    seed_provider(
        OfficialQuotaProvider::Antigravity,
        anti_dir.path(),
        "anti-OLD-ACCESS",
    );
    let anti_id = accounts::import_local_login(
        &paths(app.path()),
        OfficialQuotaProvider::Antigravity,
        anti_dir.path(),
        &backoff(app.path()),
    )
    .unwrap()
    .saved_id;
    patch_secret(
        app.path(),
        &codex_id,
        serde_json::json!({ "refresh_issuer": stub.base }),
    );
    patch_secret(
        app.path(),
        &anti_id,
        serde_json::json!({
            "refresh_token": "refresh-ANTI-OLD",
            "refresh_issuer": stub.base,
            "refresh_client_id": "anti-client",
            "refresh_client_secret": "anti-secret"
        }),
    );
    stub.old_access.lock().unwrap().extend([
        "codex-OLD-ACCESS".to_string(),
        "grok-OLD-ACCESS".to_string(),
        "anti-OLD-ACCESS".to_string(),
    ]);
    let codex_before = file_bytes(&credential_file(
        OfficialQuotaProvider::Codex,
        codex_dir.path(),
    ));
    let grok_before = file_bytes(&credential_file(
        OfficialQuotaProvider::Grok,
        grok_dir.path(),
    ));
    let anti_before = file_bytes(&credential_file(
        OfficialQuotaProvider::Antigravity,
        anti_dir.path(),
    ));
    let accounts = accounts::load_resolved(&paths(app.path()));
    for id in [&codex_id, &grok_id, &anti_id] {
        let targets = official_quota::targets_for_fetch(&[], &accounts, &[]);
        let target = account_target(&targets, id).clone();
        assert!(target.live_root.is_none(), "{id}");
        assert!(target.refresh.is_some(), "{id}");
        let snapshot = accounts::fetch_managed_account(&target, Some(&stub.base))
            .unwrap_or_else(|error| panic!("{id} refresh failed: {error}"));
        assert!(snapshot
            .windows
            .iter()
            .any(|window| window.used_percent == Some(50.0)));
    }
    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap();
    assert!(secret.contains(&stub.new_access));
    assert!(secret.contains(&stub.new_refresh));
    assert!(!secret.contains("refresh-CODEX-OLD"));
    assert!(!secret.contains("refresh-GROK-OLD"));
    assert!(!secret.contains("refresh-ANTI-OLD"));
    assert!(!secret.contains("codex-OLD-ACCESS"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(paths(app.path()).secrets)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    assert_eq!(
        file_bytes(&credential_file(
            OfficialQuotaProvider::Codex,
            codex_dir.path()
        )),
        codex_before
    );
    assert_eq!(
        file_bytes(&credential_file(
            OfficialQuotaProvider::Grok,
            grok_dir.path()
        )),
        grok_before
    );
    assert_eq!(
        file_bytes(&credential_file(
            OfficialQuotaProvider::Antigravity,
            anti_dir.path()
        )),
        anti_before
    );
    assert!(
        !codex_before
            .windows(stub.new_access.len())
            .any(|window| window == stub.new_access.as_bytes()),
        "client file picked up the refreshed token"
    );
}

#[test]
fn rejected_refresh_keeps_the_previous_windows_and_records_backoff() {
    let stub = Stub::start();
    stub.fail_token.store(true, Ordering::SeqCst);
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let id = import_codex(
        app.path(),
        source.path(),
        "codex-OLD-ACCESS",
        "refresh-CODEX-OLD",
    );
    patch_secret(
        app.path(),
        &id,
        serde_json::json!({
            "refresh_issuer": stub.base,
            "access_expires_at": "2020-01-01T00:00:00Z"
        }),
    );
    let client = credential_file(OfficialQuotaProvider::Codex, source.path());
    let before = file_bytes(&client);
    let accounts = accounts::load_resolved(&paths(app.path()));
    let targets = official_quota::targets_for_fetch(&[], &accounts, &[]);
    let target = account_target(&targets, &id);
    let wrapped = FetchTarget::Account(Box::new(target.clone()));
    let attempted = match fetch::fetch_target_throttled_at(
        &wrapped,
        &backoff(app.path()),
        chrono::Utc::now(),
    ) {
        ThrottledFetch::Attempted(result) => result,
        ThrottledFetch::Cooldown(message) => panic!("{message}"),
    };
    assert_eq!(attempted.as_ref().unwrap_err(), LOGIN_EXPIRED);
    let conn = store::open_memory().unwrap();
    store::upsert_official_quota(
        &conn,
        &id,
        &[OfficialQuotaWindow {
            kind: "weekly".into(),
            label: "7 天".into(),
            used_percent: Some(40.0),
            ..Default::default()
        }],
        "2026-08-18T12:00:00+00:00",
        None,
        Some("Plus"),
    )
    .unwrap();
    official_quota::apply_fetch_results(&conn, [(id.clone(), attempted)]).unwrap();
    let dto = official_quota::load_dto_with_accounts(
        &conn,
        &OfficialQuotaConfig::default(),
        &[],
        &accounts,
        &[],
        chrono::Utc::now(),
    );
    let row = dto.rows.iter().find(|row| row.provider == id).unwrap();
    assert_eq!(row.error.as_deref(), Some(LOGIN_EXPIRED));
    assert_eq!(row.windows[0].used_percent, Some(40.0));
    assert_eq!(row.plan.as_deref(), Some("Plus"));
    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap();
    assert!(secret.contains("refresh-CODEX-OLD"));
    assert!(secret.contains("codex-OLD-ACCESS"));
    assert!(!secret.contains("refresh-NEW"));
    assert_eq!(file_bytes(&client), before);
    let state = official_quota::backoff::load_state(&backoff(app.path()));
    assert!(state.entries.contains_key(&id));
    let hits = stub.hits.lock().unwrap();
    assert!(hits.iter().any(|hit| hit.raw.contains("/oauth/token")));
    assert!(hits
        .iter()
        .all(|hit| !hit.raw.contains("/backend-api/wham/usage")));
}

#[test]
fn quota_auth_failure_then_rejected_refresh_reports_login_expired() {
    let stub = Stub::start();
    stub.fail_token.store(true, Ordering::SeqCst);
    stub.old_access
        .lock()
        .unwrap()
        .push("codex-OLD-ACCESS".to_string());
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let id = import_codex(
        app.path(),
        source.path(),
        "codex-OLD-ACCESS",
        "refresh-CODEX-OLD",
    );
    patch_secret(
        app.path(),
        &id,
        serde_json::json!({ "refresh_issuer": stub.base }),
    );
    let before = file_bytes(&credential_file(
        OfficialQuotaProvider::Codex,
        source.path(),
    ));
    let accounts = accounts::load_resolved(&paths(app.path()));
    let targets = official_quota::targets_for_fetch(&[], &accounts, &[]);
    let target = account_target(&targets, &id).clone();
    let error = accounts::fetch_managed_account(&target, Some(&stub.base)).unwrap_err();
    assert_eq!(error, LOGIN_EXPIRED);
    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap();
    assert!(secret.contains("refresh-CODEX-OLD"));
    assert_eq!(
        file_bytes(&credential_file(
            OfficialQuotaProvider::Codex,
            source.path()
        )),
        before
    );
    let hits = stub.hits.lock().unwrap();
    assert!(hits
        .iter()
        .any(|hit| hit.raw.contains("/backend-api/wham/usage")));
    assert!(hits.iter().any(|hit| hit.raw.contains("/oauth/token")));
}
