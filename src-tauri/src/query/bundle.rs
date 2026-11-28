use chrono::{DateTime, Local, Utc};
use rusqlite::Connection;

use crate::budget;
use crate::cursor_account;
use crate::domain::{BudgetConfig, OverviewBundleDto, OverviewBundleRequest, PriceTable};
use crate::store;

use super::billing::billing_windows_installed_with_cursor;
use super::meta::filter_options;
use super::overview::overview_installed;
use super::series::{breakdown_installed_with_cursor, trend_installed_with_cursor};
use super::sessions::top_sessions_installed;
use super::sql::install_prices;

const DEFAULT_TOP_SESSIONS_LIMIT: usize = 8;

/// 概览页粗粒度聚合：在同一条只读连接上安装一次临时单价表、读一次 Cursor 账号事件，
/// 一次产出全部 SQLite 指标。官方额度联网刷新保持独立、不阻塞本查询。
pub fn overview_bundle(
    conn: &Connection,
    request: &OverviewBundleRequest,
    prices: &PriceTable,
    budget_config: &BudgetConfig,
    now_utc: DateTime<Utc>,
    now_local: DateTime<Local>,
) -> Result<OverviewBundleDto, String> {
    install_prices(conn, prices)?;
    let all_cursor_events = store::load_cursor_account_events(conn)?;
    let cursor_as_of = store::cursor_account_as_of(conn)?;

    let options = if request.include_options {
        Some(filter_options(conn)?)
    } else {
        None
    };

    let overview = overview_installed(conn, &request.filter)?;
    let previous = request
        .previous_filter
        .as_ref()
        .map(|prev| overview_installed(conn, prev))
        .transpose()?;

    let trend_cursor_events = cursor_account::filter_events_for_application_analytics(
        &all_cursor_events,
        &request.filter,
    );
    let trend = trend_installed_with_cursor(
        conn,
        &request.filter,
        prices,
        &request.grain,
        &trend_cursor_events,
    )?;

    let heatmap_cursor_events = cursor_account::filter_events_for_application_analytics(
        &all_cursor_events,
        &request.heatmap_filter,
    );
    let heatmap = trend_installed_with_cursor(
        conn,
        &request.heatmap_filter,
        prices,
        "day",
        &heatmap_cursor_events,
    )?;

    let models = breakdown_installed_with_cursor(conn, &request.filter, prices, "model", None)?;
    let projects = breakdown_installed_with_cursor(
        conn,
        &request.filter,
        prices,
        "project",
        Some(&trend_cursor_events),
    )?;

    let sessions_limit = request
        .top_sessions_limit
        .unwrap_or(DEFAULT_TOP_SESSIONS_LIMIT);
    let sessions = top_sessions_installed(conn, &request.filter, sessions_limit)?;

    let weekly_cursor_events =
        cursor_account::filter_events_for_weekly_window(&all_cursor_events, &request.filter);
    let billing_windows = billing_windows_installed_with_cursor(
        conn,
        &request.filter,
        prices,
        now_utc,
        &weekly_cursor_events,
    )?;

    let cursor_account_usage = cursor_account::summarize_events_filtered(
        &all_cursor_events,
        Some(&request.filter),
        cursor_as_of,
    );

    let month_filter = budget::local_month_filter(now_local);
    let month_overview = overview_installed(conn, &month_filter)?;
    let budget_status = budget::status_from_overview(&month_overview, budget_config, now_local);

    Ok(OverviewBundleDto {
        options,
        overview,
        previous,
        trend,
        heatmap,
        models,
        projects,
        sessions,
        billing_windows,
        cursor_account_usage,
        budget_status,
    })
}
