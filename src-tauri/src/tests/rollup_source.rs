use crate::rollup_source::rollup_source;
use crate::rollup_split::{rollup_plan, RollupPlan};
use crate::test_support::*;
use rusqlite::params_from_iter;
use rusqlite::types::Value;

fn source(plan: &RollupPlan, filter: &Filter) -> crate::rollup_source::RollupSource {
    rollup_source(plan, filter)
}

fn plan(from: Option<&str>, to: Option<&str>, ready: bool, grain: Option<&str>) -> RollupPlan {
    rollup_plan(from, to, ready, grain)
}

fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

fn texts(values: &[&str]) -> Vec<Value> {
    values.iter().copied().map(text).collect()
}

fn dims() -> Filter {
    Filter {
        sources: vec!["codex".into()],
        models: vec!["gpt-5".into(), "claude-sonnet-5".into()],
        projects: vec!["/p".into()],
        providers: vec!["official".into()],
        ..Filter::default()
    }
}

fn dim_params() -> Vec<Value> {
    texts(&["codex", "gpt-5", "claude-sonnet-5", "/p", "official"])
}

fn pragma_rollup_columns(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn.prepare("PRAGMA table_info(usage_rollup)").unwrap();
    stmt.query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn assert_shape(sql: &str, params: &[Value]) {
    let conn = store::open_memory().unwrap();
    let expected = pragma_rollup_columns(&conn);
    assert_eq!(
        sql.matches('?').count(),
        params.len(),
        "占位数与参数个数不一致\n{sql}\n{params:?}"
    );
    let wrapped = format!("SELECT * FROM ({sql}) d LIMIT 0");
    let mut stmt = conn
        .prepare(&wrapped)
        .unwrap_or_else(|e| panic!("SQL 无法 prepare：{e}\n{wrapped}"));
    let names: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let _ = stmt
        .query(params_from_iter(params.iter()))
        .unwrap_or_else(|e| panic!("参数无法绑定：{e}\n{wrapped}\n{params:?}"));
    assert_eq!(names, expected, "列集合必须与 usage_rollup 一致");
}

#[test]
fn raw_projects_one_row_per_record_matching_rollup_columns() {
    let filter = Filter {
        from: Some("2026-08-01T12:00:00Z".into()),
        to: Some("2026-08-01T18:00:00Z".into()),
        ..dims()
    };
    let got = source(
        &plan(filter.from.as_deref(), filter.to.as_deref(), true, None),
        &filter,
    );
    assert!(got.sql.contains("usage_records"), "纯明细应读消耗记录表");
    assert!(!got.sql.contains("usage_rollup"), "纯明细不应引用预聚合表");
    assert!(!got.sql.contains("UNION ALL"));
    assert!(
        !got.sql.to_ascii_uppercase().contains("GROUP BY"),
        "明细侧只做重命名投影，不做预聚合 GROUP BY"
    );
    assert!(
        got.sql.contains("substr(r.occurred_at, 1, 10)"),
        "day 由时间戳前缀推导\n{}",
        got.sql
    );
    assert!(
        got.sql
            .contains("CASE WHEN r.native_cost IS NOT NULL THEN 1 ELSE 0 END"),
        "has_native 按是否自带费用映射\n{}",
        got.sql
    );
    assert!(
        got.sql.contains("COALESCE(r.native_cost, 0)"),
        "原生费用原样映射，空则 0\n{}",
        got.sql
    );
    assert!(
        got.sql.contains("1 AS record_count"),
        "一行一记录，记录数记 1\n{}",
        got.sql
    );
    assert!(
        got.sql.contains("r.occurred_at AS first_at")
            && got.sql.contains("r.occurred_at AS last_at"),
        "时间戳同时充当首末时间\n{}",
        got.sql
    );
    assert!(
        got.sql.contains("char(31)"),
        "原始文件列按最晚非空键规则现拼\n{}",
        got.sql
    );
    assert_shape(&got.sql, &got.params);
}

#[test]
fn raw_binds_time_then_dimension_params() {
    let filter = Filter {
        from: Some("2026-08-01T12:00:00Z".into()),
        to: Some("2026-08-01T18:00:00Z".into()),
        ..dims()
    };
    let got = source(&RollupPlan::Raw, &filter);
    let mut expected = texts(&["2026-08-01T12:00:00Z", "2026-08-01T18:00:00Z"]);
    expected.extend(dim_params());
    assert_eq!(got.params, expected);
    assert!(got.sql.contains("r.occurred_at >= ?"));
    assert!(got.sql.contains("r.occurred_at <= ?"));
    assert_shape(&got.sql, &got.params);
}

#[test]
fn rollup_references_table_and_applies_dimension_filters_only() {
    let filter = Filter {
        from: Some("2026-08-01T12:00:00Z".into()),
        to: Some("2026-08-10T12:00:00Z".into()),
        ..dims()
    };
    let got = source(&RollupPlan::Rollup, &filter);
    assert!(
        got.sql.contains("usage_rollup"),
        "纯预聚合应直接引用预聚合表"
    );
    assert!(!got.sql.contains("usage_records"));
    assert!(!got.sql.contains("UNION ALL"));
    assert_eq!(
        got.params,
        dim_params(),
        "纯预聚合只施加维度过滤，忽略 filter 上的时间窗"
    );
    assert!(!got.sql.contains("d.day >="), "无时间窗时不应加日条件");
    assert!(!got.sql.contains("d.day <"), "无时间窗时不应加日条件");
    assert_shape(&got.sql, &got.params);
}

#[test]
fn split_unions_complete_days_with_partial_raw() {
    let filter = Filter {
        from: Some("2026-08-01T12:00:00Z".into()),
        to: Some("2026-08-04T15:00:00Z".into()),
        ..dims()
    };
    let got = source(
        &plan(filter.from.as_deref(), filter.to.as_deref(), true, None),
        &filter,
    );
    assert!(got.sql.contains("UNION ALL"), "切分形态应 UNION ALL 合并");
    assert!(got.sql.contains("usage_rollup"));
    assert!(got.sql.contains("usage_records"));
    assert!(got.sql.contains("d.day >= ?"));
    assert!(got.sql.contains("d.day < ?"));
    assert!(got.sql.contains("r.occurred_at >= ?"));
    assert!(got.sql.contains("r.occurred_at < ?"));
    assert!(got.sql.contains("r.occurred_at <= ?"));

    let mut expected = texts(&["2026-08-02", "2026-08-04"]);
    expected.extend(dim_params());
    expected.extend(texts(&[
        "2026-08-01T12:00:00Z",
        "2026-08-02",
        "2026-08-04",
        "2026-08-04T15:00:00Z",
    ]));
    expected.extend(dim_params());
    assert_eq!(
        got.params, expected,
        "先整天再两端 partial，每段先时间后维度"
    );
    assert_shape(&got.sql, &got.params);
}

#[test]
fn split_omits_empty_partial_arms() {
    let from_aligned = Filter {
        from: Some("2026-08-01T00:00:00Z".into()),
        to: Some("2026-08-04T15:00:00Z".into()),
        ..dims()
    };
    let got = source(
        &plan(
            from_aligned.from.as_deref(),
            from_aligned.to.as_deref(),
            true,
            None,
        ),
        &from_aligned,
    );
    assert!(got.sql.contains("UNION ALL"));
    assert!(
        got.sql.contains("r.occurred_at <= ?"),
        "尾部 partial 是闭区间\n{}",
        got.sql
    );
    assert!(
        !got.sql.contains("r.occurred_at < ?"),
        "头部为空时不应出现半开上界\n{}",
        got.sql
    );
    let mut expected = texts(&["2026-08-01", "2026-08-04"]);
    expected.extend(dim_params());
    expected.extend(texts(&["2026-08-04", "2026-08-04T15:00:00Z"]));
    expected.extend(dim_params());
    assert_eq!(got.params, expected);
    assert_shape(&got.sql, &got.params);

    let both_aligned = Filter {
        from: Some("2026-08-01T00:00:00Z".into()),
        to: Some("2026-08-05T00:00:00Z".into()),
        ..Filter::default()
    };
    let got = source(
        &plan(
            both_aligned.from.as_deref(),
            both_aligned.to.as_deref(),
            true,
            None,
        ),
        &both_aligned,
    );
    assert!(
        !got.sql.contains("UNION ALL"),
        "两端都对齐时没有 partial，不应再 UNION 明细"
    );
    assert!(!got.sql.contains("usage_records"));
    assert_eq!(got.params, texts(&["2026-08-01", "2026-08-05"]));
    assert_shape(&got.sql, &got.params);
}

#[test]
fn placeholder_count_matches_params_for_all_forms() {
    let cases: Vec<(RollupPlan, Filter)> = vec![
        (RollupPlan::Raw, Filter::default()),
        (RollupPlan::Rollup, Filter::default()),
        (
            plan(None, Some("2026-08-07T15:00:00Z"), true, None),
            Filter {
                to: Some("2026-08-07T15:00:00Z".into()),
                sources: vec!["pi".into()],
                ..Filter::default()
            },
        ),
        (
            plan(Some("2026-08-01T12:00:00Z"), None, true, None),
            Filter {
                from: Some("2026-08-01T12:00:00Z".into()),
                models: vec!["gpt-5".into()],
                ..Filter::default()
            },
        ),
        (
            plan(
                Some("2026-08-01T12:00:00Z"),
                Some("2026-08-01T18:00:00Z"),
                false,
                None,
            ),
            Filter {
                from: Some("2026-08-01T12:00:00Z".into()),
                to: Some("2026-08-01T18:00:00Z".into()),
                ..dims()
            },
        ),
    ];
    for (plan, filter) in cases {
        let got = source(&plan, &filter);
        assert_shape(&got.sql, &got.params);
    }
}
