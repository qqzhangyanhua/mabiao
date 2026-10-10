//! 所有对外请求的出口：统一在这里决定走不走代理。
//!
//! 之前每处都是裸的 `ureq::get/post`，而 ureq 不会自己读 `HTTPS_PROXY` 之类的环境变量，
//! 结果是必须走代理才能出网的用户，每一家 provider 都连不上。
//!
//! 解析顺序：应用数据目录下的 `network.json` > 环境变量。之所以两者都要——
//! 桌面应用是从图形界面启动的，拿不到用户在 shell profile 里 export 的变量，
//! 光靠环境变量对 GUI 场景等于没有。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const CONFIG_NAME: &str = "network.json";
const ENV_KEYS: [&str; 6] = [
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "HTTP_PROXY",
    "http_proxy",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// `http://host:port` / `socks5://host:port`，可带 `user:pass@`。
    /// 空串表示「显式不走代理」，用来盖掉环境变量。
    pub proxy: Option<String>,
}

pub fn config_path() -> PathBuf {
    crate::paths::app_data_dir().join(CONFIG_NAME)
}

pub fn load_config(path: &Path) -> NetworkConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_config(path: &Path, config: &NetworkConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// 配置文件里写了就以它为准（空串 = 明确不用代理）；没写才回落到环境变量。
pub fn resolve_proxy(
    config: &NetworkConfig,
    env: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    if let Some(configured) = config.proxy.as_deref() {
        let trimmed = configured.trim();
        return if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }
    ENV_KEYS.into_iter().find_map(|key| {
        env(key)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn current_proxy() -> Option<String> {
    resolve_proxy(&load_config(&config_path()), |key| std::env::var(key).ok())
}

/// 建一个带代理的 agent。代理串解析不了就退回直连——宁可直连成功，
/// 也不要因为配置写错让所有 provider 一起挂掉；错误在刷新时自然会暴露。
pub fn agent_with_timeout(timeout: Duration) -> ureq::Agent {
    build_agent(ureq::AgentBuilder::new().timeout(timeout))
}

/// 不跟随重定向。带密码或 token 的请求不能被服务端一个 307 悄悄带去别的地址
/// （尤其是 https 被降级成 http），出现跳转就让调用方报错。
/// `direct` 为真时不走代理：回环地址经代理转一圈没有意义，还会把「本机没开服务」变成代理的 502。
pub fn agent_without_redirects(timeout: Duration, direct: bool) -> ureq::Agent {
    let builder = ureq::AgentBuilder::new().timeout(timeout).redirects(0);
    if direct {
        builder.build()
    } else {
        build_agent(builder)
    }
}

fn build_agent(builder: ureq::AgentBuilder) -> ureq::Agent {
    match current_proxy().and_then(|raw| ureq::Proxy::new(raw).ok()) {
        Some(proxy) => builder.proxy(proxy).build(),
        None => builder.build(),
    }
}
