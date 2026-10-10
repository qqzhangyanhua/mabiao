use push_protocol::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

fn round_trip<T>(value: &T) -> T
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let wire = serde_json::to_string(value).unwrap();
    let back: T = serde_json::from_str(&wire).unwrap();
    assert_eq!(&back, value);
    back
}

fn device() -> DeviceInfo {
    DeviceInfo {
        device_id: "dev-1".into(),
        device_name: "alice 的 MacBook".into(),
    }
}

fn manifest() -> ContextManifestPayload {
    ContextManifestPayload {
        items: vec![
            ContextItemPayload {
                layer: ContextLayer::Injected,
                kind: ContextKind::Rule,
                id: "rule:1".into(),
                label: "workspace rules".into(),
                path: None,
                load_mode: Some(ContextLoadMode::Always),
                injection_status: None,
                char_count: Some(1200),
                is_noise: false,
                is_unused_install: false,
                content: Some("注入原文".into()),
            },
            ContextItemPayload {
                layer: ContextLayer::Injected,
                kind: ContextKind::McpServer,
                id: "mcp:figma".into(),
                label: "figma".into(),
                path: None,
                load_mode: Some(ContextLoadMode::Always),
                injection_status: Some(ContextInjectionStatus::AuthRequired),
                char_count: Some(80),
                is_noise: true,
                is_unused_install: true,
                content: None,
            },
            ContextItemPayload {
                layer: ContextLayer::OnDiskPossible,
                kind: ContextKind::Instruction,
                id: "disk:AGENTS.md".into(),
                label: "AGENTS.md".into(),
                path: Some("/repo/AGENTS.md".into()),
                load_mode: Some(ContextLoadMode::OnMatch),
                injection_status: None,
                char_count: Some(4096),
                is_noise: false,
                is_unused_install: false,
                content: None,
            },
        ],
        has_injected_snapshot: false,
        from_cache: false,
        volume_is_estimate: true,
    }
}

fn session() -> SessionPayload {
    SessionPayload {
        source: "cursor_agent".into(),
        session_id: "s-1".into(),
        title: "修一个 bug".into(),
        project: "/Users/alice/work/mabiao".into(),
        git_remote_url: Some("git@github.com:qqzhangyanhua/mabiao.git".into()),
        model: "claude-sonnet".into(),
        started_at: "2026-01-01T00:00:00Z".into(),
        ended_at: "2026-01-01T01:00:00Z".into(),
        source_files: vec!["/a.jsonl".into()],
        generated_by_work_notes: true,
        redaction_count: 3,
        events: vec![
            EventPayload {
                event_id: "e-1".into(),
                sequence: 0,
                source_file: "/a.jsonl".into(),
                source_sequence: 0,
                kind: EventKind::Message,
                occurred_at: Some("2026-01-01T00:00:01Z".into()),
                actor: Some(EventActor::User),
                name: None,
                text: Some("你好".into()),
                details: serde_json::Value::Null,
            },
            EventPayload {
                event_id: "e-2".into(),
                sequence: 1,
                source_file: "/a.jsonl".into(),
                source_sequence: 1,
                kind: EventKind::ToolCall,
                occurred_at: None,
                actor: Some(EventActor::Assistant),
                name: Some("Read".into()),
                text: None,
                details: json!({"path": "/x", "n": 2}),
            },
        ],
        context_manifest: Some(manifest()),
    }
}

#[test]
fn login_round_trip() {
    round_trip(&LoginRequest::new("alice", "pw"));
    round_trip(&LoginResponse {
        token: "t".into(),
        expires_at: "2026-02-01T00:00:00Z".into(),
        account: "alice".into(),
        role: RemoteRole::Admin,
    });
}

#[test]
fn login_request_carries_current_protocol_version() {
    assert_eq!(
        LoginRequest::new("a", "b").protocol_version,
        PROTOCOL_VERSION
    );
}

#[test]
fn roles_use_snake_case_on_the_wire() {
    assert_eq!(
        serde_json::to_string(&RemoteRole::Admin).unwrap(),
        "\"admin\""
    );
    assert_eq!(
        serde_json::to_string(&RemoteRole::Member).unwrap(),
        "\"member\""
    );
}

#[test]
fn push_session_round_trip_keeps_device_events_and_manifest() {
    let request = PushSessionRequest {
        protocol_version: PROTOCOL_VERSION,
        device: device(),
        session: session(),
    };
    let back = round_trip(&request);
    assert_eq!(back.device, device());
    assert_eq!(back.session.events.len(), 2);
    assert_eq!(back.session.redaction_count, 3);
    assert!(back.session.generated_by_work_notes);
    assert_eq!(back.session.context_manifest.unwrap().validate(), Ok(()));
}

#[test]
fn optional_session_fields_may_be_absent_on_the_wire() {
    let wire = json!({
        "protocol_version": PROTOCOL_VERSION,
        "device": {"device_id": "d", "device_name": "n"},
        "session": {
            "source": "codex", "session_id": "s", "title": "", "project": "",
            "model": "", "started_at": "", "ended_at": "",
            "source_files": [], "events": []
        }
    });
    let request: PushSessionRequest = serde_json::from_value(wire).unwrap();
    assert_eq!(request.session.git_remote_url, None);
    assert!(!request.session.generated_by_work_notes);
    assert_eq!(request.session.redaction_count, 0);
    assert!(request.session.context_manifest.is_none());
}

#[test]
fn cached_manifest_flag_survives_the_wire() {
    let manifest = ContextManifestPayload {
        from_cache: true,
        ..manifest()
    };
    let back = round_trip(&manifest);
    assert!(back.from_cache);
}

#[test]
fn push_usage_round_trip() {
    let tokens = UsageTokens {
        input: 10,
        output: 5,
        cache_read: 1,
        cache_creation: 2,
        reasoning: 3,
        total: 21,
    };
    let record = |pricing_source, cost_snapshot, native_cost| UsageRecordPayload {
        fingerprint: usage_fingerprint("codex", "/a", "2026-01-01T00:00:00Z", "gpt-5", &tokens),
        occurred_at: "2026-01-01T00:00:00Z".into(),
        source: "codex".into(),
        model: "gpt-5".into(),
        provider: "openai".into(),
        project: "/p".into(),
        session_id: "s".into(),
        source_file: "/a".into(),
        tokens,
        native_cost,
        cost_snapshot,
        pricing_source,
    };
    let request = PushUsageRequest {
        protocol_version: PROTOCOL_VERSION,
        device: device(),
        records: vec![
            record(PricingSource::Native, Some(0.5), Some(0.5)),
            record(PricingSource::Exact, Some(0.25), None),
            record(PricingSource::Fallback, Some(0.1), None),
            record(PricingSource::Unpriced, None, None),
        ],
    };
    round_trip(&request);
    round_trip(&PushUsageResponse {
        inserted: 3,
        duplicates: 1,
    });
}

#[test]
fn api_error_round_trip_and_wire_code() {
    let error = ApiError {
        code: ApiErrorCode::UnsupportedProtocolVersion,
        message: "升级".into(),
    };
    round_trip(&error);
    assert_eq!(
        serde_json::to_value(&error).unwrap()["code"],
        "unsupported_protocol_version"
    );
}
