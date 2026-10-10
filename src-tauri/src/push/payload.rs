//! 把桌面端 `domain` 转成推送协议的传输类型，并在这里完成打码（ADR 0026、ADR 0017）。
//!
//! 协议类型是传输格式，不是平行数据模型：转换只在这一处，服务端直接反序列化协议类型。

use std::collections::BTreeSet;

use chrono::DateTime;
use pricing::{price_usage_cached, PriceCache, PricedUsage, PricingBasis};
use push_protocol as wire;
use push_protocol::{
    DeviceInfo, EventPayload, PushSessionRequest, SessionPayload, UsageRecordPayload, UsageTokens,
    PROTOCOL_VERSION,
};

use super::git_remote::git_remote_url;
use super::redact::redact;
use crate::conversation::PushSessionSource;
use crate::domain::{
    ConversationContextInjectionStatus as DomainStatus, ConversationContextKind as DomainKind,
    ConversationContextLayer as DomainLayer, ConversationContextLoadMode as DomainLoad,
    ConversationEvent, ConversationEventActor as DomainActor, ConversationEventKind as DomainEvent,
    PriceTable, UsageRecord,
};

pub struct BuiltSession {
    pub request: PushSessionRequest,
    pub event_count: u32,
    pub redactions: u32,
    /// 请求体序列化后的字节数，预览里的「预计大小」。
    pub bytes: u64,
}

pub fn build_session(source: PushSessionSource, device: &DeviceInfo) -> BuiltSession {
    let PushSessionSource {
        session,
        events,
        context,
    } = source;
    let mut redactions = 0u32;
    let mut take = |text: &str| {
        let (redacted, count) = redact(text);
        redactions += count;
        redacted
    };

    let title = take(&session.title);
    let events: Vec<EventPayload> = events
        .into_iter()
        .map(|event| event_payload(event, &mut take))
        .collect();
    let context_manifest = context.map(|context| {
        let manifest = context.manifest;
        wire::ContextManifestPayload {
            items: manifest
                .items
                .into_iter()
                .map(|item| {
                    let content = (item.layer == DomainLayer::Injected)
                        .then(|| context.injected_contents.get(&item.id))
                        .flatten()
                        .map(|content| take(content));
                    wire::ContextItemPayload {
                        layer: layer(item.layer),
                        kind: kind(item.kind),
                        id: item.id,
                        label: item.label,
                        path: item.path,
                        load_mode: item.load_mode.map(load_mode),
                        injection_status: item.injection_status.map(injection_status),
                        char_count: item.char_count,
                        is_noise: item.is_noise,
                        is_unused_install: item.is_unused_install,
                        content,
                    }
                })
                .collect(),
            has_injected_snapshot: manifest.has_injected_snapshot,
            from_cache: manifest.metrics_from_cache,
            volume_is_estimate: manifest.volume_is_estimate,
        }
    });

    let source_files = if session.source_files.is_empty() {
        vec![session.source_file.clone()]
    } else {
        session.source_files.clone()
    };
    let event_count = events.len() as u32;
    let request = PushSessionRequest {
        protocol_version: PROTOCOL_VERSION,
        device: device.clone(),
        session: SessionPayload {
            source: session.source.clone(),
            session_id: session.session_id.clone(),
            title,
            git_remote_url: git_remote_url(&session.project),
            project: session.project.clone(),
            model: session.model.clone(),
            started_at: session.started_at.clone(),
            ended_at: session.ended_at.clone(),
            source_files,
            generated_by_work_notes: session.generated_by_work_notes,
            redaction_count: redactions,
            events,
            context_manifest,
        },
    };
    let bytes = serde_json::to_vec(&request).map_or(0, |body| body.len() as u64);
    BuiltSession {
        request,
        event_count,
        redactions,
        bytes,
    }
}

fn event_payload(event: ConversationEvent, take: &mut impl FnMut(&str) -> String) -> EventPayload {
    EventPayload {
        event_id: event.event_id,
        sequence: event.sequence,
        source_file: event.source_file,
        source_sequence: event.source_sequence,
        kind: event_kind(event.kind),
        occurred_at: event.occurred_at,
        actor: event.actor.map(actor),
        name: event.name,
        text: event.text.map(|text| take(&text)),
        // 原始载荷不出本机：推送只带语义事件的正文。
        details: serde_json::Value::Null,
    }
}

fn event_kind(kind: DomainEvent) -> wire::EventKind {
    match kind {
        DomainEvent::Message => wire::EventKind::Message,
        DomainEvent::Plan => wire::EventKind::Plan,
        DomainEvent::ToolCall => wire::EventKind::ToolCall,
        DomainEvent::ToolResult => wire::EventKind::ToolResult,
        DomainEvent::ModelChange => wire::EventKind::ModelChange,
        DomainEvent::Error => wire::EventKind::Error,
        DomainEvent::SystemStatus => wire::EventKind::SystemStatus,
        DomainEvent::Unadapted => wire::EventKind::Unadapted,
    }
}

fn actor(actor: DomainActor) -> wire::EventActor {
    match actor {
        DomainActor::User => wire::EventActor::User,
        DomainActor::Assistant => wire::EventActor::Assistant,
        DomainActor::Tool => wire::EventActor::Tool,
    }
}

fn layer(layer: DomainLayer) -> wire::ContextLayer {
    match layer {
        DomainLayer::Injected => wire::ContextLayer::Injected,
        DomainLayer::Observed => wire::ContextLayer::Observed,
        DomainLayer::OnDiskPossible => wire::ContextLayer::OnDiskPossible,
    }
}

fn kind(kind: DomainKind) -> wire::ContextKind {
    match kind {
        DomainKind::Tool => wire::ContextKind::Tool,
        DomainKind::SystemStatus => wire::ContextKind::SystemStatus,
        DomainKind::Error => wire::ContextKind::Error,
        DomainKind::Skill => wire::ContextKind::Skill,
        DomainKind::Instruction => wire::ContextKind::Instruction,
        DomainKind::Rule => wire::ContextKind::Rule,
        DomainKind::McpServer => wire::ContextKind::McpServer,
    }
}

fn load_mode(mode: DomainLoad) -> wire::ContextLoadMode {
    match mode {
        DomainLoad::Always => wire::ContextLoadMode::Always,
        DomainLoad::OnMatch => wire::ContextLoadMode::OnMatch,
        DomainLoad::OnDemand => wire::ContextLoadMode::OnDemand,
        DomainLoad::Manual => wire::ContextLoadMode::Manual,
        DomainLoad::Observed => wire::ContextLoadMode::Observed,
    }
}

fn injection_status(status: DomainStatus) -> wire::ContextInjectionStatus {
    match status {
        DomainStatus::Connected => wire::ContextInjectionStatus::Connected,
        DomainStatus::Failed => wire::ContextInjectionStatus::Failed,
        DomainStatus::AuthRequired => wire::ContextInjectionStatus::AuthRequired,
        DomainStatus::Disabled => wire::ContextInjectionStatus::Disabled,
    }
}

pub struct BuiltUsage {
    pub records: Vec<UsageRecordPayload>,
    /// `occurred_at` 不是 RFC 3339 而没推的条数。服务端对这种记录整批拒收，所以只能在本机挡掉。
    pub skipped_invalid_time: u32,
}

/// 费用快照与定价来源走共享计价（`pricing::price_usage_cached`），不另写规则。
pub fn build_usage(records: &[UsageRecord], prices: &PriceTable) -> BuiltUsage {
    let mut cache = PriceCache::new(prices);
    let mut seen = BTreeSet::new();
    let mut skipped_invalid_time = 0u32;
    let mut payloads = Vec::with_capacity(records.len());
    for record in records {
        if DateTime::parse_from_rfc3339(&record.occurred_at).is_err() {
            skipped_invalid_time += 1;
            continue;
        }
        let tokens = UsageTokens {
            input: record.input_tokens,
            output: record.output_tokens,
            cache_read: record.cache_read_tokens,
            cache_creation: record.cache_creation_tokens,
            reasoning: record.reasoning_tokens,
            total: record.total_tokens,
        };
        let fingerprint = wire::usage_fingerprint(
            record.source.as_str(),
            &record.source_file,
            &record.occurred_at,
            &record.model,
            &tokens,
        );
        // 同一指纹服务端本来就会丢；本机先去重，「已存在」的计数才只反映以前推过的。
        if !seen.insert(fingerprint.clone()) {
            continue;
        }
        let priced = price_usage_cached(
            &mut cache,
            PricedUsage {
                model: &record.model,
                provider: &record.provider,
                input_tokens: record.input_tokens,
                output_tokens: record.output_tokens,
                cache_read_tokens: record.cache_read_tokens,
                cache_creation_tokens: record.cache_creation_tokens,
                native_cost: record.native_cost,
            },
            false,
        );
        payloads.push(UsageRecordPayload {
            fingerprint,
            occurred_at: record.occurred_at.clone(),
            source: record.source.as_str().to_string(),
            model: record.model.clone(),
            provider: record.provider.clone(),
            project: record.project.clone(),
            session_id: record.session_id.clone(),
            source_file: record.source_file.clone(),
            tokens,
            native_cost: record.native_cost,
            cost_snapshot: priced.derived.amount,
            pricing_source: pricing_source(priced.basis),
        });
    }
    BuiltUsage {
        records: payloads,
        skipped_invalid_time,
    }
}

fn pricing_source(basis: PricingBasis) -> wire::PricingSource {
    match basis {
        PricingBasis::Native => wire::PricingSource::Native,
        PricingBasis::Exact => wire::PricingSource::Exact,
        PricingBasis::Fallback => wire::PricingSource::Fallback,
        PricingBasis::Unpriced => wire::PricingSource::Unpriced,
    }
}
