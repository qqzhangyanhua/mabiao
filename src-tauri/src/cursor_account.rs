use std::time::Duration;

use rusqlite::Connection;

use crate::adapters::cursor_account::{
    parse_cursor_usage_events, parse_cursor_usage_page, summarize_cursor_usage,
};
use crate::cursor_credentials::{self, LocalCredential};
use crate::domain::{CursorAccountUsageDto, CursorUsageEvent, Filter};
use crate::store;

const USAGE_EVENTS_URL: &str = "https://cursor.com/api/dashboard/get-filtered-usage-events";
const PAGE_SIZE: u32 = 100;
const TIMEOUT: Duration = Duration::from_secs(20);

pub fn has_token() -> Result<bool, String> {
    Ok(credential_status()?.source == CREDENTIAL_SOURCE_LOCAL)
}

pub const CREDENTIAL_SOURCE_LOCAL: &str = "local";
pub const CREDENTIAL_SOURCE_NONE: &str = "none";

/// 设置页用：本机 Cursor 登录态是否可用。凭证只有这一个来源，没有手动粘贴通路。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CursorCredentialStatus {
    /// `local` / `none`
    pub source: String,
    pub email: Option<String>,
    pub expires_at: Option<String>,
    /// 读到了本机登录态但已过期——提示语要说「去 Cursor 重新登录」而不是「没装 Cursor」。
    pub local_expired: bool,
}

pub fn credential_status() -> Result<CursorCredentialStatus, String> {
    let local = cursor_credentials::read_local_credential();
    let local_expired = local.as_ref().is_some_and(LocalCredential::is_expired);
    match local.filter(|value| !value.is_expired()) {
        Some(credential) => Ok(CursorCredentialStatus {
            source: CREDENTIAL_SOURCE_LOCAL.to_string(),
            expires_at: credential.expires_at_rfc3339(),
            email: credential.email,
            local_expired: false,
        }),
        None => Ok(CursorCredentialStatus {
            source: CREDENTIAL_SOURCE_NONE.to_string(),
            email: None,
            expires_at: None,
            local_expired,
        }),
    }
}

pub fn incremental_start_ms(conn: &Connection) -> Result<i64, String> {
    Ok(store::max_cursor_account_occurred_ms(conn)?.unwrap_or(0))
}

pub fn auth_expired_error() -> String {
    "Cursor 会话已过期，请在本机 Cursor 客户端重新登录".to_string()
}

pub fn missing_token_error() -> String {
    "未找到 Cursor 登录态：请确认本机装了 Cursor 客户端并已登录".to_string()
}

pub fn network_failure_error() -> String {
    "无法连接 Cursor 用量接口，请检查网络后重试".to_string()
}

pub fn ingest_raw_pages(conn: &Connection, pages: &[&str]) -> Result<u64, String> {
    let mut events = Vec::new();
    for raw in pages {
        events.extend(parse_cursor_usage_events(raw)?);
    }
    store::upsert_cursor_account_events(conn, &events)
}

pub fn apply_fetched_pages(
    conn: &Connection,
    fetched: Result<Vec<String>, String>,
) -> Result<CursorAccountUsageDto, String> {
    let pages = fetched?;
    let refs: Vec<&str> = pages.iter().map(String::as_str).collect();
    ingest_raw_pages(conn, &refs)?;
    store::set_cursor_account_as_of(conn, &chrono::Utc::now().to_rfc3339())?;
    load_summary(conn)
}

pub fn fetch_usage_events_page(
    token: &str,
    page: u32,
    start_date_ms: i64,
) -> Result<String, String> {
    let body = serde_json::json!({
        "page": page,
        "pageSize": PAGE_SIZE,
        "startDate": start_date_ms
    });
    let request = crate::net::agent_with_timeout(TIMEOUT)
        .post(USAGE_EVENTS_URL)
        .set("Cookie", &format!("WorkosCursorSessionToken={token}"))
        .set("Origin", "https://cursor.com")
        .set("Content-Type", "application/json");
    match request.send_string(&body.to_string()) {
        Ok(response) => response
            .into_string()
            .map_err(|e| format!("读取 Cursor 账号用量响应失败：{e}")),
        Err(ureq::Error::Status(401 | 403, _)) => Err(auth_expired_error()),
        Err(ureq::Error::Status(code, response)) => {
            let _ = response.into_string();
            Err(format!("拉取 Cursor 账号用量失败：HTTP {code}"))
        }
        Err(_) => Err(network_failure_error()),
    }
}

pub fn fetch_refresh_pages(token: &str, start_date_ms: i64) -> Result<Vec<String>, String> {
    let mut page = 1u32;
    let mut pages = Vec::new();
    loop {
        let raw = fetch_usage_events_page(token, page, start_date_ms)?;
        let parsed = parse_cursor_usage_page(&raw)?;
        let page_len = parsed.events.len();
        let total = parsed.total_count;
        pages.push(raw);
        let last_page = page_len == 0 || page_len < PAGE_SIZE as usize;
        let reached_total = total > 0 && u64::from(page) * u64::from(PAGE_SIZE) >= total;
        if last_page || reached_total {
            break;
        }
        page += 1;
    }
    Ok(pages)
}

/// 凭证只有一个来源：本机 Cursor 客户端的登录态。没有手动粘贴通路，
/// 也不落钥匙串——Cursor 自己会续期并写回 `state.vscdb`。
pub fn current_token() -> Result<String, String> {
    cursor_credentials::read_local_credential()
        .filter(|credential| !credential.is_expired())
        .map(|credential| credential.session_token)
        .ok_or_else(missing_token_error)
}

pub fn events_page(
    conn: &Connection,
    query: &crate::domain::CursorAccountEventQuery,
) -> Result<crate::domain::CursorAccountEventPage, String> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 20_000);
    let sort_dir = query.sort_dir.as_deref().unwrap_or("desc");
    let (total, events) = store::cursor_account_events_page(conn, page, page_size, sort_dir)?;
    let rows = events
        .into_iter()
        .map(|event| {
            let total_tokens = event.total_tokens();
            crate::domain::CursorAccountEventRow {
                occurred_at: event.occurred_at,
                model: event.model,
                input_tokens: event.input_tokens,
                output_tokens: event.output_tokens,
                cache_read_tokens: event.cache_read_tokens,
                cache_creation_tokens: event.cache_creation_tokens,
                total_tokens,
                is_headless: event.is_headless,
            }
        })
        .collect();
    Ok(crate::domain::CursorAccountEventPage { rows, total })
}

pub fn load_summary(conn: &Connection) -> Result<CursorAccountUsageDto, String> {
    load_summary_filtered(conn, None)
}

pub fn load_summary_filtered(
    conn: &Connection,
    filter: Option<&Filter>,
) -> Result<CursorAccountUsageDto, String> {
    let events = store::load_cursor_account_events(conn)?;
    let filtered = match filter {
        Some(filter) => events
            .into_iter()
            .filter(|event| event_matches_filter(event, filter))
            .collect(),
        None => events,
    };
    let mut dto = summarize_cursor_usage(&filtered);
    dto.as_of = store::cursor_account_as_of(conn)?;
    Ok(dto)
}

/// 供来源统计挂一行：认时间与模型，不套用来源/项目/provider。
pub fn events_for_application_analytics(
    conn: &Connection,
    filter: &Filter,
) -> Result<Vec<CursorUsageEvent>, String> {
    let scoped = Filter {
        from: filter.from.clone(),
        to: filter.to.clone(),
        sources: Vec::new(),
        models: filter.models.clone(),
        projects: Vec::new(),
        providers: Vec::new(),
    };
    let events = store::load_cursor_account_events(conn)?;
    Ok(events
        .into_iter()
        .filter(|event| event_matches_filter(event, &scoped))
        .collect())
}

/// 供概览 7 天滚动挂一行：只认模型筛选，不套用来源/项目/provider，也不跟总览日期预设。
pub fn events_for_weekly_window(
    conn: &Connection,
    filter: &Filter,
) -> Result<Vec<CursorUsageEvent>, String> {
    if !filter.sources.is_empty()
        && !filter
            .sources
            .iter()
            .any(|source| source == crate::billing_window::CURSOR_WEEKLY_SOURCE)
    {
        return Ok(Vec::new());
    }
    let scoped = Filter {
        from: None,
        to: None,
        sources: Vec::new(),
        models: filter.models.clone(),
        projects: Vec::new(),
        providers: Vec::new(),
    };
    let events = store::load_cursor_account_events(conn)?;
    Ok(events
        .into_iter()
        .filter(|event| event_matches_filter(event, &scoped))
        .collect())
}

/// 账号用量只认时间与模型；来源 / 项目 / provider 是本机消耗记录维度，不套到这里。
pub fn event_matches_filter(event: &CursorUsageEvent, filter: &Filter) -> bool {
    if let Some(from) = filter.from.as_deref() {
        if !timestamp_ge(&event.occurred_at, from) {
            return false;
        }
    }
    if let Some(to) = filter.to.as_deref() {
        if !timestamp_le(&event.occurred_at, to) {
            return false;
        }
    }
    if !filter.models.is_empty() && !filter.models.iter().any(|model| model == &event.model) {
        return false;
    }
    true
}

fn timestamp_ge(occurred_at: &str, bound: &str) -> bool {
    match (parse_millis(occurred_at), parse_millis(bound)) {
        (Some(value), Some(limit)) => value >= limit,
        _ => occurred_at >= bound,
    }
}

fn timestamp_le(occurred_at: &str, bound: &str) -> bool {
    match (parse_millis(occurred_at), parse_millis(bound)) {
        (Some(value), Some(limit)) => value <= limit,
        _ => occurred_at <= bound,
    }
}

fn parse_millis(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

pub fn clear_cache(conn: &Connection) -> Result<CursorAccountUsageDto, String> {
    store::clear_cursor_account_usage(conn)?;
    load_summary(conn)
}
