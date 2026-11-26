//! Copilot 设备码：回环桩上申请、等待、退避、取消、过期、拒绝和成功入库。

use std::path::Path;
use std::time::Duration;

use crate::domain::OfficialQuotaProvider;
use crate::net;
use crate::official_quota::accounts::{self, QuotaAccountPaths, QuotaAccountSource};
use crate::test_support::http_stub::{json_reply, serve, Reply, Stub};

const USER_CODE: &str = "ABCD-EFGH";
const DEVICE_CODE: &str = "device-CODE-SHOULD-NOT-LEAK";
const VERIFY_URL: &str = "https://github.com/login/device";
const TOKEN: &str = "gho_COPILOT_DEVICE_SECRET_9f3a";
const OLD_TOKEN: &str = "gho_COPILOT_LOCAL_SECRET_old";
const LOGIN: &str = "octocat";
const CLIENT_ID: &str = "178c6fc778ccc68e1d6a";

fn agent() -> ureq::Agent {
    net::agent_with_timeout(Duration::from_secs(3))
}

fn paths(dir: &Path) -> QuotaAccountPaths {
    QuotaAccountPaths::in_dir(dir)
}

fn device_reply(interval: u64, expires_in: u64) -> Reply {
    json_reply(
        200,
        serde_json::json!({
            "device_code": DEVICE_CODE,
            "user_code": USER_CODE,
            "verification_uri": VERIFY_URL,
            "expires_in": expires_in,
            "interval": interval,
        }),
    )
}

fn begin(stub: &Stub) -> accounts::CopilotDeviceLogin {
    accounts::start_copilot_device_login(
        &agent(),
        &accounts::CopilotDeviceEndpoints::from_base(&stub.base_url),
    )
    .unwrap_or_else(|error| panic!("device code failed: {error}"))
}

fn poll(
    login: &accounts::CopilotDeviceLogin,
    dir: &Path,
) -> Result<accounts::CopilotDevicePoll, String> {
    accounts::poll_copilot_device_login(
        login,
        &agent(),
        &paths(dir),
        &dir.join("official_quota_backoff.json"),
    )
}

fn assert_no_account(dir: &Path) {
    let panel = accounts::list(&paths(dir));
    assert!(panel.accounts.is_empty());
    let secrets = paths(dir).secrets;
    if secrets.exists() {
        let body = std::fs::read_to_string(secrets).unwrap();
        assert!(!body.contains(TOKEN));
        assert!(!body.contains(DEVICE_CODE));
    }
}

fn captured(stub: &Stub) -> Vec<crate::test_support::http_stub::Captured> {
    stub.captured.lock().unwrap().clone()
}

#[test]
fn copilot_device_login_returns_the_one_time_code_url_and_interval() {
    let stub = serve(vec![device_reply(5, 900)]);
    let login = begin(&stub);
    assert_eq!(login.user_code, USER_CODE);
    assert_eq!(login.verification_url, VERIFY_URL);
    assert_eq!(login.interval_seconds(), 5);

    let requests = captured(&stub);
    assert_eq!(requests.len(), 1);
    assert!(requests[0]
        .request_line
        .starts_with("POST /login/device/code "));
    assert!(requests[0].body.contains(&format!("client_id={CLIENT_ID}")));
    assert!(
        requests[0].body.contains("scope=read%3Auser")
            || requests[0].body.contains("scope=read:user"),
        "{}",
        requests[0].body
    );
    assert!(!requests[0].body.contains("client_secret"));
    let debug = format!("{login:?}");
    assert!(!debug.contains(DEVICE_CODE));
}

#[test]
fn polling_while_the_user_waits_does_not_save_and_honors_the_interval() {
    let stub = serve(vec![
        device_reply(5, 900),
        json_reply(200, serde_json::json!({"error": "authorization_pending"})),
        json_reply(200, serde_json::json!({"error": "authorization_pending"})),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let login = begin(&stub);
    let first = poll(&login, dir.path()).unwrap();
    assert_eq!(
        first,
        accounts::CopilotDevicePoll::Pending {
            interval_seconds: 5
        }
    );
    let second = poll(&login, dir.path()).unwrap();
    assert_eq!(
        second,
        accounts::CopilotDevicePoll::Pending {
            interval_seconds: 5
        }
    );
    assert_eq!(captured(&stub).len(), 2, "间隔未到不再打授权接口");
    assert_no_account(dir.path());
}

#[test]
fn slow_down_without_a_new_interval_waits_five_more_seconds() {
    let stub = serve(vec![
        device_reply(5, 900),
        json_reply(200, serde_json::json!({"error": "slow_down"})),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let login = begin(&stub);
    let polled = poll(&login, dir.path()).unwrap();
    assert_eq!(
        polled,
        accounts::CopilotDevicePoll::Pending {
            interval_seconds: 10
        }
    );
    assert_eq!(login.interval_seconds(), 10);
    let again = poll(&login, dir.path()).unwrap();
    assert_eq!(
        again,
        accounts::CopilotDevicePoll::Pending {
            interval_seconds: 10
        }
    );
    assert_eq!(captured(&stub).len(), 2);
    assert_no_account(dir.path());
}

#[test]
fn slow_down_uses_the_server_interval_when_it_sends_one() {
    let stub = serve(vec![
        device_reply(5, 900),
        json_reply(
            200,
            serde_json::json!({"error": "slow_down", "interval": 15}),
        ),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let login = begin(&stub);
    let polled = poll(&login, dir.path()).unwrap();
    assert_eq!(
        polled,
        accounts::CopilotDevicePoll::Pending {
            interval_seconds: 15
        }
    );
    assert_no_account(dir.path());
}

#[test]
fn cancelling_copilot_device_login_stops_before_saving() {
    let stub = serve(vec![
        device_reply(5, 900),
        json_reply(
            200,
            serde_json::json!({"access_token": TOKEN, "token_type": "bearer"}),
        ),
        json_reply(200, serde_json::json!({"login": LOGIN})),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let login = begin(&stub);
    login.cancel();
    let error = poll(&login, dir.path()).unwrap_err();
    assert_eq!(error, "已取消设备码授权，没有保存额度账号");
    assert_eq!(captured(&stub).len(), 1, "取消后不再轮询");
    assert_no_account(dir.path());
}

#[test]
fn an_already_expired_device_code_stops_with_a_chinese_message() {
    let stub = serve(vec![device_reply(5, 0)]);
    let dir = tempfile::tempdir().unwrap();
    let login = begin(&stub);
    let error = poll(&login, dir.path()).unwrap_err();
    assert_eq!(error, "设备码已过期，没有保存额度账号。请重新发起授权");
    assert_eq!(captured(&stub).len(), 1);
    assert_no_account(dir.path());
}

#[test]
fn an_expired_token_response_stops_with_a_chinese_message() {
    let stub = serve(vec![
        device_reply(5, 900),
        json_reply(400, serde_json::json!({"error": "expired_token"})),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let login = begin(&stub);
    let error = poll(&login, dir.path()).unwrap_err();
    assert_eq!(error, "设备码已过期，没有保存额度账号。请重新发起授权");
    assert_no_account(dir.path());
}

#[test]
fn a_denied_device_code_stops_with_a_chinese_message() {
    let stub = serve(vec![
        device_reply(5, 900),
        json_reply(200, serde_json::json!({"error": "access_denied"})),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let login = begin(&stub);
    let error = poll(&login, dir.path()).unwrap_err();
    assert_eq!(error, "授权已被拒绝，没有保存额度账号");
    assert_no_account(dir.path());
}

#[test]
fn successful_copilot_device_login_stores_a_masked_account_not_the_raw_token() {
    let stub = serve(vec![
        device_reply(5, 900),
        json_reply(
            200,
            serde_json::json!({"access_token": TOKEN, "token_type": "bearer", "scope": "read:user"}),
        ),
        json_reply(200, serde_json::json!({"login": LOGIN, "id": 1})),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let login = begin(&stub);
    let polled = poll(&login, dir.path()).unwrap();
    let accounts::CopilotDevicePoll::Authorized { saved } = polled else {
        panic!("expected the account to be stored");
    };
    assert_eq!(saved.panel.accounts.len(), 1);
    assert_eq!(saved.panel.accounts[0].provider, "copilot");
    assert_eq!(saved.panel.accounts[0].hint, LOGIN);
    assert_eq!(saved.panel.accounts[0].name, LOGIN);
    assert_eq!(saved.panel.accounts[0].source, QuotaAccountSource::Oauth);
    assert!(saved.enabled);
    let panel_json = serde_json::to_string(&saved.panel).unwrap();
    assert!(!panel_json.contains(TOKEN));
    assert!(!panel_json.contains(DEVICE_CODE));
    assert!(!panel_json.contains("fingerprint"));

    let stored = paths(dir.path());
    let config = std::fs::read_to_string(&stored.config).unwrap();
    assert!(config.contains("\"oauth\""));
    assert!(!config.contains(TOKEN));
    assert!(!config.contains(DEVICE_CODE));
    let secret = std::fs::read_to_string(&stored.secrets).unwrap();
    assert!(secret.contains(TOKEN));
    assert!(!secret.contains(DEVICE_CODE));
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

    let resolved = accounts::load_resolved(&stored);
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].config.fingerprint, LOGIN);
    assert_eq!(resolved[0].config.source, QuotaAccountSource::Oauth);
    assert_eq!(
        resolved[0]
            .secret
            .as_ref()
            .map(|item| item.access_token.as_str()),
        Some(TOKEN)
    );

    let requests = captured(&stub);
    assert_eq!(requests.len(), 3);
    assert!(requests[1]
        .request_line
        .starts_with("POST /login/oauth/access_token "));
    assert!(requests[1].body.contains("grant_type=urn"));
    assert!(requests[1].body.contains(DEVICE_CODE));
    assert!(!requests[1].body.contains("client_secret"));
    assert!(requests[2].request_line.starts_with("GET /user "));
    assert!(requests[2].headers.contains(TOKEN));
}

#[test]
fn the_same_copilot_fingerprint_keeps_the_id_and_clears_backoff() {
    let stub = serve(vec![
        device_reply(5, 900),
        json_reply(
            200,
            serde_json::json!({"access_token": TOKEN, "token_type": "bearer"}),
        ),
        json_reply(200, serde_json::json!({"login": LOGIN})),
    ]);
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(source.path().join("github-copilot")).unwrap();
    std::fs::write(
        source.path().join("github-copilot").join("apps.json"),
        format!(r#"{{"github.com:client":{{"oauth_token":"{OLD_TOKEN}","user":"{LOGIN}"}}}}"#),
    )
    .unwrap();
    let backoff_path = app.path().join("official_quota_backoff.json");
    let imported = accounts::import_local_login(
        &paths(app.path()),
        OfficialQuotaProvider::Copilot,
        source.path(),
        &backoff_path,
    )
    .unwrap();
    accounts::save(
        &paths(app.path()),
        accounts::SaveQuotaAccount {
            id: imported.saved_id.clone(),
            name: Some("工作号".to_string()),
            enabled: None,
        },
        &backoff_path,
    )
    .unwrap();
    let mut state = crate::official_quota::backoff::BackoffState::default();
    crate::official_quota::backoff::record_failure(
        &mut state,
        &imported.saved_id,
        "上一轮失败",
        chrono::Utc::now(),
    );
    crate::official_quota::backoff::save_state(&backoff_path, &state).unwrap();

    let login = begin(&stub);
    let polled = poll(&login, app.path()).unwrap();
    let accounts::CopilotDevicePoll::Authorized { saved } = polled else {
        panic!("expected the existing account to be updated");
    };
    assert_eq!(saved.saved_id, imported.saved_id);
    assert_eq!(saved.panel.accounts.len(), 1);
    assert_eq!(saved.panel.accounts[0].name, "工作号");
    assert_eq!(saved.panel.accounts[0].hint, LOGIN);
    assert_eq!(saved.panel.accounts[0].source, QuotaAccountSource::Oauth);
    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap();
    assert!(secret.contains(TOKEN));
    assert!(!secret.contains(OLD_TOKEN));
    let panel_json = serde_json::to_string(&saved.panel).unwrap();
    assert!(!panel_json.contains(TOKEN));
    assert!(!panel_json.contains(OLD_TOKEN));
    let cleared = crate::official_quota::backoff::load_state(&backoff_path);
    assert!(!cleared.entries.contains_key(&saved.saved_id));
}
