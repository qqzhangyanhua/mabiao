use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{OfficialQuotaHistoryDto, OfficialQuotaHistoryPoint, OfficialQuotaWindow};

pub const OFFICIAL_QUOTA_HISTORY_RETENTION_DAYS: i64 = 45;

pub fn upsert_official_quota(
    conn: &Connection,
    provider: &str,
    windows: &[OfficialQuotaWindow],
    captured_at: &str,
    error: Option<&str>,
    plan: Option<&str>,
) -> Result<(), String> {
    let existing = load_official_quota_row(conn, provider)?;
    let (prev_windows_json, prev_captured_at) = next_prev_snapshot(existing.as_ref(), captured_at)?;
    let windows_json =
        serde_json::to_string(&snapshot_windows(windows)).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO official_quota(
            provider, windows_json, captured_at, error, plan, prev_windows_json, prev_captured_at
         )
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(provider) DO UPDATE SET
            windows_json = excluded.windows_json,
            captured_at = excluded.captured_at,
            error = excluded.error,
            plan = COALESCE(excluded.plan, official_quota.plan),
            prev_windows_json = excluded.prev_windows_json,
            prev_captured_at = excluded.prev_captured_at",
        params![
            provider,
            windows_json,
            captured_at,
            error,
            plan,
            prev_windows_json,
            prev_captured_at
        ],
    )
    .map_err(|e| e.to_string())?;
    if let Some(row) = existing.as_ref() {
        if !row.captured_at.is_empty() && row.captured_at != captured_at {
            append_official_quota_history(conn, provider, &row.windows, &row.captured_at)?;
        }
        if let Some(prev_at) = row.prev_captured_at.as_deref() {
            append_official_quota_history(conn, provider, &row.prev_windows, prev_at)?;
        }
    }
    append_official_quota_history(conn, provider, windows, captured_at)?;
    prune_official_quota_history(conn)?;
    Ok(())
}

/// 捕获时刻变了就把当前快照挪成上一拍；同一拍再写一次则原样留着上一拍。
fn next_prev_snapshot(
    existing: Option<&StoredOfficialQuota>,
    captured_at: &str,
) -> Result<(String, Option<String>), String> {
    let Some(row) = existing else {
        return Ok(("[]".into(), None));
    };
    if !row.captured_at.is_empty() && row.captured_at != captured_at && !row.windows.is_empty() {
        let json =
            serde_json::to_string(&snapshot_windows(&row.windows)).map_err(|e| e.to_string())?;
        return Ok((json, Some(row.captured_at.clone())));
    }
    let json =
        serde_json::to_string(&snapshot_windows(&row.prev_windows)).map_err(|e| e.to_string())?;
    Ok((json, row.prev_captured_at.clone()))
}

fn snapshot_windows(windows: &[OfficialQuotaWindow]) -> Vec<OfficialQuotaWindow> {
    windows
        .iter()
        .cloned()
        .map(|mut window| {
            window.exhaust = None;
            window
        })
        .collect()
}

pub fn set_official_quota_error(
    conn: &Connection,
    provider: &str,
    error: &str,
) -> Result<(), String> {
    let updated = conn
        .execute(
            "UPDATE official_quota SET error = ?2 WHERE provider = ?1",
            params![provider, error],
        )
        .map_err(|e| e.to_string())?;
    if updated == 0 {
        conn.execute(
            "INSERT INTO official_quota(provider, windows_json, captured_at, error)
             VALUES(?1, '[]', '', ?2)",
            params![provider, error],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub struct StoredOfficialQuota {
    pub windows: Vec<OfficialQuotaWindow>,
    pub captured_at: String,
    pub error: Option<String>,
    pub plan: Option<String>,
    pub prev_windows: Vec<OfficialQuotaWindow>,
    pub prev_captured_at: Option<String>,
}

#[derive(Clone, Copy)]
enum QuotaRowCols {
    Full,
    WithPlan,
    Minimal,
}

pub fn load_official_quota_row(
    conn: &Connection,
    provider: &str,
) -> Result<Option<StoredOfficialQuota>, String> {
    match query_official_quota_row(conn, provider, QuotaRowCols::Full) {
        Ok(row) => Ok(row),
        Err(error) if error.contains("no such column") => {
            match query_official_quota_row(conn, provider, QuotaRowCols::WithPlan) {
                Ok(row) => Ok(row),
                Err(error) if error.contains("no such column") => {
                    query_official_quota_row(conn, provider, QuotaRowCols::Minimal)
                }
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

fn query_official_quota_row(
    conn: &Connection,
    provider: &str,
    cols: QuotaRowCols,
) -> Result<Option<StoredOfficialQuota>, String> {
    let sql = match cols {
        QuotaRowCols::Full => {
            "SELECT windows_json, captured_at, error, plan, prev_windows_json, prev_captured_at
             FROM official_quota WHERE provider = ?1"
        }
        QuotaRowCols::WithPlan => {
            "SELECT windows_json, captured_at, error, plan FROM official_quota WHERE provider = ?1"
        }
        QuotaRowCols::Minimal => {
            "SELECT windows_json, captured_at, error FROM official_quota WHERE provider = ?1"
        }
    };
    let row = conn
        .query_row(sql, params![provider], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                match cols {
                    QuotaRowCols::Minimal => None,
                    _ => row.get::<_, Option<String>>(3)?,
                },
                match cols {
                    QuotaRowCols::Full => row.get::<_, Option<String>>(4)?,
                    _ => None,
                },
                match cols {
                    QuotaRowCols::Full => row.get::<_, Option<String>>(5)?,
                    _ => None,
                },
            ))
        })
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((windows_json, captured_at, error, plan, prev_windows_json, prev_captured_at)) = row
    else {
        return Ok(None);
    };
    Ok(Some(StoredOfficialQuota {
        windows: parse_windows_json(&windows_json)?,
        captured_at,
        error,
        plan,
        prev_windows: parse_optional_windows_json(prev_windows_json.as_deref())?,
        prev_captured_at: prev_captured_at.filter(|value| !value.is_empty()),
    }))
}

fn parse_windows_json(json: &str) -> Result<Vec<OfficialQuotaWindow>, String> {
    if json.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(json).map_err(|e| format!("官方额度缓存损坏：{e}"))
}

fn parse_optional_windows_json(json: Option<&str>) -> Result<Vec<OfficialQuotaWindow>, String> {
    match json {
        None | Some("") => Ok(Vec::new()),
        Some(text) => parse_windows_json(text),
    }
}

pub fn append_official_quota_history(
    conn: &Connection,
    provider: &str,
    windows: &[OfficialQuotaWindow],
    captured_at: &str,
) -> Result<(), String> {
    if captured_at.is_empty() || windows.is_empty() {
        return Ok(());
    }
    for window in snapshot_windows(windows) {
        conn.execute(
            "INSERT OR IGNORE INTO official_quota_history(
                provider, window_kind, captured_at, window_label,
                used_percent, used_amount, limit_amount, currency
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                provider,
                window.kind,
                captured_at,
                window.label,
                window.used_percent,
                window.used_amount,
                window.limit_amount,
                window.currency,
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn seed_official_quota_history(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT provider, windows_json, captured_at, prev_windows_json, prev_captured_at
             FROM official_quota",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for (provider, windows_json, captured_at, prev_windows_json, prev_captured_at) in rows {
        append_official_quota_history(
            conn,
            &provider,
            &parse_windows_json(&windows_json)?,
            &captured_at,
        )?;
        if let Some(prev_at) = prev_captured_at.filter(|value| !value.is_empty()) {
            append_official_quota_history(
                conn,
                &provider,
                &parse_optional_windows_json(prev_windows_json.as_deref())?,
                &prev_at,
            )?;
        }
    }
    prune_official_quota_history(conn)
}

pub fn prune_official_quota_history(conn: &Connection) -> Result<(), String> {
    let cutoff = (Utc::now() - Duration::days(OFFICIAL_QUOTA_HISTORY_RETENTION_DAYS)).to_rfc3339();
    conn.execute(
        "DELETE FROM official_quota_history WHERE captured_at < ?1",
        params![cutoff],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_official_quota_history(
    conn: &Connection,
    provider: Option<&str>,
) -> Result<OfficialQuotaHistoryDto, String> {
    let sql = if provider.is_some() {
        "SELECT provider, window_kind, window_label, captured_at,
                used_percent, used_amount, limit_amount, currency
         FROM official_quota_history
         WHERE provider = ?1
         ORDER BY captured_at ASC, window_kind ASC"
    } else {
        "SELECT provider, window_kind, window_label, captured_at,
                used_percent, used_amount, limit_amount, currency
         FROM official_quota_history
         ORDER BY provider ASC, captured_at ASC, window_kind ASC"
    };
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let map_row = |row: &rusqlite::Row| -> rusqlite::Result<OfficialQuotaHistoryPoint> {
        Ok(OfficialQuotaHistoryPoint {
            provider: row.get(0)?,
            window_kind: row.get(1)?,
            window_label: row.get(2)?,
            captured_at: row.get(3)?,
            used_percent: row.get(4)?,
            used_amount: row.get(5)?,
            limit_amount: row.get(6)?,
            currency: row.get(7)?,
        })
    };
    let points = if let Some(id) = provider {
        stmt.query_map(params![id], map_row)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    } else {
        stmt.query_map([], map_row)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    Ok(OfficialQuotaHistoryDto {
        points,
        retention_days: OFFICIAL_QUOTA_HISTORY_RETENTION_DAYS,
    })
}
