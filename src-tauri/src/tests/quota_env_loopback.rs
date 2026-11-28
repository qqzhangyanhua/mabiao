//! 端到端回环 HTTP 桩测试：走真实 `ureq`（`IsolatedHttpTransport` + `quota_base`）验证
//! `QuotaEnv` 下默认 `ScanEnv` 凭证根、隔离 `profile_dir` 额度账号、`open_readonly` 纯读、
//! 与 `refresh_and_notify` 的持久化、去重告警及更新广播。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::domain::{OfficialQuotaDto, OfficialQuotaProvider};
use crate::official_quota::{self, accounts, notify, QuotaEnv, QuotaPaths, RefreshOptions};
use crate::scan_paths::{ScanEnv, ScanPathConfig};
use crate::store;
use crate::test_support::TestConnection;

struct RoutingStub {
    base_url: String,
    seen: Arc<Mutex<Vec<(String, String)>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl RoutingStub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let seen_bg = Arc::clone(&seen);
        let stop_bg = Arc::clone(&stop);

        let handle = std::thread::spawn(move || {
            while !stop_bg.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let Some(raw) = read_http(&mut stream) else {
                            continue;
                        };
                        let path = raw
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .unwrap_or("/")
                            .to_string();
                        let auth = raw
                            .lines()
                            .find_map(|line| {
                                let (k, v) = line.split_once(':')?;
                                k.eq_ignore_ascii_case("authorization")
                                    .then(|| v.trim().to_string())
                            })
                            .unwrap_or_default();
                        seen_bg.lock().unwrap().push((path.clone(), auth.clone()));

                        let body = if path.starts_with("/api/oauth/usage") {
                            r#"{"five_hour":{"utilization":86.0,"resets_at":"2026-09-10T17:00:00Z"}}"#
                        } else if path.starts_with("/backend-api/wham/usage") {
                            if auth.contains("profile-token") {
                                r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":45.0,"limit_window_seconds":18000,"reset_at":1789059600}}}"#
                            } else {
                                r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":12.0,"limit_window_seconds":18000,"reset_at":1789059600}}}"#
                            }
                        } else {
                            r#"{"error":"unexpected"}"#
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                        let _ = stream.shutdown(std::net::Shutdown::Write);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            base_url,
            seen,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for RoutingStub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn read_http(stream: &mut std::net::TcpStream) -> Option<String> {
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

fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

#[test]
fn quota_env_loopback_end_to_end_covers_scan_env_profile_dir_and_lifecycle() {
    let stub = RoutingStub::start();
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let app_data = dir.path().join("app_data");
    let claude_custom = dir.path().join("claude-custom");
    let codex_profile = dir.path().join("codex-work-profile");
    std::fs::create_dir_all(&app_data).unwrap();

    write_file(
        &claude_custom.join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"claude-custom-token","scopes":["user:profile"],"expiresAt":0}}"#,
    );
    write_file(
        &home.join(".codex/auth.json"),
        r#"{"tokens":{"access_token":"default-codex-token","account_id":"acct_default"}}"#,
    );
    write_file(
        &codex_profile.join("auth.json"),
        r#"{"tokens":{"access_token":"profile-token","account_id":"acct_work"}}"#,
    );

    let paths = QuotaPaths::in_dir(&app_data);
    let saved = accounts::save_profile_dir(
        &paths.accounts,
        accounts::SaveProfileDirAccount {
            id: None,
            provider: "codex".into(),
            name: "Work".into(),
            profile_dir: codex_profile.to_string_lossy().into_owned(),
        },
        &paths.backoff,
    )
    .unwrap();

    let scan_env = ScanEnv::new(&home).with_ui_config(ScanPathConfig {
        overrides: BTreeMap::from([(
            "claude".to_string(),
            vec![claude_custom.to_string_lossy().into_owned()],
        )]),
    });
    let now = DateTime::parse_from_rfc3339("2026-09-10T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let env = QuotaEnv::isolated(scan_env, paths.clone(), now)
        .with_quota_base(Some(stub.base_url.clone()));

    let conns = TestConnection::new(store::open_memory().unwrap());
    let alerts = Mutex::new(Vec::<notify::QuotaAlert>::new());
    let emitted = Mutex::new(Vec::<OfficialQuotaDto>::new());
    let on_alert = |alert: &notify::QuotaAlert, _title: &str, _body: &str| {
        alerts.lock().unwrap().push(alert.clone());
    };
    let on_updated = |dto: &OfficialQuotaDto| {
        emitted.lock().unwrap().push(dto.clone());
    };

    let dto = official_quota::refresh_and_notify(
        &conns,
        &env,
        RefreshOptions {
            notify: Some(&on_alert),
            on_updated: Some(&on_updated),
            stream_progress: false,
        },
    )
    .unwrap();

    let claude = dto
        .rows
        .iter()
        .find(|row| row.provider == OfficialQuotaProvider::Claude.as_str())
        .unwrap();
    assert_eq!(claude.windows[0].used_percent, Some(86.0));

    let work_codex = dto
        .rows
        .iter()
        .find(|row| row.provider == saved.saved_id)
        .unwrap();
    assert_eq!(work_codex.windows[0].used_percent, Some(45.0));

    let default_codex = dto
        .rows
        .iter()
        .find(|row| row.provider == OfficialQuotaProvider::Codex.as_str())
        .unwrap();
    assert_eq!(default_codex.windows[0].used_percent, Some(12.0));

    assert_eq!(alerts.lock().unwrap().len(), 1);
    assert_eq!(alerts.lock().unwrap()[0].threshold, 80);
    assert_eq!(emitted.lock().unwrap().len(), 1);

    let seen = stub.seen.lock().unwrap();
    assert!(seen
        .iter()
        .any(|(path, auth)| path.starts_with("/api/oauth/usage")
            && auth == "Bearer claude-custom-token"));
    assert!(seen
        .iter()
        .any(|(path, auth)| path.starts_with("/backend-api/wham/usage")
            && auth == "Bearer profile-token"));
    assert!(seen
        .iter()
        .any(|(path, auth)| path.starts_with("/backend-api/wham/usage")
            && auth == "Bearer default-codex-token"));
}
