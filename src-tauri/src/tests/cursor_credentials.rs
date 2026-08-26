use base64::Engine;
use rusqlite::Connection;
use std::path::Path;

use crate::cursor_credentials::{
    build_session_token, expires_at_ms, read_credential_at, LocalCredential,
};

fn jwt(payload: &str) -> String {
    let encode =
        |raw: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes());
    format!(
        "{}.{}.{}",
        encode(r#"{"alg":"HS256","typ":"JWT"}"#),
        encode(payload),
        "signature"
    )
}

/// 造一个和 Cursor 同构的 globalStorage。真机上 value 是 TEXT，
/// `as_blob` 用来覆盖存成 BLOB 的版本——两种都必须能读出来。
fn seed_state_db(dir: &Path, items: &[(&str, &str)], as_blob: bool) {
    let conn = Connection::open(dir.join("state.vscdb")).unwrap();
    conn.execute(
        "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)",
        [],
    )
    .unwrap();
    for (key, value) in items {
        let bound: rusqlite::types::Value = if as_blob {
            rusqlite::types::Value::Blob(value.as_bytes().to_vec())
        } else {
            rusqlite::types::Value::Text((*value).to_string())
        };
        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params![key, bound],
        )
        .unwrap();
    }
}

#[test]
fn local_credential_builds_cursor_cookie_value_from_jwt_sub() {
    let token = jwt(r#"{"sub":"google-oauth|user_01JABC","exp":1791470635}"#);
    assert_eq!(
        build_session_token(&token).unwrap(),
        format!("user_01JABC%3A%3A{token}")
    );
    assert_eq!(expires_at_ms(&token), Some(1_791_470_635_000));
}

#[test]
fn local_credential_accepts_sub_without_provider_prefix() {
    let token = jwt(r#"{"sub":"user_01PLAIN"}"#);
    assert_eq!(
        build_session_token(&token).unwrap(),
        format!("user_01PLAIN%3A%3A{token}")
    );
    assert_eq!(expires_at_ms(&token), None);
}

#[test]
fn local_credential_rejects_malformed_token() {
    assert!(build_session_token("not-a-jwt").is_err());
    assert!(build_session_token(&jwt(r#"{"exp":1}"#)).is_err());
    assert!(build_session_token(&jwt(r#"{"sub":"google-oauth|"}"#)).is_err());
}

#[test]
fn local_credential_reads_token_email_and_membership_from_state_db() {
    let token = jwt(r#"{"sub":"google-oauth|user_01JABC","exp":1791470635}"#);
    // 真机是 TEXT，旧版本可能是 BLOB，两种都得读出同样的结果。
    for as_blob in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        seed_state_db(
            dir.path(),
            &[
                ("cursorAuth/accessToken", token.as_str()),
                ("cursorAuth/cachedEmail", "dev@example.com"),
                ("cursorAuth/stripeMembershipType", "pro"),
                ("someOtherExtension/state", "ignored"),
            ],
            as_blob,
        );

        let credential = read_credential_at(dir.path()).unwrap().unwrap();
        assert_eq!(
            credential.session_token,
            format!("user_01JABC%3A%3A{token}")
        );
        assert_eq!(credential.email.as_deref(), Some("dev@example.com"));
        assert_eq!(credential.membership.as_deref(), Some("pro"));
        assert_eq!(credential.expires_at_ms, Some(1_791_470_635_000));
        assert!(credential.expires_at_rfc3339().is_some());
    }
}

#[test]
fn local_credential_absent_when_db_or_key_missing() {
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(read_credential_at(empty.path()).unwrap(), None);

    let no_key = tempfile::tempdir().unwrap();
    seed_state_db(
        no_key.path(),
        &[("cursorAuth/cachedEmail", "dev@example.com")],
        false,
    );
    assert_eq!(read_credential_at(no_key.path()).unwrap(), None);

    let blank = tempfile::tempdir().unwrap();
    seed_state_db(blank.path(), &[("cursorAuth/accessToken", "   ")], false);
    assert_eq!(read_credential_at(blank.path()).unwrap(), None);
}

#[test]
fn local_credential_expiry_uses_skew_and_tolerates_missing_exp() {
    let with_exp = |exp_ms: Option<i64>| LocalCredential {
        session_token: "user%3A%3Atoken".to_string(),
        email: None,
        membership: None,
        expires_at_ms: exp_ms,
    };
    let now = 1_000_000_000_000;
    assert!(!with_exp(Some(now + 300_000)).is_expired_at(now));
    // 落在 60s 容差窗里就当过期，别让一次刷新拉到一半失效。
    assert!(with_exp(Some(now + 30_000)).is_expired_at(now));
    assert!(with_exp(Some(now - 1)).is_expired_at(now));
    assert!(!with_exp(None).is_expired_at(now));
}

/// 联网冒烟：接口是逆向的，Cursor 随时可能改掉 cookie 形状。默认不跑，
/// 怀疑这条链路挂了就手动执行：
/// `cargo test --manifest-path src-tauri/Cargo.toml local_credential_smoke -- --ignored --nocapture`
#[test]
#[ignore = "需要本机装了 Cursor 并已登录，且会真的请求 cursor.com"]
fn local_credential_smoke_hits_cursor_usage_summary() {
    let credential = crate::cursor_credentials::read_local_credential()
        .expect("本机没读到 Cursor 登录态，先在 Cursor 客户端登录");
    assert!(!credential.is_expired(), "本机 Cursor 登录态已过期");
    assert_eq!(
        crate::cursor_account::current_token().unwrap(),
        credential.session_token
    );

    let snapshot = crate::official_quota::cursor::fetch_usage_summary()
        .expect("用本机登录态请求 cursor.com 失败");
    assert!(!snapshot.windows.is_empty());
    println!(
        "cursor windows: {:?} plan: {:?}",
        snapshot.windows.iter().map(|w| &w.kind).collect::<Vec<_>>(),
        snapshot.plan
    );
}
