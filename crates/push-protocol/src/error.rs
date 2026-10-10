use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    /// 协议版本不兼容，客户端或服务端需要升级。
    UnsupportedProtocolVersion,
    InvalidCredentials,
    /// token 过期或无效，客户端提示重新登录。
    TokenExpired,
    InvalidPayload,
    /// 已登录但角色不够，或在访问别人的数据。
    Forbidden,
    NotFound,
    /// 与现有数据冲突，如账号名已被占用。
    Conflict,
    Internal,
}

/// 服务端所有非 2xx 响应的 JSON 体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ApiErrorCode,
    pub message: String,
}
