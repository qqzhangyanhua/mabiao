//! 每日自动推送（ADR 0026）：成员自己打开的独立开关，**默认关**，推前一天的数据。
//!
//! 它不挂本机摄取的 1/5/10 分钟定时器（对齐 ADR 0006 的独立开关做法），调度在命令层单独一条线程。
//! 这里只有判定与编排：哪几天该推、凭证够不够、推完怎么记进度。筛选、打码、整场跳过与推送历史
//! 全部复用 `push::run_with`，与手动推送同一套规则。`now`、时区、路径都由调用方注入。
//!
//! 进度只有一个游标 `pushed_through`：这一天（含）之前都推干净了。只有「没有失败的会话、
//! 消耗记录没出错、登录没被拒」才推进它；本机读不全而跳过的会话重试也不会好，不挡进度。
//! 应用没开漏掉的天，下次启动时游标落后，于是一次补齐。

use std::fs;
use std::path::Path;

use chrono::{DateTime, Days, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use super::{PushOutcome, PushRange, PushRunInput};
use crate::push;
use crate::remote_server::{self, store::RemoteServerPaths, SessionState};

pub const STATE_FILE: &str = "push_auto.json";
/// 应用很久没开时最多往回补这么多天，免得一次推一整年。
pub const MAX_CATCH_UP_DAYS: i64 = 31;

/// 落盘的状态。缺文件、坏文件、缺字段都按默认（关）处理。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoPushState {
    #[serde(default)]
    pub enabled: bool,
    /// 本机日历日 `YYYY-MM-DD`：这一天（含）之前都已推完。`None` 表示还没推过。
    #[serde(default)]
    pub pushed_through: Option<String>,
    /// RFC 3339，最近一次真正尝试推送的时间。
    #[serde(default)]
    pub last_attempt_at: Option<String>,
    /// 最近一次尝试没推干净时的一句中文原因；推干净后清空。
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug)]
pub enum AutoPushStatus {
    Disabled,
    /// 昨天及之前都已推完。
    UpToDate,
    /// 登录不可用，没有联网；原因同时显示在设置页。
    NotLoggedIn(String),
    /// 推了（不代表全成功，看 `PushOutcome`）。
    Pushed(PushOutcome),
    /// 推送在开始前就失败了（本机读库失败等）。
    Failed(String),
}

/// 设置页「每日自动推送」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoPushDto {
    pub enabled: bool,
    pub pushed_through: Option<String>,
    pub last_attempt_at: Option<String>,
    pub last_error: Option<String>,
    pub session_state: SessionState,
    /// 开着但登录不可用时的一句话：自动推送不会运行，要去登录。
    pub login_notice: Option<String>,
}

pub fn load(path: &Path) -> AutoPushState {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(path: &Path, state: &AutoPushState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(
        &temp,
        serde_json::to_string_pretty(state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("保存自动推送设置失败：{e}"))?;
    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("保存自动推送设置失败：{e}")
    })
}

/// 关着的那些天不补：重新打开时进度清零，从昨天算起。关闭时保留进度，只是不再运行。
pub fn set_enabled(path: &Path, enabled: bool) -> Result<AutoPushState, String> {
    let mut state = load(path);
    if enabled && !state.enabled {
        state.pushed_through = None;
        state.last_error = None;
    }
    state.enabled = enabled;
    save(path, &state)?;
    Ok(state)
}

/// 记一次尝试。每次重读再写：推送要联网很久，期间用户可能在设置页动过开关，不能拿旧状态覆盖。
/// 进度只往前走。
pub fn record_attempt(
    path: &Path,
    now: DateTime<Utc>,
    advance_to: Option<NaiveDate>,
    error: Option<String>,
) -> Result<(), String> {
    let mut state = load(path);
    state.last_attempt_at = Some(now.to_rfc3339());
    state.last_error = error;
    if let Some(day) = advance_to {
        let current = state.pushed_through.as_deref().and_then(parse_day);
        if current.is_none_or(|current| day > current) {
            state.pushed_through = Some(day.format("%Y-%m-%d").to_string());
        }
    }
    save(path, &state)
}

fn parse_day(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

/// 该补推的本机日历日区间（含两端）；没有则 `None`。「前一天」永远是 `today` 的前一天，
/// 今天还在产生数据，不推。游标读不懂时当作没推过。
pub fn pending_days(state: &AutoPushState, today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    if !state.enabled {
        return None;
    }
    let yesterday = today.checked_sub_days(Days::new(1))?;
    let first = match state.pushed_through.as_deref().and_then(parse_day) {
        Some(done) => done.checked_add_days(Days::new(1))?,
        None => yesterday,
    };
    if first > yesterday {
        return None;
    }
    let earliest = yesterday.checked_sub_days(Days::new(MAX_CATCH_UP_DAYS as u64 - 1))?;
    Some((first.max(earliest), yesterday))
}

/// 本机时区 `first` 的 0 点到 `last` 的 23:59:59，换成库里存的那种 UTC 写法（后端按字符串比较）。
/// 不按来源筛：自动推送推全部。
pub fn range_for_days<Tz: TimeZone>(tz: &Tz, first: NaiveDate, last: NaiveDate) -> PushRange {
    let start = first.and_hms_opt(0, 0, 0).unwrap_or_default();
    let end = last.and_hms_opt(23, 59, 59).unwrap_or_default();
    PushRange {
        from: Some(utc_text(tz, start)),
        to: Some(utc_text(tz, end)),
        sources: Vec::new(),
    }
}

fn utc_text<Tz: TimeZone>(tz: &Tz, local: NaiveDateTime) -> String {
    // 夏令时空档里的本地时刻不存在，按「当作 UTC」兜底，只会偏差一小时。
    tz.from_local_datetime(&local)
        .earliest()
        .map_or_else(|| local.and_utc(), |time| time.with_timezone(&Utc))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// 调度线程每次醒来调一次。`refresh_local` 在确定要推、凭证也够之后调用，让调用方先把本机摄取跑到最新。
/// 它失败就不推也不推进进度：本机库里可能还没有昨天的会话，这时推出去的是空的，
/// 推进进度会让那一天再也补不上。
pub fn run_due<Tz: TimeZone>(
    env: &push::PushEnv<'_>,
    state_path: &Path,
    tz: &Tz,
    refresh_local: &dyn Fn() -> Result<(), String>,
) -> AutoPushStatus {
    let state = load(state_path);
    if !state.enabled {
        return AutoPushStatus::Disabled;
    }
    let today = env.now.with_timezone(tz).date_naive();
    let Some((first, last)) = pending_days(&state, today) else {
        return AutoPushStatus::UpToDate;
    };
    // 登录不可用时什么都不做、不记历史；设置页从 `panel` 读到提示。
    if let Err(message) = remote_server::push_credentials(env.remote, env.now) {
        return AutoPushStatus::NotLoggedIn(message);
    }

    if let Err(message) = refresh_local() {
        let message = format!("刷新本机数据失败：{message}");
        let _ = record_attempt(state_path, env.now, None, Some(message.clone()));
        return AutoPushStatus::Failed(message);
    }
    let input = PushRunInput {
        range: range_for_days(tz, first, last),
        only: Vec::new(),
        include_usage: true,
    };
    match push::run_with(env, &input, true, &|_| {}) {
        Ok(outcome) => {
            let error = unclean_reason(&outcome);
            let advance = error.is_none().then_some(last);
            let _ = record_attempt(state_path, env.now, advance, error);
            AutoPushStatus::Pushed(outcome)
        }
        Err(message) => {
            let _ = record_attempt(state_path, env.now, None, Some(message.clone()));
            AutoPushStatus::Failed(message)
        }
    }
}

/// 有可重试的失败才算没推干净；本机读不全而跳过的会话不算。
fn unclean_reason(outcome: &PushOutcome) -> Option<String> {
    if outcome.login_required {
        return Some("远程服务不再接受这次登录，请重新登录".to_string());
    }
    let mut parts = Vec::new();
    if !outcome.failed.is_empty() {
        parts.push(format!("{} 场会话推送失败", outcome.failed.len()));
    }
    if let Some(error) = &outcome.usage_error {
        parts.push(format!("消耗记录推送失败：{error}"));
    }
    (!parts.is_empty()).then(|| format!("{}，下次会重试", parts.join("；")))
}

pub fn panel(path: &Path, remote: &RemoteServerPaths, now: DateTime<Utc>) -> AutoPushDto {
    let state = load(path);
    let login = remote_server::panel(remote, now);
    let login_notice = match (state.enabled, login.state) {
        (false, _) | (true, SessionState::LoggedIn) => None,
        (true, SessionState::NotLoggedIn) => {
            Some("还没有登录远程服务，每日自动推送不会运行，请先登录".to_string())
        }
        (true, _) => login.notice,
    };
    AutoPushDto {
        enabled: state.enabled,
        pushed_through: state.pushed_through,
        last_attempt_at: state.last_attempt_at,
        last_error: state.last_error,
        session_state: login.state,
        login_notice,
    }
}
