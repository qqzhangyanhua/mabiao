//! 远程服务地址的校验与归一化（ADR 0026）。设置页在保存前就用它提示，登录与之后每次联网也再过一遍，
//! 所以手改配置文件也绕不过去。
//!
//! 强制 https；只有 `localhost` 与回环地址可以用 http，方便本机调试服务端。

use std::net::IpAddr;

use url::{Host, Url};

const EXAMPLE: &str = "https://mabiao.example.com";

/// 返回去掉结尾斜杠的规范地址（保留子路径，便于挂在反向代理的某个前缀下）。
pub fn normalize_base_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("请填写远程服务地址，例如 {EXAMPLE}"));
    }
    let url = Url::parse(trimmed)
        .map_err(|_| format!("地址格式不对，应类似 {EXAMPLE}（要带 https://）"))?;

    match url.scheme() {
        "https" => {}
        "http" => {
            if !is_loopback(&url) {
                return Err(
                    "必须使用 https:// 地址，明文 http 会让密码和对话内容暴露在网络上（只有 localhost 与回环地址例外）"
                        .to_string(),
                );
            }
        }
        _ => return Err(format!("地址要以 https:// 开头，例如 {EXAMPLE}")),
    }
    if url.host().is_none() {
        return Err(format!("地址里缺少主机名，例如 {EXAMPLE}"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("地址里不要带账号密码，请在下面的账号、密码栏填写".to_string());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("地址里不要带 ? 或 # 后面的参数".to_string());
    }

    let path = url.path().trim_end_matches('/');
    Ok(format!("{}{path}", url.origin().ascii_serialization()))
}

/// 已归一化的地址是否指向本机回环。解析不了按「不是」处理。
pub fn is_loopback_address(base_url: &str) -> bool {
    Url::parse(base_url).is_ok_and(|url| is_loopback(&url))
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        None => false,
    }
}
