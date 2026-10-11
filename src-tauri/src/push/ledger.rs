//! 本机推送台账：每场会话上次推成功时的内容指纹，用来跳过没变化的会话（ADR 0026「推送流程」）。
//!
//! 它只是省流量的优化，不是事实来源：服务端按 `(账号, 设备, 来源, 会话ID)` 整场覆盖，重复推送
//! 本来就不会产生重复数据。所以台账缺了、坏了、读不懂都按「没推过」处理，最坏情况是多推一遍。
//! 台账按「服务地址 + 账号 + 设备」分开记：换服务器或换账号登录，旧记录不作数。
//! 只记指纹，不记正文；文件在应用数据目录，不在备份白名单里。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const LEDGER_FILE: &str = "push_ledger.json";
pub const USAGE_LEDGER_FILE: &str = "push_usage_ledger.json";
/// 消耗记录指纹是 64 位十六进制的 SHA-256，台账只留前 24 位（96 bit）：碰撞概率可以忽略，
/// 十几万条记录的台账也只有几 MB。
const USAGE_KEY_CHARS: usize = 24;

/// 台账与推送历史放在同一目录，路径由历史文件推出，免得每个调用方多传一个路径。
pub fn path_beside(history: &Path) -> PathBuf {
    history.with_file_name(LEDGER_FILE)
}

/// 消耗记录台账与会话台账分开存：它条数多得多，只在推消耗记录时才读。
pub fn usage_path_beside(history: &Path) -> PathBuf {
    history.with_file_name(USAGE_LEDGER_FILE)
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LedgerFile {
    /// `范围\n来源\n会话ID` → 内容指纹。
    #[serde(default)]
    sessions: BTreeMap<String, String>,
}

pub struct Ledger {
    path: PathBuf,
    file: LedgerFile,
    scope: String,
    dirty: bool,
}

impl Ledger {
    /// `scope` 区分服务地址、账号与设备：任何一个变了，之前的记录都不再适用。
    pub fn load(path: &Path, base_url: &str, account: &str, device_id: &str) -> Self {
        let file = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            file,
            scope: format!("{base_url}\n{account}\n{device_id}"),
            dirty: false,
        }
    }

    fn key(&self, source: &str, session_id: &str) -> String {
        format!("{}\n{source}\n{session_id}", self.scope)
    }

    /// 这场会话上次推成功时的内容与现在完全一样。
    pub fn is_current(&self, source: &str, session_id: &str, fingerprint: &str) -> bool {
        self.file
            .sessions
            .get(&self.key(source, session_id))
            .is_some_and(|known| known == fingerprint)
    }

    pub fn record(&mut self, source: &str, session_id: &str, fingerprint: String) {
        let key = self.key(source, session_id);
        if self.file.sessions.get(&key) != Some(&fingerprint) {
            self.file.sessions.insert(key, fingerprint);
            self.dirty = true;
        }
    }

    /// 没有变化就不写盘。写失败不该把已经成功的推送报成失败，调用方可以忽略结果。
    pub fn save(&self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temp = self.path.with_extension("json.tmp");
        fs::write(
            &temp,
            serde_json::to_string(&self.file).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("写入推送台账失败：{e}"))?;
        fs::rename(&temp, &self.path).map_err(|e| {
            let _ = fs::remove_file(&temp);
            format!("写入推送台账失败：{e}")
        })
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct UsageLedgerFile {
    /// 范围 → 已推成功的消耗记录指纹（前 24 位，有序）。其它范围的记录原样保留。
    #[serde(default)]
    scopes: BTreeMap<String, BTreeSet<String>>,
}

/// 本机推过哪些消耗记录。服务端本来就按指纹去重（`DO NOTHING`），这里只是不再把已经推过的
/// 重复发一遍；缺了、坏了按「没推过」处理。范围的划分与会话台账一致。
pub struct UsageLedger {
    path: PathBuf,
    file: UsageLedgerFile,
    scope: String,
    dirty: bool,
}

impl UsageLedger {
    pub fn load(path: &Path, base_url: &str, account: &str, device_id: &str) -> Self {
        let file = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            file,
            scope: format!("{base_url}\n{account}\n{device_id}"),
            dirty: false,
        }
    }

    fn short(fingerprint: &str) -> &str {
        match fingerprint.char_indices().nth(USAGE_KEY_CHARS) {
            Some((end, _)) => &fingerprint[..end],
            None => fingerprint,
        }
    }

    pub fn contains(&self, fingerprint: &str) -> bool {
        self.file
            .scopes
            .get(&self.scope)
            .is_some_and(|known| known.contains(Self::short(fingerprint)))
    }

    pub fn record<'a>(&mut self, fingerprints: impl IntoIterator<Item = &'a str>) {
        let known = self.file.scopes.entry(self.scope.clone()).or_default();
        for fingerprint in fingerprints {
            if known.insert(Self::short(fingerprint).to_string()) {
                self.dirty = true;
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temp = self.path.with_extension("json.tmp");
        fs::write(
            &temp,
            serde_json::to_string(&self.file).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("写入推送台账失败：{e}"))?;
        fs::rename(&temp, &self.path).map_err(|e| {
            let _ = fs::remove_file(&temp);
            format!("写入推送台账失败：{e}")
        })
    }
}
