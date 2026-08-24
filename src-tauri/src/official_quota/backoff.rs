//! 取数退避：连续失败的 provider 先歇一会儿再试。
//!
//! 主要是保护限流端点。Anthropic 的 `/api/oauth/usage` 限流很紧，被限流后继续狂刷
//! 只会让恢复更慢——所以手动刷新也受约束，只是会明确告诉用户还要等多久，
//! 而不是默默不动。
//!
//! 状态落在应用数据目录的 JSON 里，重启后仍然生效：否则「重启一下再点」
//! 就能绕过，等于没做。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const STATE_NAME: &str = "official_quota_backoff.json";
/// 被限流时起步就等久一点，翻倍上限也更高——这是唯一「继续试会更糟」的失败。
const RATE_LIMITED_BASE_MINUTES: i64 = 10;
const RATE_LIMITED_MAX_MINUTES: i64 = 60;
/// 其它失败（网络抖动、结构变更）只要不密集重试就行。
const DEFAULT_BASE_MINUTES: i64 = 1;
const DEFAULT_MAX_MINUTES: i64 = 15;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BackoffState {
    /// provider 标识 → 冷却条目。
    #[serde(default)]
    pub entries: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub failures: u32,
    /// 早于这个时刻不再尝试（RFC3339）。
    pub retry_at: String,
}

pub fn state_path() -> PathBuf {
    crate::paths::app_data_dir().join(STATE_NAME)
}

pub fn load_state(path: &Path) -> BackoffState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_state(path: &Path, state: &BackoffState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// 还要等多久才允许再试；不在冷却中返回 None。
///
/// 键是额度缓存用的那个标识：内置 9 家是 `claude` / `codex` …，自定义提供商是
/// `custom:` 开头的随机串。映射键本来就是字符串，因此两边共用同一份状态文件，
/// 格式不变、不需要迁移。
pub fn cooldown_remaining(state: &BackoffState, id: &str, now: DateTime<Utc>) -> Option<Duration> {
    let entry = state.entries.get(id)?;
    let retry_at = DateTime::parse_from_rfc3339(&entry.retry_at)
        .ok()?
        .with_timezone(&Utc);
    let remaining = retry_at - now;
    if remaining > Duration::zero() {
        Some(remaining)
    } else {
        None
    }
}

/// 冷却中时给用户的话：说清楚还要等多久，别让人对着不动的界面猜。
pub fn cooldown_message(
    state: &BackoffState,
    id: &str,
    display_name: &str,
    now: DateTime<Utc>,
) -> Option<String> {
    let remaining = cooldown_remaining(state, id, now)?;
    let minutes = (remaining.num_seconds() as f64 / 60.0).ceil() as i64;
    Some(format!(
        "{display_name} 刚取数失败，{} 分钟后自动重试；期间显示的是上次结果",
        minutes.max(1)
    ))
}

pub fn record_success(state: &mut BackoffState, id: &str) {
    state.entries.remove(id);
}

/// 忘掉这一条的冷却。用在「用户刚改过配置」之后：轮换密钥、换域名、改预设类型
/// 都可能正是为了修好上一轮的失败，再拿旧的退避拦着，用户看到的会是
/// 「刚取数失败，N 分钟后自动重试」——把他刚做完的修复盖掉。
///
/// 写不下去不算失败：最坏是这次还得等，下次保存再清。
pub fn clear(path: &Path, id: &str) {
    let mut state = load_state(path);
    if state.entries.remove(id).is_some() {
        let _ = save_state(path, &state);
    }
}

/// 记一次失败并推后下次尝试。退避按连续失败次数翻倍，各自封顶。
pub fn record_failure(state: &mut BackoffState, id: &str, error: &str, now: DateTime<Utc>) {
    let failures = state
        .entries
        .get(id)
        .map_or(1, |entry| entry.failures.saturating_add(1));
    let (base, max) = if is_rate_limited(error) {
        (RATE_LIMITED_BASE_MINUTES, RATE_LIMITED_MAX_MINUTES)
    } else {
        (DEFAULT_BASE_MINUTES, DEFAULT_MAX_MINUTES)
    };
    let minutes = base
        .saturating_mul(1i64 << failures.saturating_sub(1).min(6))
        .min(max);
    state.entries.insert(
        id.to_string(),
        Entry {
            failures,
            retry_at: (now + Duration::minutes(minutes)).to_rfc3339(),
        },
    );
}

/// provider 的错误目前是纯字符串，只能按标记认。各家的限流提示里都带「限流」，
/// HTTP 码兜底认 429。
pub fn is_rate_limited(error: &str) -> bool {
    error.contains("限流") || error.contains("429")
}
