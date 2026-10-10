use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use push_protocol::{ApiError, ApiErrorCode, PushRequestError, UnsupportedVersion};

/// 所有非 2xx 响应都是 `push_protocol::ApiError` 的 JSON 体。
#[derive(Debug)]
pub struct AppError {
    status: StatusCode,
    body: ApiError,
}

impl AppError {
    fn new(status: StatusCode, code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            body: ApiError {
                code,
                message: message.into(),
            },
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn code(&self) -> ApiErrorCode {
        self.body.code
    }

    /// 账号不存在、密码错、账号被停用都走这一条，不让调用方分辨。
    pub fn invalid_credentials() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            ApiErrorCode::InvalidCredentials,
            "账号或密码错误",
        )
    }

    pub fn token_expired() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            ApiErrorCode::TokenExpired,
            "登录已失效，请重新登录",
        )
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, ApiErrorCode::Forbidden, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, ApiErrorCode::NotFound, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, ApiErrorCode::Conflict, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidPayload,
            message,
        )
    }

    pub fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(%error, "内部错误");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::Internal,
            "服务端内部错误",
        )
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.body.message)
    }
}

impl std::error::Error for AppError {}

impl From<UnsupportedVersion> for AppError {
    fn from(error: UnsupportedVersion) -> Self {
        Self::new(
            StatusCode::UPGRADE_REQUIRED,
            ApiErrorCode::UnsupportedProtocolVersion,
            error.to_string(),
        )
    }
}

impl From<PushRequestError> for AppError {
    fn from(error: PushRequestError) -> Self {
        match error {
            PushRequestError::UnsupportedVersion(version) => version.into(),
            PushRequestError::Manifest(violation) => Self::invalid(violation.to_string()),
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

/// 请求体解析失败也返回 `ApiError`，不让 axum 默认的纯文本漏出去。
#[derive(axum::extract::FromRequest)]
#[from_request(via(axum::Json), rejection(AppError))]
pub struct ApiJson<T>(pub T);

/// 查询串解析失败同样返回 `ApiError`。
#[derive(axum::extract::FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(AppError))]
pub struct ApiQuery<T>(pub T);

impl From<axum::extract::rejection::QueryRejection> for AppError {
    fn from(rejection: axum::extract::rejection::QueryRejection) -> Self {
        Self::invalid(rejection.body_text())
    }
}

impl From<axum::extract::rejection::JsonRejection> for AppError {
    fn from(rejection: axum::extract::rejection::JsonRejection) -> Self {
        Self::invalid(rejection.body_text())
    }
}
