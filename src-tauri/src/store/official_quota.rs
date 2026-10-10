use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::OfficialQuotaWindow;

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
