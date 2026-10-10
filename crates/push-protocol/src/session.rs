use serde::{Deserialize, Serialize};

use crate::device::DeviceInfo;
use crate::usage::UsageRecordPayload;

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
    /// 注入原文。只有 `injected` 层且源快照还在时才有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestViolation {
    /// `on_disk_possible` 层只许带条目、路径与体积。
    OnDiskContent { item_id: String },
    /// 来自缓存的清单没有原文，带了就是装成现场快照。
    CacheWithContent { item_id: String },
}

impl std::fmt::Display for ManifestViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OnDiskContent { item_id } => {
                write!(f, "on_disk_possible 条目 {item_id} 不得带原文")
            }
            Self::CacheWithContent { item_id } => {
                write!(f, "来自缓存的清单条目 {item_id} 不得带原文")
            }
        }
    }
}

impl std::error::Error for ManifestViolation {}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextManifestPayload {
    pub items: Vec<ContextItemPayload>,
    /// 「来自缓存，无原文」：源快照已被清理，条目只是摄取时写下的度量。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_cache: bool,
    /// 体积只是估算（Cursor）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub volume_is_estimate: bool,
}

impl ContextManifestPayload {
    /// 服务端收到后、客户端发出前都该调一次，守住 ADR 0026 的「推什么」边界。
    pub fn validate(&self) -> Result<(), ManifestViolation> {
        for item in self.items.iter().filter(|item| item.content.is_some()) {
            if item.layer == ContextLayer::OnDiskPossible {
                return Err(ManifestViolation::OnDiskContent {
                    item_id: item.id.clone(),
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
    fn on_disk_possible_must_not_carry_content() {
        let manifest = ContextManifestPayload {
            items: vec![
                item(ContextLayer::Injected, "a", Some("原文")),
                item(ContextLayer::OnDiskPossible, "b", Some("磁盘内容")),
            ],
            ..Default::default()
        };
        assert_eq!(
            manifest.validate(),
            Err(ManifestViolation::OnDiskContent {
                item_id: "b".into()
            })
        );
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
