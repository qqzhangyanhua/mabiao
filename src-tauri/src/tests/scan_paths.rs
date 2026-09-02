use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::backup;
use crate::domain::Source;
use crate::ingest::{self, PathOverrides};
use crate::scan_paths::{self, ScanPathConfig};
use crate::store;
use crate::test_support::*;

fn roots(source: &str, paths: &[&str]) -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([(
        source.to_string(),
        paths.iter().map(|path| (*path).to_string()).collect(),
    )])
}

#[test]
fn normalize_expands_home_and_rejects_relative_paths() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let extra = home.join("abs-codex");
    let extra_str = extra.to_string_lossy().into_owned();
    let config =
        scan_paths::normalize(roots("codex", &["~/work/codex", extra_str.as_str()]), home).unwrap();
    assert_eq!(
        config.overrides.get("codex").unwrap(),
        &vec![
            home.join("work/codex").to_string_lossy().into_owned(),
            extra_str
        ]
    );

    let error = scan_paths::normalize(roots("codex", &["relative/codex"]), home).unwrap_err();
    assert!(error.contains("绝对路径"), "{error}");
    assert!(error.contains("relative/codex"), "{error}");
}

#[test]
fn normalize_rejects_unknown_source_and_drops_empty_entries() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let extra = home.join("abs-codex");
    let extra_str = extra.to_string_lossy().into_owned();
    let error =
        scan_paths::normalize(roots("not-a-source", &[extra_str.as_str()]), home).unwrap_err();
    assert!(error.contains("未知来源"), "{error}");

    let config = scan_paths::normalize(
        roots("codex", &["  ", extra_str.as_str(), extra_str.as_str()]),
        home,
    )
    .unwrap();
    assert_eq!(config.overrides.get("codex").unwrap(), &vec![extra_str]);

    let cleared = scan_paths::normalize(roots("codex", &[]), home).unwrap();
    assert!(cleared.overrides.is_empty());
}

#[test]
fn ui_override_wins_over_env_and_empty_ui_keeps_env() {
    let home = Path::new("/home/example");
    let env = PathOverrides::from([("CODEX_HOME", vec![PathBuf::from("/env/codex")])]);
    let file = scan_paths::config_to_overrides(&ScanPathConfig {
        overrides: roots("codex", &["/ui/codex"]),
    });
    let merged = ingest::merge_path_overrides(env.clone(), file);
    assert_eq!(
        ingest::source_scan_dirs_with(&merged, home, Source::Codex),
        vec![PathBuf::from("/ui/codex/sessions")]
    );

    let env_only = ingest::merge_path_overrides(env, PathOverrides::new());
    assert_eq!(
        ingest::source_scan_dirs_with(&env_only, home, Source::Codex),
        vec![PathBuf::from("/env/codex/sessions")]
    );
}

#[test]
fn join_leaf_matches_adapter_scan_rule() {
    assert_eq!(scan_paths::join_leaf(Source::Codex), "sessions");
    assert_eq!(scan_paths::join_leaf(Source::Claude), "projects");
    assert_eq!(scan_paths::join_leaf(Source::Opencode), "opencode.db");
    assert_eq!(scan_paths::join_leaf(Source::Pi), "");
    assert_eq!(scan_paths::join_leaf(Source::CursorAgent), "");
}

#[test]
fn panel_reports_ui_layer_and_effective_scan_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path = dir.path().join("scan_paths.json");
    let custom = home.join("custom/codex");
    let config = scan_paths::normalize(
        roots("codex", &[custom.to_str().expect("utf-8 path")]),
        home,
    )
    .unwrap();
    scan_paths::save(&path, &config).unwrap();

    let panel = scan_paths::panel(&path, home);
    let codex = panel.rows.iter().find(|row| row.source == "codex").unwrap();
    assert_eq!(codex.env_var, "CODEX_HOME");
    assert_eq!(codex.active, "ui");
    assert_eq!(
        codex.override_roots,
        vec![custom.to_string_lossy().into_owned()]
    );
    assert_eq!(
        codex.effective_scan_dirs,
        vec![custom.join("sessions").to_string_lossy().into_owned()]
    );
    assert_eq!(codex.join_leaf, "sessions");
    assert!(
        codex
            .default_roots
            .iter()
            .any(|root| root.ends_with(".codex")),
        "{:?}",
        codex.default_roots
    );

    let cursor_agent = panel
        .rows
        .iter()
        .find(|row| row.source == "cursor_agent")
        .unwrap();
    assert_eq!(cursor_agent.active, "default");
    assert!(cursor_agent.note.contains("token 包装"));
}

#[test]
fn corrupt_config_loads_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scan_paths.json");
    std::fs::write(&path, "{not-json").unwrap();
    assert_eq!(scan_paths::load(&path), ScanPathConfig::default());
}

#[test]
fn ingest_uses_ui_roots_instead_of_default_home() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();

    let default_sessions = home.join(".codex/sessions");
    std::fs::create_dir_all(&default_sessions).unwrap();
    std::fs::write(
        default_sessions.join("ignored.jsonl"),
        fixture("codex.jsonl"),
    )
    .unwrap();

    let custom = home.join("elsewhere/codex");
    std::fs::create_dir_all(custom.join("sessions")).unwrap();
    std::fs::write(custom.join("sessions/kept.jsonl"), fixture("codex.jsonl")).unwrap();

    let file = scan_paths::config_to_overrides(&ScanPathConfig {
        overrides: roots("codex", &[custom.to_str().unwrap()]),
    });
    let conn = store::open_memory().unwrap();
    let report = ingest::ingest_all_with_overrides(&conn, home, &file).unwrap();
    assert_eq!(report.files_parsed, 1);
    let records = store::load_all(&conn).unwrap();
    assert!(records
        .iter()
        .all(|record| record.source_file.contains("elsewhere")));
    assert!(records
        .iter()
        .all(|record| !record.source_file.contains("ignored.jsonl")));
}

fn backup_paths(live: &Path) -> backup::AppDataPaths {
    backup::AppDataPaths {
        db_path: live.join("usage.sqlite"),
        prices_path: live.join("prices.json"),
        snapshot_path: live.join("litellm_prices.json"),
        budget_path: live.join("budget.json"),
        budget_notify_path: live.join("budget_notify_state.json"),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    }
}

#[test]
fn backup_round_trips_scan_path_overrides() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();

    let paths = backup_paths(&live);
    let conn = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    let custom = live.join("custom-codex");
    let config = scan_paths::normalize(
        roots("codex", &[custom.to_str().expect("utf-8 path")]),
        &live,
    )
    .unwrap();
    scan_paths::save(&live.join(scan_paths::CONFIG_NAME), &config).unwrap();

    let manifest = backup::backup_to(&conn, &dest, &paths).unwrap();
    assert!(
        manifest
            .files
            .iter()
            .any(|name| name == scan_paths::CONFIG_NAME),
        "{manifest:?}"
    );
    drop(conn);

    std::fs::remove_file(live.join(scan_paths::CONFIG_NAME)).unwrap();
    let restored = backup::restore_from(&dest, &paths).unwrap();
    assert!(restored
        .files
        .iter()
        .any(|name| name == scan_paths::CONFIG_NAME));
    let loaded = scan_paths::load(&live.join(scan_paths::CONFIG_NAME));
    assert_eq!(
        loaded.overrides.get("codex").unwrap(),
        &vec![custom.to_string_lossy().into_owned()]
    );
}
