use serde::{Deserialize, Serialize};

/// 当前线上格式版本。改了字段语义或删了字段就递增；只加可选字段不用。
pub const PROTOCOL_VERSION: u32 = 1;

/// 服务端仍接受的最低版本。
pub const MIN_SUPPORTED_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnsupportedVersion {
    pub client: u32,
    pub min_supported: u32,
    pub server: u32,
}

impl std::fmt::Display for UnsupportedVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "协议版本 {} 不被接受（服务端 {}，最低 {}）",
            self.client, self.server, self.min_supported
        )
    }
}

impl std::error::Error for UnsupportedVersion {}

/// 服务端对每个带 `protocol_version` 的请求（登录、推送）调用。比服务端新的版本也拒绝：
/// 服务端不认识的字段会被静默丢掉，不如让用户先升级服务端。
pub fn check_protocol_version(client: u32) -> Result<(), UnsupportedVersion> {
    if (MIN_SUPPORTED_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&client) {
        Ok(())
    } else {
        Err(UnsupportedVersion {
            client,
            min_supported: MIN_SUPPORTED_PROTOCOL_VERSION,
            server: PROTOCOL_VERSION,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_is_accepted() {
        assert_eq!(check_protocol_version(PROTOCOL_VERSION), Ok(()));
    }

    #[test]
    fn zero_and_newer_versions_are_rejected() {
        let too_old = check_protocol_version(MIN_SUPPORTED_PROTOCOL_VERSION - 1).unwrap_err();
        assert_eq!(too_old.min_supported, MIN_SUPPORTED_PROTOCOL_VERSION);
        assert!(check_protocol_version(PROTOCOL_VERSION + 1).is_err());
    }
}
