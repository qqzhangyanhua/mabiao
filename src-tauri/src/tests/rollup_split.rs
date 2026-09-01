use crate::rollup_split::{rollup_plan, PartialRange, RollupPlan, RollupSplit};

fn plan(from: Option<&str>, to: Option<&str>, ready: bool, grain: Option<&str>) -> RollupPlan {
    rollup_plan(from, to, ready, grain)
}

fn split(plan: RollupPlan) -> RollupSplit {
    match plan {
        RollupPlan::Split(split) => split,
        other => panic!("期望切分，得到 {other:?}"),
    }
}

#[test]
fn not_ready_returns_raw() {
    assert_eq!(
        plan(None, None, false, None),
        RollupPlan::Raw,
        "未就绪时即使没有时间窗也走明细"
    );
    assert_eq!(
        plan(
            Some("2026-08-01T12:00:00Z"),
            Some("2026-08-10T12:00:00Z"),
            false,
            Some("day"),
        ),
        RollupPlan::Raw,
        "未就绪时有完整 UTC 天也走明细"
    );
}

#[test]
fn empty_window_returns_rollup_when_ready() {
    assert_eq!(plan(None, None, true, None), RollupPlan::Rollup);
    assert_eq!(
        plan(None, None, true, Some("day")),
        RollupPlan::Rollup,
        "天粒度全量仍走纯预聚合"
    );
    assert_eq!(plan(None, None, true, Some("week")), RollupPlan::Rollup);
    assert_eq!(plan(None, None, true, Some("month")), RollupPlan::Rollup);
}

#[test]
fn complete_utc_days_round_from_up_and_to_down() {
    let got = split(plan(
        Some("2026-08-01T12:00:00Z"),
        Some("2026-08-04T15:00:00Z"),
        true,
        None,
    ));
    // 起点向上取整到 08-02 午夜，终点向下取整到 08-04 午夜 → 完整天 [08-02, 08-04)
    assert_eq!(
        got,
        RollupSplit {
            complete_from: Some("2026-08-02".into()),
            complete_to: Some("2026-08-04".into()),
            head: Some(PartialRange {
                from: "2026-08-01T12:00:00Z".into(),
                to: "2026-08-02".into(),
            }),
            tail: Some(PartialRange {
                from: "2026-08-04".into(),
                to: "2026-08-04T15:00:00Z".into(),
            }),
        }
    );

    // 只要有一个完整 UTC 天就切分，不做数量阈值。
    let one_day = split(plan(
        Some("2026-08-01T12:00:00Z"),
        Some("2026-08-03T12:00:00Z"),
        true,
        None,
    ));
    assert_eq!(one_day.complete_from.as_deref(), Some("2026-08-02"));
    assert_eq!(one_day.complete_to.as_deref(), Some("2026-08-03"));
    assert!(one_day.head.is_some());
    assert!(one_day.tail.is_some());
}

#[test]
fn no_complete_utc_day_returns_raw() {
    assert_eq!(
        plan(
            Some("2026-08-01T12:00:00Z"),
            Some("2026-08-01T18:00:00Z"),
            true,
            None,
        ),
        RollupPlan::Raw,
        "同一 UTC 天内没有完整天"
    );
    assert_eq!(
        plan(
            Some("2026-08-01T12:00:00Z"),
            Some("2026-08-02T12:00:00Z"),
            true,
            None,
        ),
        RollupPlan::Raw,
        "相邻两天、中间凑不出一个完整 UTC 天"
    );
}

#[test]
fn aligned_endpoints_leave_the_matching_partial_empty() {
    let from_aligned = split(plan(
        Some("2026-08-01T00:00:00Z"),
        Some("2026-08-04T15:00:00Z"),
        true,
        None,
    ));
    assert_eq!(from_aligned.head, None, "from 对齐 UTC 午夜时头部为空");
    assert_eq!(from_aligned.complete_from.as_deref(), Some("2026-08-01"));
    assert_eq!(
        from_aligned.tail,
        Some(PartialRange {
            from: "2026-08-04".into(),
            to: "2026-08-04T15:00:00Z".into(),
        })
    );

    let to_aligned = split(plan(
        Some("2026-08-01T12:00:00Z"),
        Some("2026-08-04T00:00:00Z"),
        true,
        None,
    ));
    assert_eq!(to_aligned.tail, None, "to 对齐 UTC 午夜时尾部为空");
    assert_eq!(to_aligned.complete_to.as_deref(), Some("2026-08-04"));
    assert_eq!(
        to_aligned.head,
        Some(PartialRange {
            from: "2026-08-01T12:00:00Z".into(),
            to: "2026-08-02".into(),
        })
    );

    let both = split(plan(
        Some("2026-08-01T00:00:00.000Z"),
        Some("2026-08-05T00:00:00+00:00"),
        true,
        None,
    ));
    assert_eq!(both.head, None);
    assert_eq!(both.tail, None);
    assert_eq!(both.complete_from.as_deref(), Some("2026-08-01"));
    assert_eq!(both.complete_to.as_deref(), Some("2026-08-05"));
}

#[test]
fn hour_grain_returns_raw() {
    assert_eq!(
        plan(None, None, true, Some("hour")),
        RollupPlan::Raw,
        "小时粒度即使没有时间窗也强制明细"
    );
    assert_eq!(
        plan(
            Some("2026-08-01T12:00:00Z"),
            Some("2026-08-10T12:00:00Z"),
            true,
            Some("hour"),
        ),
        RollupPlan::Raw,
        "小时粒度有完整 UTC 天也强制明细"
    );
}

#[test]
fn open_and_reversed_windows_follow_the_same_rules() {
    let open_from = split(plan(None, Some("2026-08-07T15:00:00Z"), true, None));
    assert_eq!(
        open_from,
        RollupSplit {
            complete_from: None,
            complete_to: Some("2026-08-07".into()),
            head: None,
            tail: Some(PartialRange {
                from: "2026-08-07".into(),
                to: "2026-08-07T15:00:00Z".into(),
            }),
        }
    );

    let open_to = split(plan(Some("2026-08-01T12:00:00Z"), None, true, None));
    assert_eq!(
        open_to,
        RollupSplit {
            complete_from: Some("2026-08-02".into()),
            complete_to: None,
            head: Some(PartialRange {
                from: "2026-08-01T12:00:00Z".into(),
                to: "2026-08-02".into(),
            }),
            tail: None,
        }
    );

    assert_eq!(
        plan(
            Some("2026-08-10T12:00:00Z"),
            Some("2026-08-08T12:00:00Z"),
            true,
            None,
        ),
        RollupPlan::Raw,
        "反向窗口自然落成无完整 UTC 天"
    );
}
