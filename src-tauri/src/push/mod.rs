//! 推送：把选定区间内的对话记录与消耗记录单向送到远程服务（ADR 0026）。
//!
//! 这是码表唯一的出站数据通道，只在用户显式发起时运行。读会话在 `conversation::push_source`，
//! 转协议类型与打码在 `payload` / `redact`，联网在 `remote_server::client`。
//!
//! 编排：选会话 → 逐场「读 → 打码 → 一个请求」→ 消耗记录分批 → 记历史。读连接只在读一场会话
//! 的那一刻持有，联网时已放开，不挡摄取。`now`、路径、连接都由调用方注入。

pub mod git_remote;
pub mod history;
pub mod payload;
pub mod redact;
mod usage;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Utc};
use push_protocol::{DeviceInfo, PushUsageRequest, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};

use crate::conversation;
use crate::domain::{ConversationQuery, ConversationSessionRow, PriceTable};
use crate::remote_server::client::{self, RemoteError};
use crate::remote_server::store::{self, RemoteServerPaths};
use crate::remote_server::{self, PushCredentials};
use crate::work_notes::ConnectionSource;
use history::PushHistoryEntry;

/// 预览里给成员的知情告知（ADR 0026「脱敏」「推送流程」）。
pub const ADMIN_NOTICE: &str = "管理员可以查看你推送的全部正文";
/// 与 `server/src/push.rs` 的 `PUSH_BODY_LIMIT_BYTES` 一致：超了服务端会直接拒收。
const MAX_SESSION_BODY_BYTES: u64 = 64 * 1024 * 1024;
/// 与 `server/src/push.rs` 的 `MAX_USAGE_RECORDS_PER_REQUEST` 一致。
const USAGE_BATCH: usize = 5000;

/// 区间与来源筛选。`from` / `to` 是 RFC 3339，两端都含；`sources` 为空表示全部来源。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushRange {
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub sources: Vec<String>,
}

impl PushRange {
    fn session_query(&self) -> ConversationQuery {
        ConversationQuery {
            sources: self.sources.clone(),
            from: self.from.clone(),
            to: self.to.clone(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionKey {
    pub source: String,
    pub session_id: String,
}

/// 一场没推成的会话。`skipped` 是本机读不全（不重试也不会好），`failed` 是联网失败（可重试）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushSessionIssue {
    pub source: String,
    pub session_id: String,
    pub title: String,
    pub reason: String,
}

impl PushSessionIssue {
    fn new(row: &ConversationSessionRow, reason: impl Into<String>) -> Self {
        Self {
            source: row.source.clone(),
            session_id: row.session_id.clone(),
            title: row.title.clone(),
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushPreviewDto {
    pub sessions: u32,
    pub events: u32,
    pub usage_records: u32,
    /// `occurred_at` 不是合法时间而不会推的消耗记录数。
    pub usage_skipped_invalid_time: u32,
    pub estimated_bytes: u64,
    /// 本次正文与注入原文被打码的处数。
    pub redactions: u32,
    pub skipped: Vec<PushSessionIssue>,
    pub notice: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushRunInput {
    pub range: PushRange,
    /// 只重试这些会话；为空表示区间内全部。
    #[serde(default)]
    pub only: Vec<SessionKey>,
    /// 重试时可以只补会话，不再发消耗记录。
    #[serde(default = "default_true")]
    pub include_usage: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushOutcome {
    pub sessions_succeeded: u32,
    pub failed: Vec<PushSessionIssue>,
    pub skipped: Vec<PushSessionIssue>,
    pub usage_inserted: u32,
    pub usage_duplicates: u32,
    pub usage_skipped_invalid_time: u32,
    pub usage_error: Option<String>,
    /// 途中服务端不再认这次登录：剩下的会话都记成失败，设置页会提示重新登录。
    pub login_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushProgress {
    pub done: u32,
    pub total: u32,
    pub title: String,
}

/// 推送要的一切外部依赖，全部由调用方注入。
pub struct PushEnv<'a> {
    pub conns: &'a dyn ConnectionSource,
    pub home: &'a Path,
    pub prices: &'a PriceTable,
    pub remote: &'a RemoteServerPaths,
    pub history: &'a Path,
    pub now: DateTime<Utc>,
}

enum ReadError {
    /// 这一场读不全，整场跳过。
    Skip(String),
    /// 连接都拿不到，没必要继续。
    Fatal(String),
}

/// 同一时刻只允许一次预览或推送：两次并发会重复读整个区间，进度也对不上。由命令层持有；
/// `preview` / `run` 自己不碰它，测试才能并行。
pub struct RunGuard;

static RUNNING: AtomicBool = AtomicBool::new(false);

impl RunGuard {
    pub fn acquire() -> Result<Self, String> {
        RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "正在推送中，请等这一次结束".to_string())
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

fn read_session(
    env: &PushEnv<'_>,
    row: &ConversationSessionRow,
    device: &DeviceInfo,
) -> Result<payload::BuiltSession, ReadError> {
    let source = {
        let conn = env.conns.read().map_err(ReadError::Fatal)?;
        conversation::read_push_session(&conn, env.home, row)
    }
    .map_err(ReadError::Skip)?;
    let built = payload::build_session(source, device);
    if built.bytes > MAX_SESSION_BODY_BYTES {
        return Err(ReadError::Skip(format!(
            "这一场打包后 {} MiB，超过服务端单场 {} MiB 的上限",
            built.bytes / (1024 * 1024),
            MAX_SESSION_BODY_BYTES / (1024 * 1024)
        )));
    }
    Ok(built)
}

fn list_sessions(
    env: &PushEnv<'_>,
    range: &PushRange,
) -> Result<Vec<ConversationSessionRow>, String> {
    let conn = env.conns.read()?;
    conversation::list_push_sessions(&conn, &range.session_query())
}

fn build_usage(env: &PushEnv<'_>, range: &PushRange) -> Result<payload::BuiltUsage, String> {
    let records = {
        let conn = env.conns.read()?;
        usage::load_usage_in_range(&conn, range)?
    };
    Ok(payload::build_usage(&records, env.prices))
}

/// 预览：数出会推什么，不联网。会把每场会话真的读一遍、打一遍码，好让数字与实际发送一致。
pub fn preview(env: &PushEnv<'_>, range: &PushRange) -> Result<PushPreviewDto, String> {
    let config = store::load_config(&env.remote.config);
    let device = DeviceInfo {
        device_id: config.device_id,
        device_name: config.device_name,
    };
    let mut dto = PushPreviewDto {
        sessions: 0,
        events: 0,
        usage_records: 0,
        usage_skipped_invalid_time: 0,
        estimated_bytes: 0,
        redactions: 0,
        skipped: Vec::new(),
        notice: ADMIN_NOTICE.to_string(),
    };
    for row in list_sessions(env, range)? {
        match read_session(env, &row, &device) {
            Ok(built) => {
                dto.sessions += 1;
                dto.events += built.event_count;
                dto.redactions += built.redactions;
                dto.estimated_bytes += built.bytes;
            }
            Err(ReadError::Skip(reason)) => dto.skipped.push(PushSessionIssue::new(&row, reason)),
            Err(ReadError::Fatal(error)) => return Err(error),
        }
    }
    let usage = build_usage(env, range)?;
    dto.usage_records = usage.records.len() as u32;
    dto.usage_skipped_invalid_time = usage.skipped_invalid_time;
    dto.estimated_bytes += serde_json::to_vec(&usage.records).map_or(0, |body| body.len() as u64);
    Ok(dto)
}

/// 真推。每场会话一个请求，失败的单独列出来；消耗记录按批发。结束时记一条本机历史。
pub fn run(
    env: &PushEnv<'_>,
    input: &PushRunInput,
    progress: &dyn Fn(PushProgress),
) -> Result<PushOutcome, String> {
    let credentials = remote_server::push_credentials(env.remote, env.now)?;

    let mut rows = list_sessions(env, &input.range)?;
    if !input.only.is_empty() {
        rows.retain(|row| {
            input
                .only
                .iter()
                .any(|key| key.source == row.source && key.session_id == row.session_id)
        });
    }
    let total = rows.len() as u32;
    let mut outcome = PushOutcome {
        sessions_succeeded: 0,
        failed: Vec::new(),
        skipped: Vec::new(),
        usage_inserted: 0,
        usage_duplicates: 0,
        usage_skipped_invalid_time: 0,
        usage_error: None,
        login_required: false,
    };

    for (index, row) in rows.iter().enumerate() {
        progress(PushProgress {
            done: index as u32,
            total,
            title: row.title.clone(),
        });
        if outcome.login_required {
            outcome
                .failed
                .push(PushSessionIssue::new(row, client::TOKEN_REJECTED));
            continue;
        }
        match read_session(env, row, &credentials.device) {
            Err(ReadError::Skip(reason)) => {
                outcome.skipped.push(PushSessionIssue::new(row, reason))
            }
            Err(ReadError::Fatal(error)) => return Err(error),
            Ok(built) => push_one(env, &credentials, row, &built, &mut outcome),
        }
    }
    progress(PushProgress {
        done: total,
        total,
        title: String::new(),
    });

    if input.include_usage && !outcome.login_required {
        push_usage(env, &credentials, &input.range, &mut outcome)?;
    }

    // 历史是回看用的，写不进去不该把已经成功的推送报成失败。
    let _ = history::append(env.history, history_entry(env, input, &outcome));
    Ok(outcome)
}

/// 服务端不再认这次登录：清掉本机 token、标成需要重登，之后的请求都不用发了。
fn token_rejected(env: &PushEnv<'_>, outcome: &mut PushOutcome) {
    let _ = remote_server::mark_token_rejected(env.remote);
    outcome.login_required = true;
}

fn push_one(
    env: &PushEnv<'_>,
    credentials: &PushCredentials,
    row: &ConversationSessionRow,
    built: &payload::BuiltSession,
    outcome: &mut PushOutcome,
) {
    match client::push_session(&credentials.base_url, &credentials.token, &built.request) {
        Ok(_) => outcome.sessions_succeeded += 1,
        Err(RemoteError::TokenRejected) => {
            token_rejected(env, outcome);
            outcome
                .failed
                .push(PushSessionIssue::new(row, client::TOKEN_REJECTED));
        }
        Err(other) => outcome
            .failed
            .push(PushSessionIssue::new(row, other.into_message())),
    }
}

fn push_usage(
    env: &PushEnv<'_>,
    credentials: &PushCredentials,
    range: &PushRange,
    outcome: &mut PushOutcome,
) -> Result<(), String> {
    let usage = build_usage(env, range)?;
    outcome.usage_skipped_invalid_time = usage.skipped_invalid_time;
    for batch in usage.records.chunks(USAGE_BATCH) {
        let request = PushUsageRequest {
            protocol_version: PROTOCOL_VERSION,
            device: credentials.device.clone(),
            records: batch.to_vec(),
        };
        match client::push_usage(&credentials.base_url, &credentials.token, &request) {
            Ok(response) => {
                outcome.usage_inserted += response.inserted;
                outcome.usage_duplicates += response.duplicates;
            }
            Err(RemoteError::TokenRejected) => {
                token_rejected(env, outcome);
                outcome.usage_error = Some(client::TOKEN_REJECTED.to_string());
                break;
            }
            Err(other) => {
                outcome.usage_error = Some(other.into_message());
                break;
            }
        }
    }
    Ok(())
}

fn history_entry(
    env: &PushEnv<'_>,
    input: &PushRunInput,
    outcome: &PushOutcome,
) -> PushHistoryEntry {
    PushHistoryEntry {
        at: env.now.to_rfc3339(),
        from: input.range.from.clone(),
        to: input.range.to.clone(),
        sources: input.range.sources.clone(),
        sessions_succeeded: outcome.sessions_succeeded,
        sessions_failed: outcome.failed.len() as u32,
        sessions_skipped: outcome.skipped.len() as u32,
        usage_inserted: outcome.usage_inserted,
        usage_duplicates: outcome.usage_duplicates,
        usage_failed: outcome.usage_error.is_some(),
    }
}
