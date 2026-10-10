//! 设置页远程服务：地址校验、登录换 token、token 文件、退出登录、备份不含 token（ADR 0026）。
//! 联网部分对一个本机回环上的桩服务器跑真实 HTTP。

use std::net::TcpListener;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use push_protocol::{ApiErrorCode, LoginResponse, RemoteRole};

use crate::backup;
use crate::remote_server::address::normalize_base_url;
use crate::remote_server::store::{self, RemoteServerPaths, StoredToken, CONFIG_NAME, TOKEN_NAME};
use crate::remote_server::{self, LoginInput, SessionState};
use crate::store as db_store;
use crate::test_support::http_stub::{api_error, json_reply, serve, Reply};

fn now() -> DateTime<Utc> {
    "2026-10-10T00:00:00Z".parse().unwrap()
}

fn login_reply(token: &str, account: &str) -> Reply {
    json_reply(
        200,
        LoginResponse {
            token: token.to_string(),
            expires_at: (now() + Duration::days(30)).to_rfc3339(),
            account: account.to_string(),
            role: RemoteRole::Member,
        },
    )
}

fn input(base_url: &str, password: &str) -> LoginInput {
    LoginInput {
        base_url: base_url.to_string(),
        account: "alice".to_string(),
        password: password.to_string(),
        device_name: Some("我的 MacBook".to_string()),
    }
}

fn paths(dir: &Path) -> RemoteServerPaths {
    RemoteServerPaths::in_dir(dir)
}

fn all_file_text(dir: &Path) -> String {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| std::fs::read_to_string(entry.unwrap().path()).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- 地址校验 ----

#[test]
fn https_is_required_but_loopback_may_use_http() {
    for ok in [
        "https://mabiao.example.com",
        "http://localhost:8080",
        "http://LOCALHOST",
        "http://127.0.0.1:3000",
        "http://127.8.9.10",
        "http://[::1]:3000",
    ] {
        assert!(normalize_base_url(ok).is_ok(), "{ok} 应通过");
    }
    for bad in [
        "http://mabiao.example.com",
        "http://192.168.1.5:8080",
        "http://localhost.evil.com",
        "ftp://mabiao.example.com",
        "mabiao.example.com",
        "localhost:8080",
        "",
        "   ",
    ] {
        assert!(normalize_base_url(bad).is_err(), "{bad:?} 应被拒绝");
    }
}

#[test]
fn http_rejection_explains_why_in_chinese() {
    let error = normalize_base_url("http://mabiao.example.com").unwrap_err();
    assert!(error.contains("https"), "{error}");
}

#[test]
fn normalization_strips_slashes_and_rejects_credentials_and_params() {
    assert_eq!(
        normalize_base_url("  https://Mabiao.Example.com/  ").unwrap(),
        "https://mabiao.example.com"
    );
    assert_eq!(
        normalize_base_url("https://example.com:8443/team//").unwrap(),
        "https://example.com:8443/team"
    );
    assert!(normalize_base_url("https://alice:secret@example.com").is_err());
    assert!(normalize_base_url("https://example.com/?token=1").is_err());
    assert!(normalize_base_url("https://example.com/#x").is_err());
}

#[test]
fn invalid_address_is_rejected_before_any_request() {
    let dir = tempfile::tempdir().unwrap();
    let error = remote_server::login(&paths(dir.path()), input("http://example.com", "pw"), now())
        .unwrap_err();
    assert!(error.contains("https"), "{error}");
    assert!(!dir.path().join(TOKEN_NAME).exists());
    assert!(!dir.path().join(CONFIG_NAME).exists());
}

// ---- 登录与落盘 ----

#[test]
fn login_posts_credentials_and_stores_token_separately_from_config() {
    let dir = tempfile::tempdir().unwrap();
    let stub = serve(vec![login_reply("tok-secret-123", "alice")]);

    let dto =
        remote_server::login(&paths(dir.path()), input(&stub.base_url, "hunter2"), now()).unwrap();

    assert_eq!(dto.state, SessionState::LoggedIn);
    assert_eq!(dto.account, "alice");
    assert_eq!(dto.role, Some(RemoteRole::Member));
    assert_eq!(dto.device_name, "我的 MacBook");
    assert!(dto.notice.is_none());

    let captured = stub.captured.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert!(
        captured[0].request_line.starts_with("POST /api/v1/login"),
        "{}",
        captured[0].request_line
    );
    let sent: serde_json::Value = serde_json::from_str(&captured[0].body).unwrap();
    assert_eq!(sent["account"], "alice");
    assert_eq!(sent["password"], "hunter2");
    assert_eq!(sent["protocol_version"], push_protocol::PROTOCOL_VERSION);

    let config_text = std::fs::read_to_string(dir.path().join(CONFIG_NAME)).unwrap();
    assert!(config_text.contains(&stub.base_url));
    assert!(
        !config_text.contains("tok-secret-123"),
        "配置里不该有 token"
    );
    let token_text = std::fs::read_to_string(dir.path().join(TOKEN_NAME)).unwrap();
    assert!(token_text.contains("tok-secret-123"));
}

#[test]
fn password_is_never_written_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let stub = serve(vec![login_reply("tok-1", "alice")]);
    remote_server::login(
        &paths(dir.path()),
        input(&stub.base_url, "hunter2-unique-password"),
        now(),
    )
    .unwrap();

    assert!(!all_file_text(dir.path()).contains("hunter2-unique-password"));
}

#[test]
fn login_input_debug_hides_password() {
    let text = format!("{:?}", input("https://example.com", "hunter2"));
    assert!(!text.contains("hunter2"), "{text}");
}

#[cfg(unix)]
#[test]
fn token_file_is_owner_only_even_when_replacing_a_looser_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let token_path = dir.path().join(TOKEN_NAME);
    std::fs::write(&token_path, "old").unwrap();
    std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o644)).unwrap();

    let stub = serve(vec![login_reply("tok-1", "alice")]);
    remote_server::login(&paths(dir.path()), input(&stub.base_url, "pw"), now()).unwrap();

    let mode = std::fs::metadata(&token_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert!(std::fs::read_to_string(&token_path)
        .unwrap()
        .contains("tok-1"));
}

#[cfg(unix)]
#[test]
fn fresh_token_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    store::save_token(
        &dir.path().join(TOKEN_NAME),
        &StoredToken {
            token: "t".into(),
            expires_at: "2026-11-09T00:00:00Z".into(),
            account: "alice".into(),
            role: RemoteRole::Member,
            rejected: false,
        },
    )
    .unwrap();
    let mode = std::fs::metadata(dir.path().join(TOKEN_NAME))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn wrong_password_gives_a_plain_chinese_error_and_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let stub = serve(vec![api_error(401, ApiErrorCode::InvalidCredentials)]);

    let error =
        remote_server::login(&paths(dir.path()), input(&stub.base_url, "bad"), now()).unwrap_err();

    assert!(error.contains("账号或密码不对"), "{error}");
    assert!(!dir.path().join(TOKEN_NAME).exists());
    assert!(!dir.path().join(CONFIG_NAME).exists());
}

#[test]
fn unreachable_server_is_reported_in_chinese() {
    let dir = tempfile::tempdir().unwrap();
    // 绑定后立刻释放，拿到一个没人监听的端口。
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    let error = remote_server::login(
        &paths(dir.path()),
        input(&format!("http://127.0.0.1:{port}"), "pw"),
        now(),
    )
    .unwrap_err();

    assert!(error.contains("连不上"), "{error}");
}

#[test]
fn a_site_that_is_not_the_remote_server_is_explained() {
    let dir = tempfile::tempdir().unwrap();
    let stub = serve(vec![Reply {
        status: 404,
        body: "<html>nope</html>".to_string(),
        location: None,
    }]);

    let error =
        remote_server::login(&paths(dir.path()), input(&stub.base_url, "pw"), now()).unwrap_err();

    assert!(error.contains("没有找到码表远程服务"), "{error}");
}

#[test]
fn redirects_are_not_followed_so_the_password_cannot_be_forwarded() {
    let dir = tempfile::tempdir().unwrap();
    let stub = serve(vec![Reply {
        status: 307,
        body: String::new(),
        location: Some("http://127.0.0.1:9/api/v1/login"),
    }]);

    let error =
        remote_server::login(&paths(dir.path()), input(&stub.base_url, "pw"), now()).unwrap_err();

    assert!(error.contains("跳转"), "{error}");
    assert_eq!(stub.captured.lock().unwrap().len(), 1);
}

#[test]
fn incompatible_protocol_version_asks_to_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let stub = serve(vec![api_error(
        400,
        ApiErrorCode::UnsupportedProtocolVersion,
    )]);

    let error =
        remote_server::login(&paths(dir.path()), input(&stub.base_url, "pw"), now()).unwrap_err();

    assert!(error.contains("升级"), "{error}");
}

// ---- 设备身份 ----

#[test]
fn device_id_is_generated_once_and_survives_relogin_and_rename() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths(dir.path());
    let stub = serve(vec![
        login_reply("tok-1", "alice"),
        login_reply("tok-2", "alice"),
    ]);

    let before = remote_server::panel(&paths, now());
    assert!(before.device_id.is_none());

    let first = remote_server::login(&paths, input(&stub.base_url, "pw"), now()).unwrap();
    let device_id = first.device_id.clone().expect("首次登录生成设备 ID");
    assert_eq!(device_id.len(), 32);

    let renamed = remote_server::rename_device(&paths, "  公司工作站 ", now()).unwrap();
    assert_eq!(renamed.device_name, "公司工作站");
    assert_eq!(renamed.device_id.as_deref(), Some(device_id.as_str()));

    // 再次登录：设备 ID 不变；表单里没填设备名则沿用改过的名字。
    let mut again = input(&stub.base_url, "pw");
    again.device_name = None;
    let second = remote_server::login(&paths, again, now()).unwrap();
    assert_eq!(second.device_id.as_deref(), Some(device_id.as_str()));
    assert_eq!(second.device_name, "公司工作站");
}

#[test]
fn first_login_without_a_device_name_still_gets_one() {
    let dir = tempfile::tempdir().unwrap();
    let stub = serve(vec![login_reply("tok-1", "alice")]);
    let mut request = input(&stub.base_url, "pw");
    request.device_name = Some("   ".to_string());

    let dto = remote_server::login(&paths(dir.path()), request, now()).unwrap();

    assert!(!dto.device_name.trim().is_empty());
}

#[test]
fn rename_requires_a_name_and_a_logged_in_device() {
    let dir = tempfile::tempdir().unwrap();
    assert!(remote_server::rename_device(&paths(dir.path()), "x", now()).is_err());

    let stub = serve(vec![login_reply("tok-1", "alice")]);
    remote_server::login(&paths(dir.path()), input(&stub.base_url, "pw"), now()).unwrap();
    assert!(remote_server::rename_device(&paths(dir.path()), "   ", now()).is_err());
}

// ---- 过期、被拒、退出 ----

#[test]
fn token_past_its_expiry_prompts_relogin() {
    let dir = tempfile::tempdir().unwrap();
    let stub = serve(vec![login_reply("tok-1", "alice")]);
    remote_server::login(&paths(dir.path()), input(&stub.base_url, "pw"), now()).unwrap();

    let later = now() + Duration::days(31);
    let dto = remote_server::panel(&paths(dir.path()), later);

    assert_eq!(dto.state, SessionState::Expired);
    assert!(dto.notice.unwrap().contains("重新登录"));
    // 账号与地址还在，表单可以直接填密码重登。
    assert_eq!(dto.account, "alice");
    assert_eq!(dto.base_url, stub.base_url);
}

#[test]
fn verify_marks_a_rejected_token_and_drops_it() {
    let dir = tempfile::tempdir().unwrap();
    let stub = serve(vec![
        login_reply("tok-to-reject", "alice"),
        api_error(401, ApiErrorCode::TokenExpired),
    ]);
    remote_server::login(&paths(dir.path()), input(&stub.base_url, "pw"), now()).unwrap();

    let dto = remote_server::verify(&paths(dir.path()), now()).unwrap();

    assert_eq!(dto.state, SessionState::Rejected);
    assert!(dto.notice.unwrap().contains("重新登录"));
    let captured = stub.captured.lock().unwrap();
    assert!(captured[1].request_line.starts_with("GET /api/v1/me"));
    assert!(captured[1]
        .headers
        .to_ascii_lowercase()
        .contains("authorization: bearer tok-to-reject"));
    assert!(
        !all_file_text(dir.path()).contains("tok-to-reject"),
        "被拒的 token 不该继续留在磁盘上"
    );
}

#[test]
fn verify_keeps_the_login_when_the_server_is_merely_unreachable() {
    let dir = tempfile::tempdir().unwrap();
    let stub = serve(vec![login_reply("tok-1", "alice")]);
    remote_server::login(&paths(dir.path()), input(&stub.base_url, "pw"), now()).unwrap();
    // 桩服务器只回答了登录，之后的连接被拒绝。

    let error = remote_server::verify(&paths(dir.path()), now()).unwrap_err();

    assert!(error.contains("连不上"), "{error}");
    assert_eq!(
        remote_server::panel(&paths(dir.path()), now()).state,
        SessionState::LoggedIn
    );
}

#[test]
fn verify_is_offline_when_not_logged_in() {
    let dir = tempfile::tempdir().unwrap();
    let dto = remote_server::verify(&paths(dir.path()), now()).unwrap();
    assert_eq!(dto.state, SessionState::NotLoggedIn);
}

#[test]
fn logout_removes_the_token_file_but_keeps_the_form_filled() {
    let dir = tempfile::tempdir().unwrap();
    let stub = serve(vec![login_reply("tok-1", "alice")]);
    remote_server::login(&paths(dir.path()), input(&stub.base_url, "pw"), now()).unwrap();
    assert!(dir.path().join(TOKEN_NAME).exists());

    let dto = remote_server::logout(&paths(dir.path()), now()).unwrap();

    assert!(!dir.path().join(TOKEN_NAME).exists());
    assert_eq!(dto.state, SessionState::NotLoggedIn);
    assert_eq!(dto.account, "alice");
    assert!(dto.device_id.is_some());
    // 重复退出不报错。
    assert!(remote_server::logout(&paths(dir.path()), now()).is_ok());
}

// ---- 备份 ----

#[test]
fn backup_carries_neither_the_token_nor_the_device_identity() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();
    let stub = serve(vec![login_reply("tok-never-in-backup", "alice")]);
    remote_server::login(&paths(&live), input(&stub.base_url, "pw"), now()).unwrap();

    let backup_paths = backup::AppDataPaths {
        db_path: live.join("usage.sqlite"),
        prices_path: live.join("prices.json"),
        snapshot_path: live.join("litellm_prices.json"),
        budget_path: live.join("budget.json"),
        budget_notify_path: live.join("budget_notify_state.json"),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    };
    let conn = db_store::open_db(backup_paths.db_path.to_str().unwrap()).unwrap();
    let manifest = backup::backup_to(&conn, &dest, &backup_paths).unwrap();

    for name in [TOKEN_NAME, CONFIG_NAME] {
        assert!(
            !manifest.files.iter().any(|file| file == name),
            "备份清单不得包含 {name}：{manifest:?}"
        );
        assert!(!dest.join(name).exists(), "备份目录不得出现 {name}");
    }
    assert!(!all_file_text(&dest).contains("tok-never-in-backup"));
}

#[test]
fn restore_leaves_the_local_login_alone() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();
    let backup_paths = backup::AppDataPaths {
        db_path: live.join("usage.sqlite"),
        prices_path: live.join("prices.json"),
        snapshot_path: live.join("litellm_prices.json"),
        budget_path: live.join("budget.json"),
        budget_notify_path: live.join("budget_notify_state.json"),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    };
    let conn = db_store::open_db(backup_paths.db_path.to_str().unwrap()).unwrap();
    backup::backup_to(&conn, &dest, &backup_paths).unwrap();
    drop(conn);

    let stub = serve(vec![login_reply("tok-local", "alice")]);
    remote_server::login(&paths(&live), input(&stub.base_url, "pw"), now()).unwrap();
    // 即便有人把这两份文件塞进备份目录，恢复也不该读它们。
    std::fs::write(dest.join(TOKEN_NAME), r#"{"token":"tok-planted"}"#).unwrap();
    std::fs::write(dest.join(CONFIG_NAME), r#"{"device_id":"planted"}"#).unwrap();

    backup::restore_from(&dest, &backup_paths).unwrap();

    let text = all_file_text(&live);
    assert!(text.contains("tok-local"));
    assert!(!text.contains("tok-planted") && !text.contains("planted\""));
}
