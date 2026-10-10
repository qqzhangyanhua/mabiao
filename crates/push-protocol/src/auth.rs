use serde::{Deserialize, Serialize};

use crate::version::PROTOCOL_VERSION;

/// 远程账号角色。没有自助注册，账号由管理员创建。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteRole {
    Admin,
    Member,
}

/// 密码只用来换 token，客户端不落盘。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginRequest {
    pub protocol_version: u32,
    pub account: String,
    pub password: String,
}

impl LoginRequest {
    pub fn new(account: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            account: account.into(),
            password: password.into(),
        }
    }
}

impl std::fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginRequest")
            .field("protocol_version", &self.protocol_version)
            .field("account", &self.account)
            .field("password", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    /// RFC 3339。过期后客户端提示重新登录。
    pub expires_at: String,
    pub account: String,
    pub role: RemoteRole,
}

impl std::fmt::Debug for LoginResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginResponse")
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("account", &self.account)
            .field("role", &self.role)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_password_and_token() {
        let request = format!("{:?}", LoginRequest::new("alice", "hunter2"));
        assert!(request.contains("alice"));
        assert!(!request.contains("hunter2"));

        let response = format!(
            "{:?}",
            LoginResponse {
                token: "tok-secret".into(),
                expires_at: "2026-02-01T00:00:00Z".into(),
                account: "alice".into(),
                role: RemoteRole::Member,
            }
        );
        assert!(!response.contains("tok-secret"));
    }
}
