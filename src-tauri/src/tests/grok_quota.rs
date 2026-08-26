use crate::official_quota;

#[test]
fn grok_credits_parse_weekly_and_build() {
    let raw = r#"{
        "config": {
            "creditUsagePercent": 34.0,
            "currentPeriod": {
                "type": "USAGE_PERIOD_TYPE_WEEKLY",
                "end": "2026-08-05T01:12:18.000Z"
            },
            "productUsage": [{ "product": "GrokBuild", "usagePercent": 45.0 }]
        }
    }"#;
    let windows = official_quota::grok::parse_credits(raw).unwrap();
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].kind, "weekly");
    assert_eq!(windows[0].label, "周额度");
    assert_eq!(windows[0].used_percent, Some(34.0));
    assert_eq!(
        windows[0].resets_at.as_deref(),
        Some("2026-08-05T01:12:18.000Z")
    );
    assert_eq!(windows[1].kind, "product_grokbuild");
    assert_eq!(windows[1].label, "Grok Build");
    assert_eq!(windows[1].used_percent, Some(45.0));
}

#[test]
fn grok_credits_use_build_percent_when_weekly_missing() {
    let raw = r#"{
        "config": {
            "currentPeriod": {
                "type": "USAGE_PERIOD_TYPE_WEEKLY",
                "end": "2026-08-05T01:12:18.000Z"
            },
            "productUsage": [{ "product": "GrokBuild", "usagePercent": 12.5 }]
        }
    }"#;
    let windows = official_quota::grok::parse_credits(raw).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, "weekly");
    assert_eq!(windows[0].used_percent, Some(12.5));
}

#[test]
fn grok_credits_treat_empty_weekly_period_as_zero() {
    let raw = r#"{
        "config": {
            "currentPeriod": {
                "type": "USAGE_PERIOD_TYPE_WEEKLY",
                "start": "2026-07-29T01:12:18.000Z",
                "end": "2026-08-05T01:12:18.000Z"
            }
        }
    }"#;
    let windows = official_quota::grok::parse_credits(raw).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, "weekly");
    assert_eq!(windows[0].used_percent, Some(0.0));
}

#[test]
fn grok_credits_skip_zero_on_demand_cap() {
    let raw = r#"{
        "config": {
            "creditUsagePercent": 10,
            "onDemandUsed": { "val": 0 },
            "onDemandCap": { "val": 0 }
        }
    }"#;
    let windows = official_quota::grok::parse_credits(raw).unwrap();
    assert_eq!(windows.len(), 1);
    assert!(windows.iter().all(|window| window.kind != "on_demand"));
}

#[test]
fn grok_credits_parse_on_demand_when_cap_present() {
    let raw = r#"{
        "config": {
            "creditUsagePercent": 10,
            "onDemandUsed": { "val": 250 },
            "onDemandCap": { "val": 1000 }
        }
    }"#;
    let windows = official_quota::grok::parse_credits(raw).unwrap();
    let on_demand = windows
        .iter()
        .find(|window| window.kind == "on_demand")
        .unwrap();
    assert_eq!(on_demand.label, "按需");
    assert_eq!(on_demand.used_percent, Some(25.0));
}

#[test]
fn grok_monthly_parses_used_limit_wrappers() {
    let raw = r#"{
        "config": {
            "used": { "val": 2000 },
            "monthlyLimit": { "val": 8000 },
            "billingPeriodEnd": "2026-09-01T00:00:00Z"
        }
    }"#;
    let windows = official_quota::grok::parse_monthly(raw).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, "monthly");
    assert_eq!(windows[0].label, "月额度");
    assert_eq!(windows[0].used_percent, Some(25.0));
    assert_eq!(
        windows[0].resets_at.as_deref(),
        Some("2026-09-01T00:00:00Z")
    );
}

#[test]
fn grok_monthly_skips_when_used_missing() {
    let raw = r#"{ "config": { "monthlyLimit": { "val": 8000 } } }"#;
    assert!(official_quota::grok::parse_monthly(raw).unwrap().is_empty());
}

#[test]
fn grok_settings_plan_reads_subscription_tier_display() {
    assert_eq!(
        official_quota::grok::parse_settings_plan(
            r#"{"subscription_tier_display":"SuperGrok Heavy"}"#
        )
        .as_deref(),
        Some("SuperGrok Heavy")
    );
    assert_eq!(
        official_quota::grok::parse_settings_plan(
            r#"{"config":{"subscription_tier":"supergrok"}}"#
        )
        .as_deref(),
        Some("SuperGrok")
    );
    assert!(official_quota::grok::parse_settings_plan(r#"{"ok":true}"#).is_none());
}

#[test]
fn grok_jwt_plan_maps_numeric_tier() {
    fn jwt(payload: &str) -> String {
        use base64::Engine;
        let encode =
            |raw: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes());
        format!("{}.{}.sig", encode(r#"{"alg":"none"}"#), encode(payload))
    }
    assert_eq!(
        official_quota::grok::parse_jwt_plan(&jwt(r#"{"tier":1}"#)).as_deref(),
        Some("SuperGrok")
    );
    assert_eq!(
        official_quota::grok::parse_jwt_plan(&jwt(r#"{"tier":5}"#)).as_deref(),
        Some("SuperGrok Heavy")
    );
    assert_eq!(
        official_quota::grok::parse_jwt_plan(&jwt(r#"{"tier":"x_premium_plus"}"#)).as_deref(),
        Some("X Premium+")
    );
    assert!(official_quota::grok::parse_jwt_plan("not-a-jwt").is_none());
}

#[test]
fn grok_rejects_leaked_percent_and_unknown_shape() {
    let leaked = r#"{ "config": { "creditUsagePercent": 1776950400 } }"#;
    assert!(official_quota::grok::parse_credits(leaked)
        .unwrap()
        .is_empty());
    assert!(official_quota::grok::parse_credits(r#"{"ok":true}"#)
        .unwrap()
        .is_empty());
}

#[test]
fn grok_auth_prefers_supergrok_scope_and_skips_expired() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-19T00:00:00+00:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let raw = r#"{
        "https://auth.x.ai::openid": {
            "key": "supergrok-token",
            "auth_mode": "oidc",
            "expires_at": "2026-08-20T00:00:00+00:00"
        },
        "https://accounts.x.ai/sign-in": {
            "key": "legacy-token",
            "auth_mode": "oidc",
            "expires_at": "2026-08-20T00:00:00+00:00"
        }
    }"#;
    let session = official_quota::grok::parse_auth_json(raw, now).unwrap();
    assert_eq!(session.token, "supergrok-token");
    assert_eq!(session.user_id, None);

    let expired = r#"{
        "https://auth.x.ai::openid": {
            "key": "old",
            "auth_mode": "oidc",
            "expires_at": "2026-08-18T00:00:00+00:00"
        }
    }"#;
    let error = official_quota::grok::parse_auth_json(expired, now).unwrap_err();
    assert!(error.contains("已过期"));
}

#[test]
fn grok_auth_rejects_api_key_and_weblogin() {
    let now = chrono::Utc::now();
    let api_key = r#"{
        "xai::api_key": { "key": "xai-secret", "auth_mode": "api_key" }
    }"#;
    let error = official_quota::grok::parse_auth_json(api_key, now).unwrap_err();
    assert!(error.contains("会话登录"));

    let web_login = r#"{
        "https://accounts.x.ai/sign-in": { "key": "legacy-web", "auth_mode": "web_login" }
    }"#;
    let error = official_quota::grok::parse_auth_json(web_login, now).unwrap_err();
    assert!(error.contains("无效"));
}

#[test]
fn grok_auth_keeps_expired_session_with_refresh_credentials() {
    // 过期但带 refresh_token/oidc_issuer/oidc_client_id 的会话不该直接报错，
    // 得留给 fetch_rate_limits 现刷——否则就要用户手动跑一次 grok CLI 才能续上。
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-23T09:00:00+00:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let raw = r#"{
        "https://auth.x.ai::openid": {
            "key": "stale-token",
            "auth_mode": "oidc",
            "expires_at": "2026-08-23T08:00:00+00:00",
            "refresh_token": "refresh-abc",
            "oidc_issuer": "https://auth.x.ai",
            "oidc_client_id": "client-123"
        }
    }"#;
    let session = official_quota::grok::parse_auth_json(raw, now).unwrap();
    assert_eq!(session.token, "stale-token");
    assert!(session.expired);
    let refresh = session.refresh.expect("refresh credentials should be kept");
    assert_eq!(refresh.refresh_token, "refresh-abc");
    assert_eq!(refresh.oidc_issuer, "https://auth.x.ai");
    assert_eq!(refresh.client_id, "client-123");
}

#[test]
fn grok_auth_prefers_valid_session_over_expired_refreshable() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-23T09:00:00+00:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let raw = r#"{
        "https://accounts.x.ai/sign-in": {
            "key": "legacy-token",
            "auth_mode": "oidc",
            "expires_at": "2026-08-23T08:00:00+00:00",
            "refresh_token": "refresh-abc",
            "oidc_issuer": "https://auth.x.ai",
            "oidc_client_id": "client-123"
        },
        "https://auth.x.ai::openid": {
            "key": "fresh-token",
            "auth_mode": "oidc",
            "expires_at": "2026-08-23T10:00:00+00:00"
        }
    }"#;
    let session = official_quota::grok::parse_auth_json(raw, now).unwrap();
    assert_eq!(session.token, "fresh-token");
    assert!(!session.expired);
}

#[test]
fn grok_auth_still_reports_expired_without_refresh_token() {
    // 没有 refresh_token 的过期会话（比如老版本 CLI 写的凭证）保持原来的报错行为。
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-23T09:00:00+00:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let raw = r#"{
        "https://auth.x.ai::openid": {
            "key": "stale-token",
            "auth_mode": "oidc",
            "expires_at": "2026-08-23T08:00:00+00:00"
        }
    }"#;
    let error = official_quota::grok::parse_auth_json(raw, now).unwrap_err();
    assert!(error.contains("已过期"));
}

#[test]
fn grok_parses_refreshed_access_token_response() {
    assert_eq!(
        official_quota::grok::parse_refreshed_access_token(
            r#"{"access_token":"new-token","expires_in":3600}"#
        )
        .unwrap(),
        "new-token"
    );
    assert!(official_quota::grok::parse_refreshed_access_token(r#"{"expires_in":3600}"#).is_err());
}

#[test]
fn grok_auth_reads_user_id_from_session() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-19T00:00:00+00:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let raw = r#"{
        "https://auth.x.ai::openid": {
            "key": "supergrok-token",
            "auth_mode": "oidc",
            "user_id": "user-123",
            "expires_at": "2026-08-20T00:00:00+00:00"
        }
    }"#;
    let session = official_quota::grok::parse_auth_json(raw, now).unwrap();
    assert_eq!(session.token, "supergrok-token");
    assert_eq!(session.user_id.as_deref(), Some("user-123"));
}

#[test]
fn grok_user_response_reads_camel_case_user_id() {
    assert_eq!(
        official_quota::grok::parse_user_id_response(r#"{"userId":"mock-user","email":"a@b.c"}"#)
            .unwrap(),
        "mock-user"
    );
    assert!(official_quota::grok::parse_user_id_response(r#"{"email":"a@b.c"}"#).is_err());
}

#[test]
fn grok_rest_serialize_error_falls_back_to_grpc() {
    assert!(official_quota::grok_grpc::should_fallback_to_grpc(
        "拉取 Grok 限额失败：Failed to serialize billing response"
    ));
    assert!(official_quota::grok_grpc::should_fallback_to_grpc(
        "拉取 Grok 限额失败：HTTP 500"
    ));
    assert!(!official_quota::grok_grpc::should_fallback_to_grpc(
        "Grok 登录已过期，请重新运行 grok login"
    ));
}

#[test]
fn grok_grpc_parses_ratio_and_reset() {
    let inner = {
        let mut body = vec![0x0d];
        body.extend_from_slice(&0.425f32.to_le_bytes());
        let mut timestamp = vec![0x08];
        timestamp.extend(encode_varint(1_800_000_000));
        body.push(0x2a);
        body.extend(encode_varint(timestamp.len() as u64));
        body.extend(timestamp);
        body
    };
    let mut payload = vec![0x0a];
    payload.extend(encode_varint(inner.len() as u64));
    payload.extend(inner);
    let mut framed = vec![0x00];
    framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    framed.extend(payload);

    let windows = official_quota::grok_grpc::parse_credits_grpc(&framed, 1_700_000_000).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, "weekly");
    assert!((windows[0].used_percent.unwrap() - 42.5).abs() < 0.01);
    assert!(windows[0]
        .resets_at
        .as_deref()
        .unwrap()
        .starts_with("2027-01-15"));
}

fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
    out
}
