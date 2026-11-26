//! 导入当前本机登录：掩码、防重、快照合流、当前本机不花 refresh token。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use base64::Engine;

use crate::domain::{OfficialQuotaConfig, OfficialQuotaProvider, OfficialQuotaWindow};
use crate::official_quota::accounts::{self, QuotaAccountPaths, QuotaSnapshotInputs};
use crate::official_quota::{self, FetchTarget, QuotaTarget};
use crate::store;

const CLAUDE_SECRET: &str = "sk-ant-oat01-SUPER-SECRET-TOKEN-9f3a";
const MASKED_TAIL: &str = "••••••9f3a";

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

fn import(
    dir: &Path,
    provider: OfficialQuotaProvider,
    root: &Path,
) -> accounts::SavedQuotaAccountDto {
    accounts::import_local_login(
        &paths(dir),
        provider,
        root,
        &dir.join("official_quota_backoff.json"),
    )
    .unwrap_or_else(|error| panic!("{} import failed: {error}", provider.as_str()))
}

fn claude_root(dir: &Path) -> PathBuf {
    write_file(
        dir,
        ".credentials.json",
        &format!(r#"{{"claudeAiOauth":{{"accessToken":"{CLAUDE_SECRET}","expiresAt":0}}}}"#),
    );
    dir.to_path_buf()
}

#[test]
fn import_local_login_stores_masked_hint_and_owner_only_secret() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    claude_root(source.path());
    let cred = source.path().join(".credentials.json");
    let before = std::fs::read(&cred).unwrap();
    let saved = import(app.path(), OfficialQuotaProvider::Claude, source.path());
    assert_eq!(std::fs::read(&cred).unwrap(), before);

    let id = &saved.saved_id;
    let hex = id.strip_prefix("acct:claude:").expect(id);
    assert_eq!(hex.len(), 16);
    assert!(hex
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));

    assert_eq!(saved.panel.accounts.len(), 1);
    assert_eq!(saved.panel.accounts[0].hint, MASKED_TAIL);
    assert_eq!(saved.panel.accounts[0].name, MASKED_TAIL);
    assert_eq!(saved.panel.accounts[0].provider, "claude");
    assert!(saved.panel.accounts[0].enabled);
    let panel_json = serde_json::to_string(&saved.panel).unwrap();
    assert!(!panel_json.contains(CLAUDE_SECRET));
    assert!(!panel_json.contains("fingerprint"));

    let stored = paths(app.path());
    let config = std::fs::read_to_string(&stored.config).unwrap();
    assert!(!config.contains(CLAUDE_SECRET));
    assert!(config.contains("local_snapshot"));
    assert!(config.contains(MASKED_TAIL));
    let secret = std::fs::read_to_string(&stored.secrets).unwrap();
    assert!(secret.contains(CLAUDE_SECRET));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&stored.secrets)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn reimporting_the_same_fingerprint_keeps_the_id_and_replaces_the_secret() {
    let app = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let body = |token: &str| {
        format!(
            r#"{{"tokens":{{"access_token":"{token}","account_id":"acct_01HZwork","refresh_token":"refresh-old"}}}}"#
        )
    };
    write_file(first.path(), "auth.json", &body("codex-SECRET-TOKEN-old"));
    write_file(second.path(), "auth.json", &body("codex-SECRET-TOKEN-new"));
    let saved = import(app.path(), OfficialQuotaProvider::Codex, first.path());
    accounts::save(
        &paths(app.path()),
        accounts::SaveQuotaAccount {
            id: saved.saved_id.clone(),
            name: Some("工作号".into()),
            enabled: None,
        },
        &app.path().join("official_quota_backoff.json"),
    )
    .unwrap();
    let again = import(app.path(), OfficialQuotaProvider::Codex, second.path());
    assert_eq!(again.saved_id, saved.saved_id);
    assert_eq!(again.panel.accounts.len(), 1);
    assert_eq!(again.panel.accounts[0].name, "工作号");
    assert_eq!(again.panel.accounts[0].hint, "acct_01HZwork");
    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap();
    assert!(secret.contains("codex-SECRET-TOKEN-new"));
    assert!(!secret.contains("codex-SECRET-TOKEN-old"));
    let panel_json = serde_json::to_string(&again.panel).unwrap();
    assert!(!panel_json.contains("codex-SECRET-TOKEN-new"));
}

#[test]
fn each_builtin_provider_can_import_its_local_login() {
    let cases: [(OfficialQuotaProvider, &str); 9] = [
        (OfficialQuotaProvider::Claude, CLAUDE_SECRET),
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
        let identity = official_quota::read_identity(provider, source.path()).unwrap();
        let saved = import(app.path(), provider, source.path());
        assert!(
            saved
                .saved_id
                .starts_with(&format!("acct:{}:", provider.as_str())),
            "{}",
            saved.saved_id
        );
        assert_eq!(saved.panel.accounts[0].hint, identity.label);
        let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap();
        assert!(secret.contains(marker), "{}", provider.as_str());
        let panel_json = serde_json::to_string(&saved.panel).unwrap();
        assert!(!panel_json.contains(marker), "{}", provider.as_str());
    }
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
                    r#"{{"tokens":{{"access_token":"{marker}","account_id":"acct_01HZwork"}}}}"#
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

fn codex_login(root: &Path, account_id: &str, token: &str) {
    write_file(
        root,
        "auth.json",
        &format!(r#"{{"tokens":{{"access_token":"{token}","account_id":"{account_id}"}}}}"#),
    );
}

fn codex_rows(dto: &crate::domain::OfficialQuotaDto) -> Vec<&crate::domain::OfficialQuotaRow> {
    dto.rows
        .iter()
        .filter(|row| {
            row.provider == "codex"
                || row.base_provider.as_deref() == Some("codex")
                || row.provider.starts_with("acct:codex:")
        })
        .collect()
}

#[test]
fn enabled_local_login_replaces_the_builtin_row() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    codex_login(source.path(), "acct_01HZwork", "codex-SECRET-TOKEN-9f3a");
    let saved = import(app.path(), OfficialQuotaProvider::Codex, source.path());
    let accounts = accounts::load_resolved(&paths(app.path()));
    let login = accounts::LocalLogin {
        provider: OfficialQuotaProvider::Codex,
        fingerprint: "acct_01HZwork".into(),
        root: source.path().to_path_buf(),
    };
    let dto = official_quota::load_dto_with_accounts(
        &store::open_memory().unwrap(),
        &OfficialQuotaConfig::default(),
        &[],
        &accounts,
        &[login],
        chrono::Utc::now(),
    );
    let rows = codex_rows(&dto);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider, saved.saved_id);
    assert_eq!(rows[0].base_provider.as_deref(), Some("codex"));
    assert_eq!(rows[0].account_label.as_deref(), Some("acct_01HZwork"));
    assert!(rows[0].is_active_local);
    assert_eq!(rows[0].application, "Codex · acct_01HZwork · 当前本机");
    assert!(!dto.undetected.iter().any(|id| id == "codex"));
}

#[test]
fn disabled_local_login_suppresses_the_builtin_row_and_cannot_alert() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    codex_login(source.path(), "acct_01HZwork", "codex-SECRET-TOKEN-9f3a");
    let saved = import(app.path(), OfficialQuotaProvider::Codex, source.path());
    accounts::save(
        &paths(app.path()),
        accounts::SaveQuotaAccount {
            id: saved.saved_id.clone(),
            name: None,
            enabled: Some(false),
        },
        &app.path().join("official_quota_backoff.json"),
    )
    .unwrap();
    let conn = store::open_memory().unwrap();
    store::upsert_official_quota(
        &conn,
        &saved.saved_id,
        &[OfficialQuotaWindow {
            kind: "weekly".into(),
            label: "7 天".into(),
            used_percent: Some(90.0),
            resets_at: Some("2026-12-01T00:00:00+00:00".into()),
            ..Default::default()
        }],
        "2026-08-18T12:00:00+00:00",
        None,
        None,
    )
    .unwrap();
    let accounts = accounts::load_resolved(&paths(app.path()));
    let login = accounts::LocalLogin {
        provider: OfficialQuotaProvider::Codex,
        fingerprint: "acct_01HZwork".into(),
        root: source.path().to_path_buf(),
    };
    let dto = official_quota::load_dto_with_accounts(
        &conn,
        &OfficialQuotaConfig::default(),
        &[],
        &accounts,
        std::slice::from_ref(&login),
        chrono::Utc::now(),
    );
    assert!(codex_rows(&dto).is_empty());
    assert!(!dto.undetected.iter().any(|id| id == "codex"));
    let (_, alerts) = official_quota::notify::prepare_notifications(
        official_quota::notify::NotifyState::default(),
        &dto,
    );
    assert!(alerts.iter().all(|alert| alert.provider != saved.saved_id));
    let targets = official_quota::targets_for_fetch(&[], &accounts, std::slice::from_ref(&login));
    assert!(targets
        .iter()
        .all(|target| target.quota_id() != saved.saved_id));
    assert!(targets
        .iter()
        .all(|target| !matches!(target, FetchTarget::Builtin(OfficialQuotaProvider::Codex))));
}

#[test]
fn unmatched_local_login_keeps_a_marked_builtin_beside_enabled_accounts() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    codex_login(source.path(), "acct_01HZwork", "codex-SECRET-TOKEN-9f3a");
    let saved = import(app.path(), OfficialQuotaProvider::Codex, source.path());
    let accounts = accounts::load_resolved(&paths(app.path()));
    let login = accounts::LocalLogin {
        provider: OfficialQuotaProvider::Codex,
        fingerprint: "someone-else".into(),
        root: source.path().to_path_buf(),
    };
    let dto = official_quota::load_dto_with_accounts(
        &store::open_memory().unwrap(),
        &OfficialQuotaConfig::default(),
        &[],
        &accounts,
        &[login],
        chrono::Utc::now(),
    );
    let rows = codex_rows(&dto);
    assert_eq!(rows.len(), 2);
    let builtin = rows.iter().find(|row| row.provider == "codex").unwrap();
    assert!(builtin.is_active_local);
    assert!(builtin.account_label.is_none());
    assert!(builtin.base_provider.is_none());
    assert_eq!(builtin.application, "Codex · 当前本机");
    let account = rows
        .iter()
        .find(|row| row.provider == saved.saved_id)
        .unwrap();
    assert!(!account.is_active_local);
    assert_eq!(account.application, "Codex · acct_01HZwork");
    assert_eq!(account.base_provider.as_deref(), Some("codex"));
}

#[test]
fn rename_keeps_the_id_and_the_cached_windows() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    codex_login(source.path(), "acct_01HZwork", "codex-SECRET-TOKEN-9f3a");
    let saved = import(app.path(), OfficialQuotaProvider::Codex, source.path());
    let conn = store::open_memory().unwrap();
    store::upsert_official_quota(
        &conn,
        &saved.saved_id,
        &[OfficialQuotaWindow {
            kind: "weekly".into(),
            label: "7 天".into(),
            used_percent: Some(40.0),
            ..Default::default()
        }],
        "2026-08-18T12:00:00+00:00",
        None,
        None,
    )
    .unwrap();
    let renamed = accounts::save(
        &paths(app.path()),
        accounts::SaveQuotaAccount {
            id: saved.saved_id.clone(),
            name: Some("工作号".into()),
            enabled: None,
        },
        &app.path().join("official_quota_backoff.json"),
    )
    .unwrap();
    assert_eq!(renamed.saved_id, saved.saved_id);
    assert_eq!(renamed.panel.accounts[0].name, "工作号");
    let accounts = accounts::load_resolved(&paths(app.path()));
    let dto = official_quota::load_dto_with_accounts(
        &conn,
        &OfficialQuotaConfig::default(),
        &[],
        &accounts,
        &[],
        chrono::Utc::now(),
    );
    let row = codex_rows(&dto)
        .into_iter()
        .find(|row| row.provider == saved.saved_id)
        .unwrap();
    assert_eq!(row.account_label.as_deref(), Some("工作号"));
    assert_eq!(row.application, "Codex · 工作号");
    assert!(!row.is_active_local);
    assert_eq!(row.windows.len(), 1);
    assert_eq!(row.windows[0].used_percent, Some(40.0));
}

#[test]
fn delete_removes_the_account_and_its_secret() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    claude_root(source.path());
    let saved = import(app.path(), OfficialQuotaProvider::Claude, source.path());
    let panel = accounts::delete_account(&paths(app.path()), &saved.saved_id).unwrap();
    assert!(panel.accounts.is_empty());
    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap_or_default();
    assert!(!secret.contains(CLAUDE_SECRET));
    let config = std::fs::read_to_string(paths(app.path()).config).unwrap();
    assert!(!config.contains(&saved.saved_id));
}

#[test]
fn save_and_reimport_clear_backoff_for_that_id() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    codex_login(source.path(), "acct_01HZwork", "codex-SECRET-TOKEN-9f3a");
    let backoff_path = app.path().join("official_quota_backoff.json");
    let saved = import(app.path(), OfficialQuotaProvider::Codex, source.path());
    let mut state = official_quota::backoff::BackoffState::default();
    official_quota::backoff::record_failure(
        &mut state,
        &saved.saved_id,
        "boom",
        chrono::Utc::now(),
    );
    official_quota::backoff::save_state(&backoff_path, &state).unwrap();
    import(app.path(), OfficialQuotaProvider::Codex, source.path());
    let cleared = official_quota::backoff::load_state(&backoff_path);
    assert!(!cleared.entries.contains_key(&saved.saved_id));

    official_quota::backoff::record_failure(
        &mut state,
        &saved.saved_id,
        "boom",
        chrono::Utc::now(),
    );
    official_quota::backoff::save_state(&backoff_path, &state).unwrap();
    accounts::save(
        &paths(app.path()),
        accounts::SaveQuotaAccount {
            id: saved.saved_id.clone(),
            name: Some("工作号".into()),
            enabled: Some(true),
        },
        &backoff_path,
    )
    .unwrap();
    let cleared = official_quota::backoff::load_state(&backoff_path);
    assert!(!cleared.entries.contains_key(&saved.saved_id));
    let empty = accounts::save(
        &paths(app.path()),
        accounts::SaveQuotaAccount {
            id: saved.saved_id.clone(),
            name: Some("  ".into()),
            enabled: None,
        },
        &backoff_path,
    );
    assert_eq!(empty.unwrap_err(), "请填写名称");
}

#[test]
fn missing_secret_shows_a_todo_and_is_not_fetched() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    codex_login(source.path(), "acct_01HZwork", "codex-SECRET-TOKEN-9f3a");
    let saved = import(app.path(), OfficialQuotaProvider::Codex, source.path());
    std::fs::remove_file(paths(app.path()).secrets).unwrap();
    let accounts = accounts::load_resolved(&paths(app.path()));
    let dto = official_quota::load_dto_with_accounts(
        &store::open_memory().unwrap(),
        &OfficialQuotaConfig::default(),
        &[],
        &accounts,
        &[],
        chrono::Utc::now(),
    );
    let row = codex_rows(&dto)
        .into_iter()
        .find(|row| row.provider == saved.saved_id)
        .unwrap();
    assert_eq!(
        row.todo.as_deref(),
        Some("登录凭证还没保存，请在设置页重新导入")
    );
    let targets = official_quota::targets_for_fetch(&[], &accounts, &[]);
    assert!(targets
        .iter()
        .all(|target| target.quota_id() != saved.saved_id));
}

#[test]
fn cli_loader_includes_enabled_accounts_with_the_same_row_fields() {
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    codex_login(source.path(), "acct_01HZwork", "codex-SECRET-TOKEN-9f3a");
    let saved = import(app.path(), OfficialQuotaProvider::Codex, source.path());
    let login = accounts::LocalLogin {
        provider: OfficialQuotaProvider::Codex,
        fingerprint: "acct_01HZwork".into(),
        root: source.path().to_path_buf(),
    };
    let inputs = QuotaSnapshotInputs::from_stored(app.path(), vec![login.clone()]);
    let dto = official_quota::load_dto_with_accounts(
        &store::open_memory().unwrap(),
        &OfficialQuotaConfig::default(),
        &inputs.custom,
        &inputs.accounts,
        &inputs.logins,
        chrono::Utc::now(),
    );
    let row = dto
        .rows
        .iter()
        .find(|row| row.provider == saved.saved_id)
        .unwrap();
    assert_eq!(row.base_provider.as_deref(), Some("codex"));
    assert_eq!(row.account_label.as_deref(), Some("acct_01HZwork"));
    assert!(row.is_active_local);
    let targets =
        official_quota::targets_for_fetch(&inputs.custom, &inputs.accounts, &inputs.logins);
    let target = targets
        .iter()
        .find(|target| target.quota_id() == saved.saved_id)
        .unwrap();
    match target {
        FetchTarget::Account(account) => assert!(account.live_root.is_some()),
        other => panic!("expected the stored account, got {other:?}"),
    }
}

struct Hit {
    path: String,
    authorization: String,
}

struct Stub {
    base: String,
    hits: Arc<Mutex<Vec<Hit>>>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Stub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let hits_thread = Arc::clone(&hits);
        let shutdown_thread = Arc::clone(&shutdown);
        let thread = std::thread::spawn(move || {
            let _ = listener.set_nonblocking(false);
            while !shutdown_thread.load(Ordering::SeqCst) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if shutdown_thread.load(Ordering::SeqCst) {
                    break;
                }
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 1024];
                loop {
                    match stream.read(&mut tmp) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            if buf.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let text = String::from_utf8_lossy(&buf);
                let mut lines = text.lines();
                if let Some(start) = lines.next() {
                    let mut parts = start.split_whitespace();
                    let _method = parts.next().unwrap_or("");
                    let path = parts.next().unwrap_or("").to_string();
                    if !path.is_empty() {
                        let authorization = text
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("authorization")
                                    .then(|| value.trim().to_string())
                            })
                            .unwrap_or_default();
                        hits_thread.lock().unwrap().push(Hit {
                            path,
                            authorization,
                        });
                    }
                }
                let body = r#"{"config":{"creditUsagePercent":12}}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            base: format!("http://{addr}"),
            hits,
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

fn grok_auth(dir: &Path, token: &str, expires: &str, refresh: Option<(&str, &str)>) {
    let refresh_fields = match refresh {
        Some((refresh_token, issuer)) => format!(
            r#","refresh_token":"{refresh_token}","oidc_issuer":"{issuer}","oidc_client_id":"client-1""#
        ),
        None => String::new(),
    };
    write_file(
        dir,
        "auth.json",
        &format!(
            r#"{{"https://auth.x.ai::openid":{{"key":"{token}","auth_mode":"oidc","user_id":"user-stable-1","expires_at":"{expires}"{refresh_fields}}}}}"#
        ),
    );
}

#[test]
fn active_local_grok_fetch_uses_the_live_access_token_and_skips_refresh() {
    let stub = Stub::start();
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    grok_auth(
        source.path(),
        "stored-access-TOKEN",
        "2020-01-01T00:00:00Z",
        Some(("refresh-SHOULD-NOT-USE", &stub.base)),
    );
    let saved = import(app.path(), OfficialQuotaProvider::Grok, source.path());
    grok_auth(
        source.path(),
        "live-access-TOKEN",
        "2099-01-01T00:00:00Z",
        None,
    );
    let accounts = accounts::load_resolved(&paths(app.path()));
    let identity =
        official_quota::read_identity(OfficialQuotaProvider::Grok, source.path()).unwrap();
    let login = accounts::LocalLogin {
        provider: OfficialQuotaProvider::Grok,
        fingerprint: identity.fingerprint,
        root: source.path().to_path_buf(),
    };
    let targets = official_quota::targets_for_fetch(&[], &accounts, &[login]);
    let FetchTarget::Account(target) = targets
        .into_iter()
        .find(|target| target.quota_id() == saved.saved_id)
        .unwrap()
    else {
        panic!("active local account was not a fetch target");
    };
    assert!(target.live_root.is_some());
    official_quota::accounts::fetch_account_row(
        target.provider,
        &target.access_token,
        target.account_id.as_deref(),
        target.live_root.as_deref(),
        Some(&stub.base),
    )
    .unwrap();
    let hits = stub.hits.lock().unwrap();
    assert!(hits
        .iter()
        .any(|hit| hit.authorization == "Bearer live-access-TOKEN"));
    assert!(hits
        .iter()
        .all(|hit| hit.authorization != "Bearer stored-access-TOKEN"));
    assert!(hits.iter().any(|hit| hit.path.contains("/v1/billing")));
    assert!(hits
        .iter()
        .all(|hit| !hit.path.contains("openid-configuration") && !hit.path.contains("/token")));
    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap();
    assert!(secret.contains("refresh-SHOULD-NOT-USE"));
}

#[test]
fn separated_grok_snapshot_uses_the_stored_access_token_without_refresh() {
    let stub = Stub::start();
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    grok_auth(
        source.path(),
        "stored-access-TOKEN",
        "2020-01-01T00:00:00Z",
        Some(("refresh-SHOULD-NOT-USE", &stub.base)),
    );
    let saved = import(app.path(), OfficialQuotaProvider::Grok, source.path());
    let accounts = accounts::load_resolved(&paths(app.path()));
    let targets = official_quota::targets_for_fetch(&[], &accounts, &[]);
    let FetchTarget::Account(target) = targets
        .into_iter()
        .find(|target| target.quota_id() == saved.saved_id)
        .unwrap()
    else {
        panic!("separated account was not a fetch target");
    };
    assert!(target.live_root.is_none());
    official_quota::accounts::fetch_account_row(
        target.provider,
        &target.access_token,
        target.account_id.as_deref(),
        target.live_root.as_deref(),
        Some(&stub.base),
    )
    .unwrap();
    let hits = stub.hits.lock().unwrap();
    assert!(hits
        .iter()
        .any(|hit| hit.authorization == "Bearer stored-access-TOKEN"));
    assert!(hits.iter().any(|hit| hit.path.contains("/v1/billing")));
    assert!(hits
        .iter()
        .all(|hit| !hit.path.contains("openid-configuration") && !hit.path.contains("/token")));
}
