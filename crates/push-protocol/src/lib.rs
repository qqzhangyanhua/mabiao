//! 码表推送的线上传输格式（ADR 0026）。
//!
//! 这是传输格式，不是平行数据模型：桌面端在推送时把 `domain` 转换成这里的类型，
//! 服务端直接反序列化它们。本 crate 不依赖桌面端，只依赖 serde 与 sha2。

pub mod auth;
pub mod device;
pub mod error;
pub mod session;
pub mod usage;
pub mod version;

pub use auth::{LoginRequest, LoginResponse, RemoteRole};
pub use device::DeviceInfo;
pub use error::{ApiError, ApiErrorCode};
pub use session::{
    ContextInjectionStatus, ContextItemPayload, ContextKind, ContextLayer, ContextLoadMode,
    ContextManifestPayload, EventActor, EventKind, EventPayload, ManifestViolation,
    PushSessionRequest, PushSessionResponse, PushUsageRequest, PushUsageResponse, SessionPayload,
};
pub use usage::{usage_fingerprint, PricingSource, UsageRecordPayload, UsageTokens};
pub use version::{
    check_protocol_version, UnsupportedVersion, MIN_SUPPORTED_PROTOCOL_VERSION, PROTOCOL_VERSION,
};
