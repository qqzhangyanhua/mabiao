//! 本机推送历史：时间、区间、成功 / 失败数（ADR 0026「推送流程」）。
//!
//! 只记结果数字，不记正文。文件在应用数据目录，不在备份白名单里：它描述的是「这台机器推过什么」。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

const MAX_ENTRIES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushHistoryEntry {
    /// RFC 3339，推送结束的时间。
    pub at: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub sources: Vec<String>,
    pub sessions_succeeded: u32,
    pub sessions_failed: u32,
    pub sessions_skipped: u32,
    pub usage_inserted: u32,
    pub usage_duplicates: u32,
    /// 消耗记录这一步是否失败（会话与消耗是分开发的）。
    #[serde(default)]
    pub usage_failed: bool,
    /// 每日自动推送发起的（否则是用户手动推的）。
    #[serde(default)]
    pub automatic: bool,
}

/// 新的在前。读不动或解析不了按「没有历史」处理：历史只是回看用，不该挡住推送。
pub fn load(path: &Path) -> Vec<PushHistoryEntry> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn append(path: &Path, entry: PushHistoryEntry) -> Result<(), String> {
    let mut entries = load(path);
    entries.insert(0, entry);
    entries.truncate(MAX_ENTRIES);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(
        &temp,
        serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写入推送历史失败：{e}"))?;
    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("写入推送历史失败：{e}")
    })
}
