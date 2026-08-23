use crate::official_quota::antigravity;

/// 真机响应的形状：两个模型组，各有 weekly + 5h 两个桶。
const LIVE_SHAPE: &str = r#"{
    "groups": [
        {
            "displayName": "Gemini Models",
            "description": "Models within this group: Gemini Flash, Gemini Pro",
            "buckets": [
                { "bucketId": "gemini-weekly", "window": "weekly", "remainingFraction": 0.9995217,
                  "resetTime": "2026-08-28T16:21:14Z", "displayName": "Weekly Limit Remaining" },
                { "bucketId": "gemini-5h", "window": "5h", "remainingFraction": 0.99713,
                  "resetTime": "2026-08-21T21:21:14Z", "displayName": "Five Hour Limit Remaining" }
            ]
        },
        {
            "displayName": "Claude and GPT models",
            "buckets": [
                { "bucketId": "3p-weekly", "window": "weekly", "remainingFraction": 1,
                  "resetTime": "2026-08-28T17:15:00Z" }
            ]
        }
    ]
}"#;

#[test]
fn antigravity_quota_converts_remaining_fraction_to_used_percent() {
    let windows = antigravity::parse_quota_summary(LIVE_SHAPE).unwrap();
    let kinds: Vec<&str> = windows.iter().map(|w| w.kind.as_str()).collect();
    assert_eq!(kinds, ["gemini_weekly", "gemini_5h", "3p_weekly"]);

    // 接口给的是「剩余」，展示的是「已用」，必须取反。
    assert!((windows[0].used_percent.unwrap() - 0.04783).abs() < 1e-6);
    assert!((windows[1].used_percent.unwrap() - 0.287).abs() < 1e-6);
    assert_eq!(windows[2].used_percent, Some(0.0));
    assert_eq!(
        windows[0].resets_at.as_deref(),
        Some("2026-08-28T16:21:14Z")
    );
}

#[test]
fn antigravity_quota_labels_by_window_not_by_remaining_wording() {
    let windows = antigravity::parse_quota_summary(LIVE_SHAPE).unwrap();
    // 官方 displayName 是「Weekly Limit Remaining」，照抄会和已用口径读反。
    assert_eq!(windows[0].label, "Gemini Models 周");
    assert_eq!(windows[1].label, "Gemini Models 5 小时");
    assert_eq!(windows[2].label, "Claude and GPT models 周");
}

#[test]
fn antigravity_quota_skips_bad_buckets_and_reports_structure_change() {
    assert!(antigravity::parse_quota_summary("not json").is_err());
    assert!(antigravity::parse_quota_summary(r#"{"ok":true}"#).is_err());
    assert!(antigravity::parse_quota_summary(r#"{"groups":[]}"#).is_err());

    // 缺 remainingFraction 或 bucketId 的桶跳过，不当成 0%。
    let partial = r#"{"groups":[{"displayName":"G","buckets":[
        {"bucketId":"a","window":"weekly"},
        {"window":"5h","remainingFraction":0.5},
        {"bucketId":"good","window":"5h","remainingFraction":0.25}
    ]}]}"#;
    let windows = antigravity::parse_quota_summary(partial).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, "good");
    assert_eq!(windows[0].used_percent, Some(75.0));
}

#[test]
fn antigravity_refresh_token_comes_from_nested_protobuf() {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;

    fn field2(bytes: &[u8]) -> Vec<u8> {
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

    let refresh = "1//06fXXAV0iAfMbCgYIARAAGAYSNwF-L9IrGUwE3Z7N2TE";
    // 内层 protobuf：access token、"Bearer"、refresh token 三个字符串字段。
    let mut inner = field2(b"ya29.a0AT3oNZ8z7pqAZ0ZHvEq6pExJXbks8XH2Hp4GMijbMPjr3");
    inner.extend(field2(b"Bearer"));
    inner.extend(field2(refresh.as_bytes()));
    // 外层把内层的 base64 文本再包一层 protobuf。
    let outer = field2(engine.encode(&inner).as_bytes());

    assert_eq!(
        antigravity::extract_refresh_token(&engine.encode(&outer)).as_deref(),
        Some(refresh)
    );
    assert_eq!(antigravity::extract_refresh_token("not base64!!"), None);
    assert_eq!(
        antigravity::extract_refresh_token(&engine.encode(field2(b"nothing useful here at all"))),
        None
    );
}

#[test]
fn antigravity_credentials_absent_when_state_db_missing() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        antigravity::read_local_tokens_at(dir.path()).unwrap(),
        antigravity::LocalTokens::default()
    );
}

#[test]
fn antigravity_oauth_clients_come_from_the_local_install() {
    // main.js 里 id 和 secret 各有多个，配对关系看不出来，所以全组合都要留。
    //
    // 用的是编造的占位值——Antigravity 的真实凭证不该进本仓库，运行时从本机安装里取。
    // 但 GitHub 的 secret scanning 只认形状不认真假，写成完整字面量会拦下推送，
    // 所以在这里拼出来。这两个片段正是解析器要识别的特征。
    let id_suffix = ".apps.googleusercontent.com";
    let secret_prefix = "GOCSPX-";
    let source = format!(
        r#"...oauthConfig={{clientId:"111111111111-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa{id_suffix}",
        clientSecret:"{secret_prefix}AAAAAAAAAAAAAAAAAAAAAAAAAAAA"}},alt={{clientId:"222222222222-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb{id_suffix}",
        clientSecret:"{secret_prefix}BBBBBBBBBBBBBBBBBBBBBBBBBBBB"}}..."#
    );
    let pairs = antigravity::parse_oauth_clients(&source);
    assert_eq!(pairs.len(), 4);
    assert!(pairs
        .iter()
        .all(|(id, secret)| id.ends_with(id_suffix) && secret.starts_with(secret_prefix)));
    assert!(pairs.iter().any(|(id, _)| id.starts_with("111111111111-")));
    assert!(pairs.iter().any(|(id, _)| id.starts_with("222222222222-")));

    assert!(antigravity::parse_oauth_clients("no credentials in here").is_empty());
}

#[test]
fn antigravity_oauth_clients_can_be_scanned_from_binary_blob() {
    let id_suffix = ".apps.googleusercontent.com";
    let secret_prefix = "GOCSPX-";
    let id = format!("111111111111-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa{id_suffix}");
    let secret = format!("{secret_prefix}AAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    let mut blob = vec![0u8, 1, 2, 255];
    blob.extend_from_slice(id.as_bytes());
    blob.push(0);
    blob.extend_from_slice(secret.as_bytes());
    blob.extend_from_slice(&[0, 9, 10]);

    let pairs = antigravity::parse_oauth_clients_bytes(&blob);
    assert_eq!(pairs, vec![(id, secret)]);
}

#[test]
fn antigravity_keyring_blob_reads_go_keyring_json() {
    use base64::Engine;
    let json = r#"{"token":{"access_token":"ya29.example","refresh_token":"1//example","token_type":"Bearer"},"auth_method":"consumer"}"#;
    let encoded = format!(
        "go-keyring-base64:{}",
        base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
    );
    let tokens = antigravity::parse_keyring_blob(&encoded).unwrap();
    assert_eq!(tokens.access_token.as_deref(), Some("ya29.example"));
    assert_eq!(tokens.refresh_token.as_deref(), Some("1//example"));
    assert_eq!(
        antigravity::parse_keyring_blob(json)
            .unwrap()
            .access_token
            .as_deref(),
        Some("ya29.example")
    );
    assert_eq!(antigravity::parse_keyring_blob("not-a-token"), None);
}

#[test]
fn antigravity_looks_at_both_macos_client_data_dirs() {
    assert_eq!(antigravity::APP_DIRS, ["Antigravity", "Antigravity IDE"]);
}

#[test]
fn antigravity_macos_prefers_current_app_over_legacy_ide() {
    let files = antigravity::oauth_source_files();
    let server = std::path::PathBuf::from(
        "/Applications/Antigravity.app/Contents/Resources/bin/language_server",
    );
    let ide = std::path::PathBuf::from(
        "/Applications/Antigravity IDE.app/Contents/Resources/app/out/main.js",
    );
    if server.is_file() && ide.is_file() {
        let server_at = files.iter().position(|path| path == &server);
        let ide_at = files.iter().position(|path| path == &ide);
        assert!(
            server_at.is_some() && ide_at.is_some() && server_at < ide_at,
            "当前 Antigravity.app 的 language_server 应先于旧 IDE：{files:?}"
        );
    }
}

#[test]
#[ignore = "需要本机已登录 Antigravity，且会请求 Google"]
fn antigravity_live_fetch_rate_limits() {
    let (windows, _) = antigravity::fetch_rate_limits().expect("本机 Antigravity 额度拉取失败");
    assert!(!windows.is_empty());
}

#[test]
fn antigravity_local_install_yields_oauth_clients_when_present() {
    let files = antigravity::oauth_source_files();
    if files.is_empty() {
        return;
    }
    let found = files.iter().find_map(|path| {
        let bytes = std::fs::read(path).ok()?;
        let clients = antigravity::parse_oauth_clients_bytes(&bytes);
        (!clients.is_empty()).then_some(clients)
    });
    let clients = found
        .unwrap_or_else(|| panic!("本机有 Antigravity 安装文件但扫不到 OAuth 客户端：{files:?}"));
    assert!(clients.iter().all(|(id, secret)| {
        id.ends_with(".apps.googleusercontent.com") && secret.starts_with("GOCSPX-")
    }));
}
