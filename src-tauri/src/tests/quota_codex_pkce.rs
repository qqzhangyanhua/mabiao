//! Codex PKCE：回环回调、状态校验、端口占用时粘贴回调、取消释放端口、令牌走注入的代理缝。
//!
//! 不打 auth.openai.com，不读不写 `~/.codex`。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use url::Url;

use crate::domain::OfficialQuotaProvider;
use crate::net;
use crate::official_quota::accounts::{self, QuotaAccountPaths, QuotaAccountSource};
use crate::test_support::http_stub::{json_reply, serve, Reply, Stub};

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ACCOUNT_ID: &str = "acct_01HZwork";
const EMAIL: &str = "ada@example.com";
const AUTH_CODE: &str = "codex-auth-code-SHOULD-NOT-LEAK";
const ACCESS: &str = "codex-access-SHOULD-NOT-LEAK";
const REFRESH: &str = "codex-refresh-SHOULD-NOT-LEAK";
const OLD_ACCESS: &str = "codex-local-access-OLD";
const OLD_REFRESH: &str = "codex-local-refresh-OLD";

fn agent() -> ureq::Agent {
    net::agent_with_timeout(Duration::from_secs(3))
}

fn paths(dir: &Path) -> QuotaAccountPaths {
    QuotaAccountPaths::in_dir(dir)
}

fn backoff(dir: &Path) -> std::path::PathBuf {
    dir.join("official_quota_backoff.json")
}

fn jwt(claims: &str) -> String {
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims);
    format!("{header}.{payload}.sig")
}

fn id_token() -> String {
    jwt(&format!(
        r#"{{"email":"{EMAIL}","https://api.openai.com/auth":{{"chatgpt_account_id":"{ACCOUNT_ID}"}}}}"#
    ))
}

fn expired_access() -> String {
    jwt(&format!(
        r#"{{"exp":1,"https://api.openai.com/auth":{{"chatgpt_account_id":"{ACCOUNT_ID}"}}}}"#
    ))
}

fn token_reply(access: &str) -> Reply {
    json_reply(
        200,
        serde_json::json!({
            "access_token": access,
            "refresh_token": REFRESH,
            "id_token": id_token(),
            "expires_in": 3600,
            "token_type": "Bearer",
        }),
    )
}

fn endpoints(stub: &Stub) -> accounts::CodexPkceEndpoints {
    accounts::CodexPkceEndpoints::from_base(&stub.base_url)
}

fn begin(stub: &Stub, port: u16) -> accounts::CodexPkceLogin {
    accounts::start_codex_pkce_login(&endpoints(stub), port)
        .unwrap_or_else(|error| panic!("pkce start failed: {error}"))
}

fn poll(login: &accounts::CodexPkceLogin, dir: &Path) -> Result<accounts::CodexPkcePoll, String> {
    accounts::poll_codex_pkce_login(login, &agent(), &paths(dir), &backoff(dir))
}

fn state_of(login: &accounts::CodexPkceLogin) -> String {
    Url::parse(login.authorize_url())
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
        .expect("authorize url has state")
}

fn callback_url(login: &accounts::CodexPkceLogin, code: &str, state: &str) -> String {
    format!("{}?code={code}&state={state}", login.redirect_uri())
}

fn hit_callback(login: &accounts::CodexPkceLogin, code: &str, state: &str) -> String {
    let url = Url::parse(&callback_url(login, code, state)).unwrap();
    let host = url.host_str().unwrap();
    let port = url.port().unwrap();
    let mut stream = TcpStream::connect((host, port)).unwrap();
    let request = format!(
        "GET {}?{} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
        url.path(),
        url.query().unwrap_or("")
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut body = String::new();
    stream.read_to_string(&mut body).unwrap();
    body
}

fn captured(stub: &Stub) -> Vec<crate::test_support::http_stub::Captured> {
    stub.captured.lock().unwrap().clone()
}

fn form_value(body: &str, key: &str) -> String {
    for pair in body.split('&') {
        let Some((raw_key, raw_value)) = pair.split_once('=') else {
            continue;
        };
        let Ok(decoded_key) = urlencoding::decode(raw_key) else {
            continue;
        };
        if decoded_key == key {
            return urlencoding::decode(raw_value)
                .unwrap_or_default()
                .into_owned();
        }
    }
    String::new()
}

fn assert_no_account(dir: &Path) {
    let panel = accounts::list(&paths(dir));
    assert!(panel.accounts.is_empty());
    let secrets = paths(dir).secrets;
    if secrets.exists() {
        let body = std::fs::read_to_string(secrets).unwrap();
        assert!(!body.contains(ACCESS));
        assert!(!body.contains(REFRESH));
        assert!(!body.contains(AUTH_CODE));
    }
}

fn assert_port_free(port: u16) {
    TcpListener::bind(("127.0.0.1", port))
        .unwrap_or_else(|error| panic!("port {port} still taken: {error}"));
}

#[test]
fn codex_pkce_callback_stores_an_oauth_account_and_closes_the_port() {
    let stub = serve(vec![token_reply(ACCESS)]);
    let dir = tempfile::tempdir().unwrap();
    let login = begin(&stub, 0);
    assert!(login.listening());
    let port = login.listen_port().expect("listener port");
    let state = state_of(&login);
    let authorize = login.authorize_url().to_string();
    assert!(authorize.contains(&format!("client_id={CLIENT_ID}")));
    assert!(authorize.contains("code_challenge_method=S256"));
    assert!(authorize.contains("code_challenge="));
    assert!(!authorize.contains("client_secret"));
    assert!(!authorize.contains("code_verifier"));
    let page = hit_callback(&login, AUTH_CODE, &state);
    assert!(page.contains("码表"));

    let polled = poll(&login, dir.path()).unwrap();
    let accounts::CodexPkcePoll::Authorized { saved } = polled else {
        panic!("expected the account to be stored");
    };
    assert_eq!(saved.panel.accounts.len(), 1);
    assert_eq!(saved.panel.accounts[0].provider, "codex");
    assert_eq!(saved.panel.accounts[0].source, QuotaAccountSource::Oauth);
    assert_eq!(saved.panel.accounts[0].name, EMAIL);
    assert_eq!(saved.panel.accounts[0].hint, EMAIL);
    let panel_json = serde_json::to_string(&saved.panel).unwrap();
    assert!(!panel_json.contains(ACCESS));
    assert!(!panel_json.contains(REFRESH));
    assert!(!panel_json.contains(AUTH_CODE));
    assert!(!panel_json.contains("fingerprint"));

    let stored = paths(dir.path());
    let config = std::fs::read_to_string(&stored.config).unwrap();
    assert!(config.contains("\"oauth\""));
    assert!(!config.contains(ACCESS));
    assert!(!config.contains(REFRESH));
    let secret = std::fs::read_to_string(&stored.secrets).unwrap();
    assert!(secret.contains(ACCESS));
    assert!(secret.contains(REFRESH));
    assert!(secret.contains(ACCOUNT_ID));
    assert!(secret.contains(CLIENT_ID));
    assert!(!secret.contains(AUTH_CODE));
    assert!(!secret.contains(&id_token()));
    assert!(!secret.contains("client_secret"));
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
    assert_eq!(resolved[0].config.fingerprint, ACCOUNT_ID);
    assert_eq!(resolved[0].config.source, QuotaAccountSource::Oauth);
    assert_eq!(
        resolved[0]
            .secret
            .as_ref()
            .map(|item| item.access_token.as_str()),
        Some(ACCESS)
    );
    assert_eq!(
        resolved[0]
            .secret
            .as_ref()
            .and_then(|item| item.refresh_token.as_deref()),
        Some(REFRESH)
    );
    assert_eq!(
        resolved[0]
            .secret
            .as_ref()
            .and_then(|item| item.refresh_client_id.as_deref()),
        Some(CLIENT_ID)
    );

    let requests = captured(&stub);
    assert_eq!(requests.len(), 1);
    assert!(requests[0].request_line.starts_with("POST /oauth/token "));
    assert!(!requests[0].request_line.contains("auth.openai.com"));
    let body = &requests[0].body;
    assert_eq!(form_value(body, "grant_type"), "authorization_code");
    assert_eq!(form_value(body, "client_id"), CLIENT_ID);
    assert_eq!(form_value(body, "code"), AUTH_CODE);
    assert_eq!(form_value(body, "redirect_uri"), login.redirect_uri());
    let verifier = form_value(body, "code_verifier");
    assert!(!verifier.is_empty());
    assert!(!authorize.contains(&verifier));
    assert!(!body.contains("client_secret"));
    assert_port_free(port);
}

#[test]
fn a_mismatched_callback_state_is_rejected_without_a_token_request() {
    let stub = serve(vec![token_reply(ACCESS)]);
    let dir = tempfile::tempdir().unwrap();
    let login = begin(&stub, 0);
    let page = hit_callback(&login, AUTH_CODE, "not-the-state");
    assert!(page.contains("没有保存"));
    let error = poll(&login, dir.path()).unwrap_err();
    assert_eq!(error, "回调状态对不上，没有保存额度账号");
    assert!(captured(&stub).is_empty());
    assert_no_account(dir.path());
    assert_port_free(login.listen_port().unwrap());
}

#[test]
fn a_pasted_callback_works_when_the_listen_port_is_taken() {
    let stub = serve(vec![token_reply(ACCESS)]);
    let dir = tempfile::tempdir().unwrap();
    let holder = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = holder.local_addr().unwrap().port();
    let login = begin(&stub, port);
    assert!(!login.listening());
    assert!(login.listen_port().is_none());
    let hint = login.paste_hint().unwrap_or("");
    assert!(hint.contains("端口"), "{hint}");
    assert!(hint.contains("粘贴"), "{hint}");
    let pasted = format!(
        "http://127.0.0.1:{port}/auth/callback?code={AUTH_CODE}&state={}",
        state_of(&login)
    );
    let saved = accounts::submit_codex_pkce_callback(
        &login,
        &pasted,
        &agent(),
        &paths(dir.path()),
        &backoff(dir.path()),
    )
    .unwrap_or_else(|error| panic!("paste failed: {error}"));
    assert_eq!(saved.panel.accounts.len(), 1);
    assert_eq!(saved.panel.accounts[0].provider, "codex");
    assert_eq!(saved.panel.accounts[0].source, QuotaAccountSource::Oauth);
    assert_eq!(saved.panel.accounts[0].hint, EMAIL);
    let requests = captured(&stub);
    assert_eq!(requests.len(), 1);
    assert!(requests[0].request_line.starts_with("POST /oauth/token "));
    assert_eq!(form_value(&requests[0].body, "code"), AUTH_CODE);
    assert_eq!(form_value(&requests[0].body, "client_id"), CLIENT_ID);
    assert!(!requests[0].body.contains("client_secret"));
    assert!(holder.local_addr().is_ok());
}

#[test]
fn cancelling_codex_pkce_login_releases_the_port_and_saves_nothing() {
    let stub = serve(vec![token_reply(ACCESS)]);
    let dir = tempfile::tempdir().unwrap();
    let login = begin(&stub, 0);
    let port = login.listen_port().unwrap();
    login.cancel();
    assert_port_free(port);
    let error = poll(&login, dir.path()).unwrap_err();
    assert_eq!(error, "已取消浏览器授权，没有保存额度账号");
    assert!(captured(&stub).is_empty());
    assert_no_account(dir.path());
    assert_port_free(port);
}

#[test]
fn the_same_codex_fingerprint_keeps_the_id_and_clears_backoff() {
    let stub = serve(vec![token_reply(ACCESS)]);
    let app = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(
        source.path().join("auth.json"),
        format!(
            r#"{{"tokens":{{"access_token":"{OLD_ACCESS}","account_id":"{ACCOUNT_ID}","refresh_token":"{OLD_REFRESH}"}}}}"#
        ),
    )
    .unwrap();
    let imported = accounts::import_local_login(
        &paths(app.path()),
        OfficialQuotaProvider::Codex,
        source.path(),
        &backoff(app.path()),
    )
    .unwrap();
    accounts::save(
        &paths(app.path()),
        accounts::SaveQuotaAccount {
            id: imported.saved_id.clone(),
            name: Some("工作号".to_string()),
            enabled: None,
        },
        &backoff(app.path()),
    )
    .unwrap();
    let mut state = crate::official_quota::backoff::BackoffState::default();
    crate::official_quota::backoff::record_failure(
        &mut state,
        &imported.saved_id,
        "上一轮失败",
        chrono::Utc::now(),
    );
    crate::official_quota::backoff::save_state(&backoff(app.path()), &state).unwrap();

    let login = begin(&stub, 0);
    hit_callback(&login, AUTH_CODE, &state_of(&login));
    let polled = poll(&login, app.path()).unwrap();
    let accounts::CodexPkcePoll::Authorized { saved } = polled else {
        panic!("expected the existing account to be updated");
    };
    assert_eq!(saved.saved_id, imported.saved_id);
    assert_eq!(saved.panel.accounts.len(), 1);
    assert_eq!(saved.panel.accounts[0].name, "工作号");
    assert_eq!(saved.panel.accounts[0].hint, EMAIL);
    assert_eq!(saved.panel.accounts[0].source, QuotaAccountSource::Oauth);
    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap();
    assert!(secret.contains(ACCESS));
    assert!(secret.contains(REFRESH));
    assert!(!secret.contains(OLD_ACCESS));
    assert!(!secret.contains(OLD_REFRESH));
    let cleared = crate::official_quota::backoff::load_state(&backoff(app.path()));
    assert!(!cleared.entries.contains_key(&saved.saved_id));
    let client = std::fs::read_to_string(source.path().join("auth.json")).unwrap();
    assert!(client.contains(OLD_ACCESS));
    assert!(!client.contains(ACCESS));
}

#[test]
fn an_expired_oauth_codex_token_refreshes_without_touching_auth_json() {
    let fresh = "access-REFRESHED";
    let rotated = "refresh-ROTATED";
    let stub = serve(vec![
        json_reply(
            200,
            serde_json::json!({
                "access_token": expired_access(),
                "refresh_token": REFRESH,
                "id_token": id_token(),
                "expires_in": 3600,
                "token_type": "Bearer",
            }),
        ),
        json_reply(
            200,
            serde_json::json!({
                "access_token": fresh,
                "refresh_token": rotated,
            }),
        ),
        json_reply(
            200,
            serde_json::json!({
                "plan_type": "plus",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 50,
                        "limit_window_seconds": 18000,
                        "reset_at": 1893456000
                    }
                }
            }),
        ),
    ]);
    let app = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let auth_path = outside.path().join("auth.json");
    let original = r#"{"tokens":{"access_token":"FAKE-LOCAL-SHOULD-STAY","account_id":"someone-else","refresh_token":"FAKE-REFRESH"}}"#.to_string();
    std::fs::write(&auth_path, &original).unwrap();
    let modified = std::fs::metadata(&auth_path).unwrap().modified().unwrap();

    let login = begin(&stub, 0);
    hit_callback(&login, AUTH_CODE, &state_of(&login));
    let polled = poll(&login, app.path()).unwrap();
    let accounts::CodexPkcePoll::Authorized { saved } = polled else {
        panic!("expected the oauth account");
    };
    let accounts = accounts::load_resolved(&paths(app.path()));
    let account = accounts
        .iter()
        .find(|item| item.config.id == saved.saved_id)
        .unwrap();
    let target = accounts::prepare_manual_target(account, &[]).unwrap();
    assert!(target.live_root.is_none());
    assert!(target.refresh.is_some());
    let snapshot = accounts::fetch_managed_account(&target, Some(&stub.base_url))
        .unwrap_or_else(|error| panic!("refresh failed: {error}"));
    assert!(snapshot
        .windows
        .iter()
        .any(|window| window.used_percent == Some(50.0)));

    let secret = std::fs::read_to_string(paths(app.path()).secrets).unwrap();
    assert!(secret.contains(fresh));
    assert!(secret.contains(rotated));
    assert!(!secret.contains(REFRESH));
    assert_eq!(std::fs::read_to_string(&auth_path).unwrap(), original);
    assert_eq!(
        std::fs::metadata(&auth_path).unwrap().modified().unwrap(),
        modified
    );
    let requests = captured(&stub);
    assert_eq!(requests.len(), 3);
    assert!(requests[1].request_line.starts_with("POST /oauth/token "));
    assert_eq!(form_value(&requests[1].body, "grant_type"), "refresh_token");
    assert_eq!(form_value(&requests[1].body, "client_id"), CLIENT_ID);
    assert_eq!(form_value(&requests[1].body, "refresh_token"), REFRESH);
    assert!(!requests[1].body.contains("client_secret"));
    assert!(requests[2]
        .request_line
        .starts_with("GET /backend-api/wham/usage "));
}
