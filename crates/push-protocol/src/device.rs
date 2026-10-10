use serde::{Deserialize, Serialize};

/// 每台桌面端首次登录生成设备 ID，设备名可改。随每次推送携带。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: String,
    pub device_name: String,
}
