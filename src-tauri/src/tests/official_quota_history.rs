use crate::official_quota;
use crate::store;

fn window(kind: &str, used: f64) -> crate::domain::OfficialQuotaWindow {
    crate::domain::OfficialQuotaWindow {
        kind: kind.into(),
        label: kind.into(),
        used_percent: Some(used),
        resets_at: Some("2026-10-20T00:00:00+00:00".into()),
        ..Default::default()
    }
}

#[test]
fn history_appends_on_captured_at_change_and_dedupes_same_capture() {
    let conn = store::open_memory().unwrap();
    official_quota::apply_success(
        &conn,
        crate::domain::OfficialQuotaProvider::Claude,
        vec![window("weekly", 20.0)],
        "2026-10-01T12:00:00+00:00",
    )
    .unwrap();
    official_quota::apply_success(
        &conn,
        crate::domain::OfficialQuotaProvider::Claude,
        vec![window("weekly", 35.0)],
        "2026-10-01T12:10:00+00:00",
    )
    .unwrap();
    official_quota::apply_success(
        &conn,
        crate::domain::OfficialQuotaProvider::Claude,
        vec![window("weekly", 36.0)],
        "2026-10-01T12:10:00+00:00",
    )
    .unwrap();
    let history = store::load_official_quota_history(&conn, Some("claude")).unwrap();
    assert_eq!(history.retention_days, 45);
    assert_eq!(history.points.len(), 2);
    assert_eq!(history.points[0].used_percent, Some(20.0));
    assert_eq!(history.points[1].used_percent, Some(35.0));
}

#[test]
fn seed_copies_current_and_prev_snapshots() {
    let conn = store::open_memory().unwrap();
    official_quota::apply_success(
        &conn,
        crate::domain::OfficialQuotaProvider::Claude,
        vec![window("weekly", 10.0)],
        "2026-10-01T12:00:00+00:00",
    )
    .unwrap();
    official_quota::apply_success(
        &conn,
        crate::domain::OfficialQuotaProvider::Claude,
        vec![window("weekly", 20.0)],
        "2026-10-01T13:00:00+00:00",
    )
    .unwrap();
    conn.execute("DELETE FROM official_quota_history", [])
        .unwrap();
    assert!(store::load_official_quota_history(&conn, None)
        .unwrap()
        .points
        .is_empty());
    store::seed_official_quota_history(&conn).unwrap();
    let history = store::load_official_quota_history(&conn, Some("claude")).unwrap();
    assert_eq!(history.points.len(), 2);
    let percents: Vec<Option<f64>> = history
        .points
        .iter()
        .map(|point| point.used_percent)
        .collect();
    assert!(percents.contains(&Some(10.0)));
    assert!(percents.contains(&Some(20.0)));
}

#[test]
fn prune_drops_points_older_than_retention() {
    let conn = store::open_memory().unwrap();
    official_quota::apply_success(
        &conn,
        crate::domain::OfficialQuotaProvider::Claude,
        vec![window("weekly", 15.0)],
        "2020-01-01T00:00:00+00:00",
    )
    .unwrap();
    let history = store::load_official_quota_history(&conn, Some("claude")).unwrap();
    assert!(history.points.is_empty());
}
