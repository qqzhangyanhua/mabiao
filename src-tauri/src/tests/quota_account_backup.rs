//! 额度账号配置进备份，凭证不进。恢复后隔离目录仍能取数，缺凭证的是待办。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::domain::{OfficialQuotaConfig, OfficialQuotaProvider, OfficialQuotaWindow};
use crate::official_quota::accounts::{
    self, QuotaAccountPaths, SaveProfileDirAccount, SaveQuotaAccount,
};
use crate::official_quota::{self, FetchTarget, QuotaTarget};
use crate::store;
use crate::test_support::*;

const PROFILE_TOKEN: &str = "sk-ant-oat01-PROFILE-DIR-TOKEN";
const SNAPSHOT_TOKEN: &str = "codex-SNAPSHOT-SECRET-TOKEN";
const SNAPSHOT_REFRESH: &str = "refresh-SNAPSHOT-ONLY";
const OAUTH_TOKEN: &str = "gho_OAUTH-SECRET-SHOULD-STAY";

fn live_paths(live: &Path) -> backup::AppDataPaths {
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

fn account_paths(dir: &Path) -> QuotaAccountPaths {
    QuotaAccountPaths::in_dir(dir)
}

fn backoff_path(dir: &Path) -> PathBuf {
    dir.join("official_quota_backoff.json")
}

fn write_file(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

fn mark_oauth(dir: &Path, id: &str) {
    let path = account_paths(dir).config;
    let mut root: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let accounts = root
        .get_mut("accounts")
        .and_then(|value| value.as_array_mut())
        .unwrap();
    let account = accounts
        .iter_mut()
        .find(|account| account.get("id").and_then(|value| value.as_str()) == Some(id))
        .unwrap();
    account["source"] = serde_json::json!("oauth");
    std::fs::write(path, serde_json::to_string_pretty(&root).unwrap()).unwrap();
}

struct Seeded {
    profile_id: String,
    snapshot_id: String,
    oauth_id: String,
}

fn seed_three_sources(
    app: &Path,
    profile_dir: &Path,
    codex_dir: &Path,
    copilot_dir: &Path,
) -> Seeded {
    write_file(
        profile_dir,
        ".credentials.json",
        &format!(r#"{{"claudeAiOauth":{{"accessToken":"{PROFILE_TOKEN}","expiresAt":0}}}}"#),
    );
    write_file(
        codex_dir,
        "auth.json",
        &format!(
            r#"{{"tokens":{{"access_token":"{SNAPSHOT_TOKEN}","account_id":"acct_01HZwork","refresh_token":"{SNAPSHOT_REFRESH}"}}}}"#
        ),
    );
    write_file(
        copilot_dir,
        "github-copilot/apps.json",
        &format!(r#"{{"github.com:client":{{"oauth_token":"{OAUTH_TOKEN}","user":"octocat"}}}}"#),
    );
    let profile = accounts::save_profile_dir(
        &account_paths(app),
        SaveProfileDirAccount {
            id: None,
            provider: OfficialQuotaProvider::Claude.as_str().to_string(),
            name: "隔离目录".to_string(),
            profile_dir: profile_dir.to_string_lossy().into_owned(),
        },
        &backoff_path(app),
    )
    .unwrap();
    let snapshot = accounts::import_local_login(
        &account_paths(app),
        OfficialQuotaProvider::Codex,
        codex_dir,
        &backoff_path(app),
    )
    .unwrap();
    accounts::save(
        &account_paths(app),
        SaveQuotaAccount {
            id: snapshot.saved_id.clone(),
            name: Some("本机快照".into()),
            enabled: None,
        },
        &backoff_path(app),
    )
    .unwrap();
    let oauth = accounts::import_local_login(
        &account_paths(app),
        OfficialQuotaProvider::Copilot,
        copilot_dir,
        &backoff_path(app),
    )
    .unwrap();
    accounts::save(
        &account_paths(app),
        SaveQuotaAccount {
            id: oauth.saved_id.clone(),
            name: Some("应用内授权".into()),
            enabled: None,
        },
        &backoff_path(app),
    )
    .unwrap();
    mark_oauth(app, &oauth.saved_id);
    Seeded {
        profile_id: profile.saved_id,
        snapshot_id: snapshot.saved_id,
        oauth_id: oauth.saved_id,
    }
}

fn json_files_contain(dir: &Path, needle: &str) -> bool {
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(&current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains(needle) {
                return true;
            }
        }
    }
    false
}

#[test]
fn backup_keeps_quota_account_config_and_drops_the_secret_file() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    let profile = root.path().join("profile");
    let codex = root.path().join("codex");
    let copilot = root.path().join("copilot");
    std::fs::create_dir_all(&live).unwrap();
    seed_three_sources(&live, &profile, &codex, &copilot);

    let paths = live_paths(&live);
    let conn = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    let manifest = backup::backup_to(&conn, &dest, &paths).unwrap();

    assert!(
        manifest
            .files
            .iter()
            .any(|name| name == accounts::CONFIG_NAME),
        "备份清单应包含额度账号配置：{manifest:?}"
    );
    assert!(
        !manifest
            .files
            .iter()
            .any(|name| name == accounts::SECRET_NAME),
        "备份清单不得包含额度账号凭证：{manifest:?}"
    );
    assert!(dest.join(accounts::CONFIG_NAME).is_file());
    assert!(
        !dest.join(accounts::SECRET_NAME).exists(),
        "凭证文件即使就在配置旁边，也不该被复制进备份目录"
    );
    let config = std::fs::read_to_string(dest.join(accounts::CONFIG_NAME)).unwrap();
    assert!(config.contains("隔离目录"));
    assert!(config.contains("本机快照"));
    assert!(config.contains("应用内授权"));
    assert!(
        config.contains(r#""source": "profile_dir""#)
            || config.contains(r#""source":"profile_dir""#)
    );
    assert!(config.contains("local_snapshot"));
    assert!(config.contains("oauth"));
    for secret in [PROFILE_TOKEN, SNAPSHOT_TOKEN, SNAPSHOT_REFRESH, OAUTH_TOKEN] {
        assert!(
            !json_files_contain(&dest, secret),
            "备份目录的 json 不该出现 {secret}"
        );
    }
    assert!(manifest.note.contains("额度账号配置"), "{}", manifest.note);
    assert!(manifest.note.contains("密钥不进备份"), "{}", manifest.note);
}

#[test]
fn restore_writes_quota_account_config_and_leaves_the_local_secret_file_alone() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    let profile = root.path().join("profile");
    let codex = root.path().join("codex");
    let copilot = root.path().join("copilot");
    std::fs::create_dir_all(&live).unwrap();
    let seeded = seed_three_sources(&live, &profile, &codex, &copilot);
    let paths = live_paths(&live);
    let conn = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    backup::backup_to(&conn, &dest, &paths).unwrap();
    drop(conn);

    accounts::save(
        &account_paths(&live),
        SaveQuotaAccount {
            id: seeded.snapshot_id.clone(),
            name: Some("本机现有的".into()),
            enabled: None,
        },
        &backoff_path(&live),
    )
    .unwrap();
    std::fs::write(
        live.join(accounts::SECRET_NAME),
        r#"{"secrets":{"acct:codex:local":{"access_token":"sk-machine-local"}}}"#,
    )
    .unwrap();
    std::fs::write(
        dest.join(accounts::SECRET_NAME),
        r#"{"secrets":{"acct:codex:from-backup":{"access_token":"sk-from-backup"}}}"#,
    )
    .unwrap();

    backup::restore_from(&dest, &paths).unwrap();

    let restored = accounts::list(&account_paths(&live));
    let snapshot = restored
        .accounts
        .iter()
        .find(|account| account.id == seeded.snapshot_id)
        .unwrap();
    assert_eq!(snapshot.name, "本机快照");
    assert_eq!(snapshot.source, accounts::QuotaAccountSource::LocalSnapshot);
    let oauth = restored
        .accounts
        .iter()
        .find(|account| account.id == seeded.oauth_id)
        .unwrap();
    assert_eq!(oauth.source, accounts::QuotaAccountSource::Oauth);
    let secret = std::fs::read_to_string(live.join(accounts::SECRET_NAME)).unwrap();
    assert!(secret.contains("sk-machine-local"));
    assert!(!secret.contains("sk-from-backup"));
    assert!(!secret.contains(SNAPSHOT_TOKEN));
    assert!(!secret.contains(OAUTH_TOKEN));
}

#[test]
fn failed_restore_rolls_the_quota_account_config_back() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();
    let config = live.join(accounts::CONFIG_NAME);
    std::fs::write(&config, r#"{"accounts":[{"name":"备份里的"}]}"#).unwrap();
    std::fs::write(live.join("scan_paths.json"), "{}").unwrap();
    let paths = live_paths(&live);
    let conn = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    let manifest = backup::backup_to(&conn, &dest, &paths).unwrap();
    drop(conn);
    assert!(
        manifest
            .files
            .iter()
            .any(|name| name == accounts::CONFIG_NAME),
        "回滚用例也要先把额度账号配置放进清单：{manifest:?}"
    );

    std::fs::write(&config, r#"{"accounts":[{"name":"本机现有的"}]}"#).unwrap();
    let scan_paths = live.join("scan_paths.json");
    std::fs::remove_file(&scan_paths).unwrap();
    std::fs::create_dir(&scan_paths).unwrap();

    let error = backup::restore_from(&dest, &paths).unwrap_err();
    assert!(error.contains("写入") || error.contains("失败"), "{error}");
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        r#"{"accounts":[{"name":"本机现有的"}]}"#,
        "后面的文件写失败时，额度账号配置应回到恢复前"
    );
}

#[test]
fn restored_profile_dir_fetches_while_missing_secrets_stay_a_todo() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let backup_dir = root.path().join("backup");
    let restored = root.path().join("restored");
    let profile = root.path().join("profile");
    let codex = root.path().join("codex");
    let copilot = root.path().join("copilot");
    std::fs::create_dir_all(&live).unwrap();
    let seeded = seed_three_sources(&live, &profile, &codex, &copilot);

    let source_paths = live_paths(&live);
    let conn = store::open_db(source_paths.db_path.to_str().unwrap()).unwrap();
    store::upsert_official_quota(
        &conn,
        &seeded.snapshot_id,
        &[OfficialQuotaWindow {
            kind: "primary".into(),
            label: "主窗口".into(),
            used_percent: Some(22.0),
            ..Default::default()
        }],
        "2026-08-01T00:00:00Z",
        None,
        Some("plus"),
    )
    .unwrap();
    store::set_official_quota_error(&conn, &seeded.snapshot_id, accounts::LOGIN_EXPIRED).unwrap();
    store::set_official_quota_error(&conn, &seeded.oauth_id, accounts::MISSING_SECRET_TODO)
        .unwrap();
    backup::backup_to(&conn, &backup_dir, &source_paths).unwrap();
    drop(conn);

    std::fs::create_dir_all(&restored).unwrap();
    let paths = live_paths(&restored);
    backup::restore_from(&backup_dir, &paths).unwrap();
    assert!(
        !restored.join(accounts::SECRET_NAME).exists(),
        "备份里没有凭证，恢复不该造出一份"
    );

    let accounts = accounts::load_resolved(&account_paths(&restored));
    let conn = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    let dto = official_quota::load_dto_with_accounts(
        &conn,
        &OfficialQuotaConfig::default(),
        &[],
        &accounts,
        &[],
        chrono::Utc::now(),
    );
    let snapshot = row(&dto, &seeded.snapshot_id);
    let oauth = row(&dto, &seeded.oauth_id);
    let profile_row = row(&dto, &seeded.profile_id);
    assert_eq!(
        snapshot.todo.as_deref(),
        Some(accounts::MISSING_SECRET_TODO)
    );
    assert_eq!(snapshot.error, None, "残留的登录失效不该画成失败");
    assert_eq!(snapshot.windows[0].used_percent, Some(22.0));
    assert_eq!(oauth.todo.as_deref(), Some(accounts::MISSING_SECRET_TODO));
    assert_eq!(oauth.error, None, "残留的待办文案不该留在 error");
    assert!(profile_row.todo.is_none());
    assert!(profile_row.error.is_none());
    let rendered = serde_json::to_string(&dto).unwrap();
    assert!(rendered.contains(accounts::MISSING_SECRET_TODO));
    assert!(rendered.contains(&seeded.snapshot_id));

    let targets = official_quota::targets_for_fetch(&[], &accounts, &[]);
    assert!(targets
        .iter()
        .any(|target| target.quota_id() == seeded.profile_id));
    assert!(targets
        .iter()
        .all(|target| target.quota_id() != seeded.snapshot_id));
    assert!(targets
        .iter()
        .all(|target| target.quota_id() != seeded.oauth_id));

    let stub = Stub::start();
    let account_targets: Vec<_> = targets
        .into_iter()
        .filter(|target| {
            target.quota_id() == seeded.profile_id
                || target.quota_id() == seeded.snapshot_id
                || target.quota_id() == seeded.oauth_id
        })
        .collect();
    assert_eq!(account_targets.len(), 1);
    let fetched = official_quota::fetch_in_parallel(account_targets, |target| {
        let FetchTarget::Account(account) = target else {
            return Err("缺凭证的账号不该进刷新名单".into());
        };
        accounts::fetch_managed_account(account, Some(&stub.base))
    });
    assert!(fetched[0].1.is_ok(), "{:?}", fetched[0].1);
    let hits = stub.hits.lock().unwrap().clone();
    assert!(hits.iter().any(|hit| hit.contains(PROFILE_TOKEN)));
    assert!(hits.iter().all(|hit| !hit.contains(SNAPSHOT_TOKEN)));
    assert!(hits.iter().all(|hit| !hit.contains(OAUTH_TOKEN)));
    let backoff_file = backoff_path(&restored);
    let mut state = official_quota::backoff::BackoffState::default();
    official_quota::fetch::record_backoff(
        &mut state,
        fetched
            .iter()
            .map(|(target, result)| (target.quota_id(), result)),
        chrono::Utc::now(),
        &backoff_file,
    );
    let stored = official_quota::backoff::load_state(&backoff_file);
    assert!(!stored.entries.contains_key(&seeded.snapshot_id));
    assert!(!stored.entries.contains_key(&seeded.oauth_id));

    let reimported = accounts::import_local_login(
        &account_paths(&restored),
        OfficialQuotaProvider::Codex,
        &codex,
        &backoff_file,
    )
    .unwrap();
    assert_eq!(reimported.saved_id, seeded.snapshot_id);
    let accounts = accounts::load_resolved(&account_paths(&restored));
    let dto = official_quota::load_dto_with_accounts(
        &conn,
        &OfficialQuotaConfig::default(),
        &[],
        &accounts,
        &[],
        chrono::Utc::now(),
    );
    assert_eq!(row(&dto, &seeded.snapshot_id).todo, None);
    let targets = official_quota::targets_for_fetch(&[], &accounts, &[]);
    let snapshot_target = targets
        .into_iter()
        .find(|target| target.quota_id() == seeded.snapshot_id)
        .expect("重新导入后这一条要回到刷新名单");
    stub.hits.lock().unwrap().clear();
    let FetchTarget::Account(account) = &snapshot_target else {
        panic!("expected an account target");
    };
    let fetched = accounts::fetch_managed_account(account, Some(&stub.base)).unwrap();
    official_quota::apply_fetch_results(&conn, [(snapshot_target, Ok(fetched))]).unwrap();
    let hits = stub.hits.lock().unwrap().clone();
    assert!(hits.iter().any(|hit| hit.contains(SNAPSHOT_TOKEN)));
    assert!(hits.iter().all(|hit| !hit.contains("/token")));
    let dto = official_quota::load_dto_with_accounts(
        &conn,
        &OfficialQuotaConfig::default(),
        &[],
        &accounts,
        &[],
        chrono::Utc::now(),
    );
    let snapshot = row(&dto, &seeded.snapshot_id);
    assert_eq!(snapshot.todo, None);
    assert_eq!(snapshot.error, None);
    assert_eq!(snapshot.windows[0].used_percent, Some(40.0));

    let reauthorized = accounts::import_local_login(
        &account_paths(&restored),
        OfficialQuotaProvider::Copilot,
        &copilot,
        &backoff_file,
    )
    .unwrap();
    assert_eq!(reauthorized.saved_id, seeded.oauth_id);
    let accounts = accounts::load_resolved(&account_paths(&restored));
    let dto = official_quota::load_dto_with_accounts(
        &conn,
        &OfficialQuotaConfig::default(),
        &[],
        &accounts,
        &[],
        chrono::Utc::now(),
    );
    let oauth = row(&dto, &seeded.oauth_id);
    assert_eq!(oauth.todo, None, "重新导入同一条后待办应消失");
    assert_eq!(oauth.error, None, "残留的待办文案不该再画成失败");
    let targets = official_quota::targets_for_fetch(&[], &accounts, &[]);
    let oauth_target = targets
        .into_iter()
        .find(|target| target.quota_id() == seeded.oauth_id)
        .expect("补上凭证后应用内授权账号要重新取数");
    stub.hits.lock().unwrap().clear();
    let FetchTarget::Account(account) = &oauth_target else {
        panic!("expected an account target");
    };
    let fetched = accounts::fetch_managed_account(account, Some(&stub.base)).unwrap();
    assert!(fetched
        .windows
        .iter()
        .any(|window| window.used_percent == Some(75.0)));
    let hits = stub.hits.lock().unwrap().clone();
    assert!(hits.iter().any(|hit| hit.contains(OAUTH_TOKEN)));
}

fn row<'a>(
    dto: &'a crate::domain::OfficialQuotaDto,
    id: &str,
) -> &'a crate::domain::OfficialQuotaRow {
    dto.rows.iter().find(|row| row.provider == id).unwrap()
}

struct Stub {
    base: String,
    hits: Arc<Mutex<Vec<String>>>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Stub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let hits = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let hits_thread = Arc::clone(&hits);
        let shutdown_thread = Arc::clone(&shutdown);
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
                hits_thread.lock().unwrap().push(raw.clone());
                let path = raw
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("");
                let body = if path.contains("/api/oauth/usage") {
                    r#"{"five_hour":{"utilization":50}}"#
                } else if path.contains("/backend-api/wham/usage") {
                    r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":40,"limit_window_seconds":18000,"reset_at":1893456000}}}"#
                } else if path.contains("/copilot_internal/user") {
                    r#"{"copilot_plan":"individual","quota_reset_date":"2026-09-01","quota_snapshots":{"premium_interactions":{"entitlement":300,"remaining":75,"percent_remaining":25}}}"#
                } else {
                    r#"{"error":"unexpected"}"#
                };
                let response = format!(
                    "HTTP/1.1 200 X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            base,
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
