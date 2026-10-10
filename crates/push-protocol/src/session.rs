use serde::{Deserialize, Serialize};

use crate::device::DeviceInfo;
use crate::usage::UsageRecordPayload;
use crate::version::{check_protocol_version, UnsupportedVersion};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Message,
    Plan,
    ToolCall,
    ToolResult,
    ModelChange,
    Error,
    SystemStatus,
    Unadapted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventActor {
    User,
    Assistant,
    Tool,
}

/// 一条语义事件。`text` 与 `details` 已按内置规则打码。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventPayload {
    pub event_id: String,
    pub sequence: u32,
    pub source_file: String,
    pub source_sequence: u32,
    pub kind: EventKind,
    pub occurred_at: Option<String>,
    pub actor: Option<EventActor>,
    pub name: Option<String>,
    pub text: Option<String>,
    #[serde(default)]
    pub details: serde_json::Value,
}

/// 证据层级。`on_disk_possible` 是磁盘上现在的状态，不是会话当时的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextLayer {
    Injected,
    Observed,
    OnDiskPossible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    Tool,
    SystemStatus,
    Error,
    Skill,
    Instruction,
    Rule,
    McpServer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextLoadMode {
    Always,
    OnMatch,
    OnDemand,
    Manual,
    Observed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextInjectionStatus {
    Connected,
    Failed,
    AuthRequired,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextItemPayload {
    pub layer: ContextLayer,
    pub kind: ContextKind,
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_mode: Option<ContextLoadMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub injection_status: Option<ContextInjectionStatus>,
    /// 体积的权威值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_count: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_noise: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_unused_install: bool,
    /// 注入原文。只有 `injected` 层且源快照还在时才有；其它层带了会被 `validate` 拒绝。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestViolation {
    /// 只有 `injected` 层可带原文：`on_disk_possible` 是磁盘现状，`observed` 本就只是痕迹。
    ContentOnNonInjected {
        item_id: String,
        layer: ContextLayer,
    },
    /// 来自缓存的清单没有原文，带了就是装成现场快照。
    CacheWithContent { item_id: String },
}

impl std::fmt::Display for ManifestViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContentOnNonInjected { item_id, layer } => {
                write!(f, "{layer:?} 层条目 {item_id} 不得带原文")
            }
            Self::CacheWithContent { item_id } => {
                write!(f, "来自缓存的清单条目 {item_id} 不得带原文")
            }
        }
    }
}

impl std::error::Error for ManifestViolation {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextManifestPayload {
    pub items: Vec<ContextItemPayload>,
    /// 为 false 时没有会话当时的注入快照（Cursor 按当前磁盘重建），不得说成已注入。
    #[serde(default = "default_true")]
    pub has_injected_snapshot: bool,
    /// 「来自缓存，无原文」：源快照已被清理，条目只是摄取时写下的度量。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_cache: bool,
    /// 体积只是估算（Cursor）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub volume_is_estimate: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ContextManifestPayload {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            has_injected_snapshot: true,
            from_cache: false,
            volume_is_estimate: false,
        }
    }
}

impl ContextManifestPayload {
    /// 守住 ADR 0026 的「推什么」边界。`PushSessionRequest::validate` 会调它。
    pub fn validate(&self) -> Result<(), ManifestViolation> {
        for item in self.items.iter().filter(|item| item.content.is_some()) {
            if item.layer != ContextLayer::Injected {
                return Err(ManifestViolation::ContentOnNonInjected {
                    item_id: item.id.clone(),
                    layer: item.layer,
                });
            }
            if self.from_cache {
                return Err(ManifestViolation::CacheWithContent {
                    item_id: item.id.clone(),
                });
            }
        }
        Ok(())
    }
}

/// 单场会话。服务端按（远程账号, 设备, source, session_id）整场覆盖。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionPayload {
    pub source: String,
    pub session_id: String,
    pub title: String,
    pub project: String,
    /// 读 `project` 目录 `.git/config` 得到的 remote URL，读不到为空。服务端据此归并项目。
    #[serde(default)]
    pub git_remote_url: Option<String>,
    pub model: String,
    pub started_at: String,
    pub ended_at: String,
    pub source_files: Vec<String>,
    /// 工作纪要引擎自造的会话（「码表生成」标记）。
    #[serde(default)]
    pub generated_by_work_notes: bool,
    /// 本场正文与注入原文被打码的处数。
    #[serde(default)]
    pub redaction_count: u32,
    pub events: Vec<EventPayload>,
    #[serde(default)]
    pub context_manifest: Option<ContextManifestPayload>,
}

/// 每场会话一个请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushSessionRequest {
    pub protocol_version: u32,
    pub device: DeviceInfo,
    pub session: SessionPayload,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PushRequestError {
    UnsupportedVersion(UnsupportedVersion),
    Manifest(ManifestViolation),
}

impl std::fmt::Display for PushRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion(e) => e.fmt(f),
            Self::Manifest(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for PushRequestError {}

impl PushSessionRequest {
    /// 服务端收到请求后先调这个，再落库。
    pub fn validate(&self) -> Result<(), PushRequestError> {
        check_protocol_version(self.protocol_version)
            .map_err(PushRequestError::UnsupportedVersion)?;
        if let Some(manifest) = &self.session.context_manifest {
            manifest.validate().map_err(PushRequestError::Manifest)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushSessionResponse {
    pub source: String,
    pub session_id: String,
    /// true 表示覆盖了之前推过的同一场会话。
    pub replaced: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushUsageRequest {
    pub protocol_version: u32,
    pub device: DeviceInfo,
    pub records: Vec<UsageRecordPayload>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushUsageResponse {
    pub inserted: u32,
    /// 指纹已存在而被丢弃的条数。
    pub duplicates: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(layer: ContextLayer, id: &str, content: Option<&str>) -> ContextItemPayload {
        ContextItemPayload {
            layer,
            kind: ContextKind::Instruction,
            id: id.into(),
            label: id.into(),
            path: None,
            load_mode: None,
            injection_status: None,
            char_count: Some(10),
            is_noise: false,
            is_unused_install: false,
            content: content.map(str::to_string),
        }
    }

    #[test]
    fn injected_content_is_allowed() {
        let manifest = ContextManifestPayload {
            items: vec![item(ContextLayer::Injected, "a", Some("原文"))],
            ..Default::default()
        };
        assert_eq!(manifest.validate(), Ok(()));
    }

    #[test]
    fn non_injected_layers_must_not_carry_content() {
        let manifest = ContextManifestPayload {
            items: vec![
                item(ContextLayer::Injected, "a", Some("原文")),
                item(ContextLayer::OnDiskPossible, "b", Some("磁盘内容")),
            ],
            ..Default::default()
        };
        assert_eq!(
            manifest.validate(),
            Err(ManifestViolation::ContentOnNonInjected {
                item_id: "b".into(),
                layer: ContextLayer::OnDiskPossible,
            })
        );
        let observed = ContextManifestPayload {
            items: vec![item(ContextLayer::Observed, "c", Some("x"))],
            ..Default::default()
        };
        assert!(observed.validate().is_err());
    }

    fn request(manifest: Option<ContextManifestPayload>, version: u32) -> PushSessionRequest {
        PushSessionRequest {
            protocol_version: version,
            device: DeviceInfo {
                device_id: "d".into(),
                device_name: "n".into(),
            },
            session: SessionPayload {
                source: "codex".into(),
                session_id: "s".into(),
                title: String::new(),
                project: String::new(),
                git_remote_url: None,
                model: String::new(),
                started_at: String::new(),
                ended_at: String::new(),
                source_files: vec![],
                generated_by_work_notes: false,
                redaction_count: 0,
                events: vec![],
                context_manifest: manifest,
            },
        }
    }

    #[test]
    fn request_validate_checks_version_and_manifest() {
        use crate::version::PROTOCOL_VERSION;
        assert_eq!(request(None, PROTOCOL_VERSION).validate(), Ok(()));
        assert!(matches!(
            request(None, PROTOCOL_VERSION + 1).validate(),
            Err(PushRequestError::UnsupportedVersion(_))
        ));
        let bad = ContextManifestPayload {
            items: vec![item(ContextLayer::OnDiskPossible, "b", Some("x"))],
            ..Default::default()
        };
        assert!(matches!(
            request(Some(bad), PROTOCOL_VERSION).validate(),
            Err(PushRequestError::Manifest(_))
        ));
    }

    #[test]
    fn missing_has_injected_snapshot_defaults_to_true() {
        let manifest: ContextManifestPayload = serde_json::from_str(r#"{"items":[]}"#).unwrap();
        assert!(manifest.has_injected_snapshot);
    }

    #[test]
    fn cached_manifest_must_not_carry_content() {
        let manifest = ContextManifestPayload {
            items: vec![item(ContextLayer::Injected, "a", Some("原文"))],
            from_cache: true,
            ..Default::default()
        };
        assert_eq!(
            manifest.validate(),
            Err(ManifestViolation::CacheWithContent {
                item_id: "a".into()
            })
        );
    }

    #[test]
    fn cached_manifest_with_metrics_only_is_valid() {
        let manifest = ContextManifestPayload {
            items: vec![item(ContextLayer::Injected, "a", None)],
            from_cache: true,
            ..Default::default()
        };
        assert_eq!(manifest.validate(), Ok(()));
    }
}
