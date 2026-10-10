//! 管理接口与账号接口的响应形状。只有服务端与它自带的网页用，不进 `push-protocol`。

use chrono::{DateTime, Utc};
use push_protocol::RemoteRole;
use serde::{Deserialize, Serialize};

use crate::accounts::AccountRow;
use crate::devices::DeviceRow;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountView {
    pub id: i64,
    pub account: String,
    pub role: RemoteRole,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub deactivated_at: Option<DateTime<Utc>>,
}

impl From<AccountRow> for AccountView {
    /// 不带 `password_hash`。
    fn from(row: AccountRow) -> Self {
        Self {
            role: row.role(),
            id: row.id,
            account: row.account,
            active: row.active,
            created_at: row.created_at,
            deactivated_at: row.deactivated_at,
        }
    }
}

/// 管理员只能建成员；管理员账号走服务端命令行。
#[derive(Clone, Deserialize)]
pub struct CreateMemberRequest {
    pub account: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceView {
    pub device_id: String,
    pub device_name: String,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

impl From<DeviceRow> for DeviceView {
    fn from(row: DeviceRow) -> Self {
        Self {
            device_id: row.device_id,
            device_name: row.device_name,
            first_seen_at: row.first_seen_at,
            last_seen_at: row.last_seen_at,
        }
    }
}
