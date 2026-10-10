use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use push_protocol::RemoteRole;

use crate::error::AppError;
use crate::tokens;
use crate::AppState;

/// 已通过 token 鉴权的远程账号。作为 handler 参数即要求登录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthedAccount {
    pub id: i64,
    pub account: String,
    pub role: RemoteRole,
}

impl AuthedAccount {
    pub fn is_admin(&self) -> bool {
        self.role == RemoteRole::Admin
    }

    /// 数据隔离的唯一判定：成员只能碰自己的，管理员可碰全体。
    /// 每个按账号归属的数据接口都先过这里，不各写各的。
    pub fn can_access(&self, owner_account_id: i64) -> bool {
        self.is_admin() || self.id == owner_account_id
    }
}

fn bearer_token(parts: &Parts) -> Option<&str> {
    let value = parts.headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()).then(|| token.trim())
}

impl FromRequestParts<AppState> for AuthedAccount {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts).ok_or_else(AppError::token_expired)?;
        tokens::authenticate(&state.pool, token)
            .await?
            .ok_or_else(AppError::token_expired)
    }
}

/// 只放管理员过的 handler 参数。成员得到 403。
#[derive(Debug, Clone)]
pub struct AdminAccount(pub AuthedAccount);

impl FromRequestParts<AppState> for AdminAccount {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let account = AuthedAccount::from_request_parts(parts, state).await?;
        if account.is_admin() {
            Ok(Self(account))
        } else {
            Err(AppError::forbidden("需要管理员权限"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: i64, role: RemoteRole) -> AuthedAccount {
        AuthedAccount {
            id,
            account: format!("u{id}"),
            role,
        }
    }

    #[test]
    fn member_reaches_only_own_data() {
        let member = account(1, RemoteRole::Member);
        assert!(member.can_access(1));
        assert!(!member.can_access(2));
    }

    #[test]
    fn admin_reaches_everyone() {
        let admin = account(1, RemoteRole::Admin);
        assert!(admin.can_access(1));
        assert!(admin.can_access(2));
    }

    #[test]
    fn bearer_header_parsing() {
        let parse = |value: Option<&str>| {
            let mut builder = axum::http::Request::builder();
            if let Some(value) = value {
                builder = builder.header(AUTHORIZATION, value);
            }
            let (parts, _) = builder.body(()).unwrap().into_parts();
            bearer_token(&parts).map(str::to_owned)
        };
        assert_eq!(parse(Some("Bearer abc")), Some("abc".into()));
        assert_eq!(parse(Some("bearer abc")), Some("abc".into()));
        assert_eq!(parse(Some("Basic abc")), None);
        assert_eq!(parse(Some("Bearer ")), None);
        assert_eq!(parse(Some("Bearer")), None);
        assert_eq!(parse(None), None);
    }
}
