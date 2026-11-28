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
        vec![
            PathBuf::from("/ui/codex/sessions"),
            PathBuf::from("/ui/codex/archived_sessions"),
        ]
    );

    let env_only = ingest::merge_path_overrides(env, PathOverrides::new());
    assert_eq!(
        ingest::source_scan_dirs_with(&env_only, home, Source::Codex),
        vec![
            PathBuf::from("/env/codex/sessions"),
            PathBuf::from("/env/codex/archived_sessions"),
        ]
    );
}

#[test]
fn join_leaf_matches_adapter_scan_rule() {
    assert_eq!(scan_paths::join_leaf(Source::Codex), "sessions");
    assert_eq!(scan_paths::join_leaf(Source::Claude), "projects");
    assert_eq!(scan_paths::join_leaf(Source::Opencode), "opencode.db");
    assert_eq!(scan_paths::join_leaf(Source::Pi), "");
    assert_eq!(scan_paths::join_leaf(Source::CursorAgent), "");
    assert_eq!(scan_paths::join_leaf(Source::Agy), "conversations");
    assert_eq!(scan_paths::join_leaf(Source::Qoder), "projects");
    assert_eq!(scan_paths::join_leaf(Source::QoderCn), "projects");
    assert_eq!(scan_paths::join_leaf(Source::Cline), "");
    assert_eq!(scan_paths::join_leaf(Source::WorkBuddy), "projects");
    assert_eq!(scan_paths::join_leaf(Source::Zcode), "cli/db/db.sqlite");
    assert_eq!(scan_paths::join_leaf(Source::Alma), "");
    assert_eq!(scan_paths::join_leaf(Source::CodeBuddy), "projects");
    assert_eq!(scan_paths::join_leaf(Source::Trae), "cli/sessions");
    assert_eq!(scan_paths::join_leaf(Source::Goose), "data/sessions");
    assert_eq!(scan_paths::join_leaf(Source::Roo), "");
    assert_eq!(scan_paths::join_leaf(Source::Kilo), "");
    assert_eq!(scan_paths::join_leaf(Source::Amp), "threads");
    assert_eq!(scan_paths::join_leaf(Source::Zed), "threads");
    assert_eq!(scan_paths::join_leaf(Source::Cherry), "");
    assert_eq!(scan_paths::join_leaf(Source::OpenClaw), "agents");
    assert_eq!(scan_paths::join_leaf(Source::CodeBuddyIde), "");
    assert_eq!(scan_paths::join_leaf(Source::Crush), "");
    assert_eq!(scan_paths::join_leaf(Source::Aider), "");
    assert_eq!(scan_paths::join_leaf(Source::Warp), "");
    assert_eq!(scan_paths::join_leaf(Source::Windsurf), "");
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
        vec![
            custom.join("sessions").to_string_lossy().into_owned(),
            custom
                .join("archived_sessions")
                .to_string_lossy()
                .into_owned(),
        ]
    );
    assert_eq!(codex.join_leaf, "sessions");
    assert!(codex.note.contains("archived_sessions"));
    let codex_roots: Vec<_> = codex
        .default_roots
        .iter()
        .filter(|root| root.ends_with(".codex"))
        .collect();
    assert_eq!(codex_roots.len(), 1, "{:?}", codex.default_roots);

    let cursor_agent = panel
        .rows
        .iter()
        .find(|row| row.source == "cursor_agent")
        .unwrap();
    assert_eq!(cursor_agent.active, "default");
    assert!(cursor_agent.note.contains("token 包装"));

    let claude = panel
        .rows
        .iter()
        .find(|row| row.source == "claude")
        .unwrap();
    assert!(claude.note.contains("Desktop / Cowork"));
    assert_eq!(claude.join_leaf, "projects");
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

#[test]
fn detect_default_presence_marks_existing_default_dirs_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let present = home.join(".codex").join("sessions");
    std::fs::create_dir_all(&present).unwrap();
    let dto = scan_paths::detect_default_presence(home, false);
    assert!(!dto.has_usage_records);
    let codex = dto
        .rows
        .iter()
        .find(|row| row.source == "codex")
        .expect("codex row");
    assert!(
        codex
            .roots
            .iter()
            .any(|root| root.exists && root.path.contains("sessions")),
        "{codex:?}"
    );
    let missing = dto
        .rows
        .iter()
        .find(|row| row.source == "pi")
        .expect("pi row");
    assert!(missing.roots.iter().all(|root| !root.exists), "{missing:?}");
}

#[test]
fn panel_with_env_reports_clean_default_roots_asymmetric_leaves_and_wsl_homes() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("win-home");
    let wsl_home = dir.path().join("wsl-home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&wsl_home).unwrap();

    // Seed dynamic discovery fixtures on disk to verify default_roots never leak dynamic entries.
    let cowork = home
        .join("Library/Application Support/Claude/local-agent-mode-sessions/acc/org/local_1/.claude/projects");
    std::fs::create_dir_all(&cowork).unwrap();
    let opencode_dir = home.join(".local/share/opencode");
    std::fs::create_dir_all(&opencode_dir).unwrap();
    std::fs::write(opencode_dir.join("opencode.db"), b"").unwrap();
    std::fs::write(opencode_dir.join("opencode-work.db"), b"").unwrap();
    let hermes_profile = home.join(".hermes/profiles/work");
    std::fs::create_dir_all(&hermes_profile).unwrap();
    let crush_reg = home.join(".local/share/crush");
    let crush_proj = dir.path().join("crush-proj/.crush");
    std::fs::create_dir_all(&crush_reg).unwrap();
    std::fs::create_dir_all(&crush_proj).unwrap();
    std::fs::write(
        crush_reg.join("projects.json"),
        serde_json::json!({
            "projects": [{ "path": "/repo", "data_dir": crush_proj }]
        })
        .to_string(),
    )
    .unwrap();

    let env = scan_paths::ScanEnv::new(&home).with_wsl_homes([wsl_home.clone()]);
    let panel = scan_paths::panel_with_env(&env);
    let row = |slug: &str| {
        panel
            .rows
            .iter()
            .find(|item| item.source == slug)
            .unwrap_or_else(|| panic!("missing row {slug}"))
    };
    let s = |p: PathBuf| p.to_string_lossy().into_owned();

    // Trae: paired cli/sessions + cli/archived_sessions without leaking archived_sessions into default_roots.
    let trae = row("trae");
    assert_eq!(trae.join_leaf, "cli/sessions");
    assert_eq!(
        trae.default_roots,
        vec![s(home.join(".trae")), s(wsl_home.join(".trae"))]
    );
    assert_eq!(
        trae.effective_scan_dirs,
        vec![
            s(home.join(".trae/cli/sessions")),
            s(home.join(".trae/cli/archived_sessions")),
            s(wsl_home.join(".trae/cli/sessions")),
            s(wsl_home.join(".trae/cli/archived_sessions")),
        ]
    );

    // Codex: paired sessions + archived_sessions across primary + WSL.
    let codex = row("codex");
    assert_eq!(codex.join_leaf, "sessions");
    assert_eq!(
        codex.default_roots,
        vec![s(home.join(".codex")), s(wsl_home.join(".codex"))]
    );
    assert_eq!(
        codex.effective_scan_dirs,
        vec![
            s(home.join(".codex/sessions")),
            s(home.join(".codex/archived_sessions")),
            s(wsl_home.join(".codex/sessions")),
            s(wsl_home.join(".codex/archived_sessions")),
        ]
    );

    // Goose: distinct override leaf (data/sessions) vs 5 platform-default session paths.
    let goose = row("goose");
    assert_eq!(goose.join_leaf, "data/sessions");
    assert_eq!(
        goose.default_roots,
        vec![
            s(home.join(".local/share/goose")),
            s(home.join("Library/Application Support/goose")),
            s(home.join("Library/Application Support/Block/goose")),
            s(home.join(".local/share/Block/goose")),
            s(home.join("AppData/Roaming/Block/goose")),
            s(wsl_home.join(".local/share/goose")),
            s(wsl_home.join("Library/Application Support/goose")),
            s(wsl_home.join("Library/Application Support/Block/goose")),
            s(wsl_home.join(".local/share/Block/goose")),
            s(wsl_home.join("AppData/Roaming/Block/goose")),
        ]
    );
    assert_eq!(
        goose.effective_scan_dirs,
        vec![
            s(home.join(".local/share/goose/sessions")),
            s(home.join("Library/Application Support/goose/sessions")),
            s(home.join("Library/Application Support/Block/goose/sessions")),
            s(home.join(".local/share/Block/goose/sessions")),
            s(home.join("AppData/Roaming/Block/goose/data/sessions")),
            s(wsl_home.join(".local/share/goose/sessions")),
            s(wsl_home.join("Library/Application Support/goose/sessions")),
            s(wsl_home.join("Library/Application Support/Block/goose/sessions")),
            s(wsl_home.join(".local/share/Block/goose/sessions")),
            s(wsl_home.join("AppData/Roaming/Block/goose/data/sessions")),
        ]
    );

    // Amp & Zed: clean platform default roots + threads leaf (primary + WSL).
    let amp = row("amp");
    assert_eq!(amp.join_leaf, "threads");
    assert_eq!(
        amp.default_roots,
        vec![
            s(home.join(".local/share/amp")),
            s(home.join("Library/Application Support/amp")),
            s(home.join("AppData/Roaming/amp")),
            s(wsl_home.join(".local/share/amp")),
            s(wsl_home.join("Library/Application Support/amp")),
            s(wsl_home.join("AppData/Roaming/amp")),
        ]
    );
    assert_eq!(
        amp.effective_scan_dirs,
        vec![
            s(home.join(".local/share/amp/threads")),
            s(home.join("Library/Application Support/amp/threads")),
            s(home.join("AppData/Roaming/amp/threads")),
            s(wsl_home.join(".local/share/amp/threads")),
            s(wsl_home.join("Library/Application Support/amp/threads")),
            s(wsl_home.join("AppData/Roaming/amp/threads")),
        ]
    );
    let zed = row("zed");
    assert_eq!(zed.join_leaf, "threads");
    assert_eq!(
        zed.default_roots,
        vec![
            s(home.join(".local/share/zed")),
            s(home.join("Library/Application Support/Zed")),
            s(home.join("AppData/Local/Zed")),
            s(wsl_home.join(".local/share/zed")),
            s(wsl_home.join("Library/Application Support/Zed")),
            s(wsl_home.join("AppData/Local/Zed")),
        ]
    );
    assert_eq!(
        zed.effective_scan_dirs,
        vec![
            s(home.join(".local/share/zed/threads")),
            s(home.join("Library/Application Support/Zed/threads")),
            s(home.join("AppData/Local/Zed/threads")),
            s(wsl_home.join(".local/share/zed/threads")),
            s(wsl_home.join("Library/Application Support/Zed/threads")),
            s(wsl_home.join("AppData/Local/Zed/threads")),
        ]
    );

    // Goose UI override uses data/sessions leaf and suppresses WSL defaults.
    let custom_goose = dir.path().join("custom-goose");
    let goose_ui_env = env.with_ui_config(ScanPathConfig {
        overrides: roots("goose", &[custom_goose.to_str().unwrap()]),
    });
    let goose_ui_panel = scan_paths::panel_with_env(&goose_ui_env);
    let goose_ui = goose_ui_panel
        .rows
        .iter()
        .find(|item| item.source == "goose")
        .unwrap();
    assert_eq!(goose_ui.active, "ui");
    assert_eq!(
        goose_ui.effective_scan_dirs,
        vec![s(custom_goose.join("data/sessions"))]
    );

    // Claude: default_roots never contain Cowork nested dirs, effective_scan_dirs do.
    let claude = row("claude");
    assert_eq!(
        claude.default_roots,
        vec![
            s(home.join(".claude")),
            s(home.join(".config/claude")),
            s(wsl_home.join(".claude")),
            s(wsl_home.join(".config/claude")),
        ]
    );
    assert!(claude.effective_scan_dirs.contains(&s(cowork.clone())));
    assert!(claude
        .effective_scan_dirs
        .contains(&s(wsl_home.join(".claude/projects"))));

    // OpenCode: default_roots never leak opencode-*.db filenames; effective_scan_dirs list them.
    let opencode = row("opencode");
    assert_eq!(
        opencode.default_roots,
        vec![
            s(home.join(".local/share/opencode")),
            s(wsl_home.join(".local/share/opencode")),
        ]
    );
    assert!(opencode
        .effective_scan_dirs
        .contains(&s(opencode_dir.join("opencode.db"))));
    assert!(opencode
        .effective_scan_dirs
        .contains(&s(opencode_dir.join("opencode-work.db"))));
    assert!(opencode
        .effective_scan_dirs
        .contains(&s(wsl_home.join(".local/share/opencode/opencode.db"))));

    // Hermes & Crush: default_roots stay base roots, effective_scan_dirs include dynamic expansions.
    let hermes = row("hermes");
    assert_eq!(
        hermes.default_roots,
        vec![s(home.join(".hermes")), s(wsl_home.join(".hermes"))]
    );
    assert!(hermes.effective_scan_dirs.contains(&s(hermes_profile)));
    let crush = row("crush");
    assert_eq!(
        crush.default_roots,
        vec![
            s(home.join(".local/share/crush")),
            s(home.join("AppData/Local/crush")),
            s(wsl_home.join(".local/share/crush")),
            s(wsl_home.join("AppData/Local/crush")),
        ]
    );
    assert!(crush.effective_scan_dirs.contains(&s(crush_proj)));
}

#[test]
fn scan_env_opencode_db_suppresses_wsl_and_marks_panel_active_env() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("win-home");
    let wsl_home = dir.path().join("wsl-home");
    let custom_db = dir.path().join("external/custom.db");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(wsl_home.join(".local/share/opencode")).unwrap();
    std::fs::write(wsl_home.join(".local/share/opencode/opencode.db"), b"").unwrap();
    std::fs::create_dir_all(custom_db.parent().unwrap()).unwrap();
    std::fs::write(&custom_db, b"").unwrap();

    let env = scan_paths::ScanEnv::new(&home)
        .with_wsl_homes([wsl_home.clone()])
        .with_opencode_db(&custom_db);
    assert_eq!(
        env.usage_scan_dirs(Source::Opencode),
        vec![custom_db.clone()]
    );
    assert_eq!(
        env.conversation_trust_roots(Source::Opencode),
        vec![custom_db.parent().unwrap().to_path_buf(), custom_db.clone()]
    );

    let panel = scan_paths::panel_with_env(&env);
    let opencode = panel
        .rows
        .iter()
        .find(|row| row.source == "opencode")
        .unwrap();
    assert_eq!(opencode.active, "env");
    assert_eq!(
        opencode.effective_scan_dirs,
        vec![custom_db.to_string_lossy().into_owned()]
    );
    assert!(
        !opencode
            .effective_scan_dirs
            .iter()
            .any(|p| p.contains("wsl-home")),
        "OPENCODE_DB must suppress WSL fallback: {:?}",
        opencode.effective_scan_dirs
    );

    // Relative OPENCODE_DB also suppresses WSL fallback.
    let rel_env = scan_paths::ScanEnv::new(&home)
        .with_wsl_homes([wsl_home])
        .with_opencode_db("sessions.db");
    assert_eq!(
        rel_env.usage_scan_dirs(Source::Opencode),
        vec![home.join(".local/share/opencode/sessions.db")]
    );

    // UI override + relative OPENCODE_DB reports active = "ui" and joins DB name onto UI root.
    let ui_root = dir.path().join("ui-opencode");
    let ui_and_db_env = rel_env.with_ui_config(ScanPathConfig {
        overrides: roots("opencode", &[ui_root.to_str().unwrap()]),
    });
    let ui_panel = scan_paths::panel_with_env(&ui_and_db_env);
    let ui_row = ui_panel
        .rows
        .iter()
        .find(|row| row.source == "opencode")
        .unwrap();
    assert_eq!(ui_row.active, "ui");
    assert_eq!(
        ui_row.effective_scan_dirs,
        vec![ui_root.join("sessions.db").to_string_lossy().into_owned()]
    );
}

#[test]
fn scan_env_claude_override_keeps_desktop_cowork_and_suppresses_wsl() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("win-home");
    let wsl_home = dir.path().join("wsl-home");
    let custom = dir.path().join("custom-claude");
    let cowork = home
        .join("Library/Application Support/Claude/local-agent-mode-sessions/acc/org/local_1/.claude/projects");
    std::fs::create_dir_all(&cowork).unwrap();
    std::fs::create_dir_all(wsl_home.join(".claude/projects")).unwrap();

    let env = scan_paths::ScanEnv::new(&home)
        .with_wsl_homes([wsl_home.clone()])
        .with_env_override("CLAUDE_CONFIG_DIR", vec![custom.clone()]);
    let dirs = env.usage_scan_dirs(Source::Claude);
    assert_eq!(dirs, vec![custom.join("projects"), cowork]);
    assert!(!dirs.iter().any(|p| p.starts_with(&wsl_home)));
}

#[test]
fn scan_env_cursor_agent_override_keeps_shared_cursor_dirs_and_wsl_roots() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("win-home");
    let wsl_home = dir.path().join("wsl-home");
    let custom_usage = dir.path().join("custom-usage");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&wsl_home).unwrap();
    std::fs::create_dir_all(&custom_usage).unwrap();

    let env = scan_paths::ScanEnv::new(&home)
        .with_wsl_homes([wsl_home.clone()])
        .with_env_override("CURSOR_AGENT_USAGE_DIR", vec![custom_usage.clone()]);

    assert_eq!(
        env.usage_scan_dirs(Source::CursorAgent),
        vec![custom_usage.clone()]
    );
    assert_eq!(
        env.display_dirs(Source::CursorAgent),
        vec![
            home.join(".cursor/chats"),
            home.join(".cursor/projects"),
            wsl_home.join(".cursor/chats"),
            wsl_home.join(".cursor/projects"),
            custom_usage,
        ]
    );
    assert_eq!(
        env.conversation_catalog_roots(Source::CursorAgent),
        vec![
            home.join(".cursor/projects"),
            wsl_home.join(".cursor/projects"),
        ]
    );
    assert_eq!(
        env.cursor_session_roots(),
        vec![
            home.join(".cursor/projects"),
            wsl_home.join(".cursor/projects"),
        ]
    );
}

#[test]
fn scan_env_discovers_wsl_homes_at_most_once_per_snapshot() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("win-home");
    let wsl_home = dir.path().join("wsl-home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&wsl_home).unwrap();

    let calls = AtomicUsize::new(0);
    let env =
        scan_paths::ScanEnv::capture_with_wsl(&home, &dir.path().join("missing.json"), || {
            calls.fetch_add(1, Ordering::SeqCst);
            vec![wsl_home.clone()]
        });

    for source in Source::ALL {
        let _ = env.usage_scan_dirs(source);
        let _ = env.display_dirs(source);
        let _ = env.default_roots(source);
        let _ = env.conversation_catalog_roots(source);
        let _ = env.conversation_trust_roots(source);
    }
    let _ = env.cursor_session_roots();
    let _ = scan_paths::panel_with_env(&env);
    let _ = scan_paths::detect_default_presence_with_env(&env, false);

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "WSL home discovery must run at most once per ScanEnv snapshot"
    );
}

#[test]
fn scan_env_client_config_roots_honor_precedence_and_first_valid_credential() {
    use crate::official_quota::{claude_usage, codex_usage, grok, hook, opencode};

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("win-home");
    let wsl_home = dir.path().join("wsl-home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&wsl_home).unwrap();

    // 1. Codex: multi-root env override selects first root containing auth.json; UI override beats env;
    //    unoverridden falls back to WSL home when primary has no auth.json.
    let codex_a = dir.path().join("codex-a");
    let codex_b = dir.path().join("codex-b");
    let codex_ui = dir.path().join("codex-ui");
    std::fs::create_dir_all(&codex_a).unwrap();
    std::fs::create_dir_all(&codex_b).unwrap();
    std::fs::create_dir_all(&codex_ui).unwrap();
    std::fs::write(codex_b.join("auth.json"), "{}").unwrap();
    std::fs::write(codex_ui.join("auth.json"), "{}").unwrap();
    std::fs::create_dir_all(wsl_home.join(".codex")).unwrap();
    std::fs::write(wsl_home.join(".codex/auth.json"), "{}").unwrap();

    let codex_env = scan_paths::ScanEnv::new(&home)
        .with_wsl_homes([wsl_home.clone()])
        .with_env_override("CODEX_HOME", vec![codex_a.clone(), codex_b.clone()]);
    assert_eq!(
        codex_usage::auth_path_in(&codex_env),
        codex_b.join("auth.json")
    );

    let codex_ui_env = codex_env.with_ui_config(ScanPathConfig {
        overrides: roots("codex", &[codex_ui.to_str().unwrap()]),
    });
    assert_eq!(
        codex_usage::auth_path_in(&codex_ui_env),
        codex_ui.join("auth.json")
    );

    let codex_default_wsl = scan_paths::ScanEnv::new(&home).with_wsl_homes([wsl_home.clone()]);
    assert_eq!(
        codex_usage::auth_path_in(&codex_default_wsl),
        wsl_home.join(".codex/auth.json")
    );

    // 2. Grok: multi-root env picks second root with auth.json, and grok_home_in matches that root.
    let grok_a = dir.path().join("grok-a");
    let grok_b = dir.path().join("grok-b");
    std::fs::create_dir_all(&grok_a).unwrap();
    std::fs::create_dir_all(&grok_b).unwrap();
    std::fs::write(grok_b.join("auth.json"), "{}").unwrap();
    let grok_env = scan_paths::ScanEnv::new(&home)
        .with_env_override("GROK_HOME", vec![grok_a, grok_b.clone()]);
    assert_eq!(grok::grok_home_in(&grok_env), grok_b);
    assert_eq!(grok::auth_path_in(&grok_env), grok_b.join("auth.json"));

    // 3. Claude: credentials prefer configured root with file, fall back to default home,
    //    while statusline hook always uses the primary configured root (UI > env > ~/.claude).
    let claude_a = dir.path().join("claude-a");
    let claude_b = dir.path().join("claude-b");
    let claude_ui = dir.path().join("claude-ui");
    std::fs::create_dir_all(&claude_a).unwrap();
    std::fs::create_dir_all(&claude_b).unwrap();
    std::fs::create_dir_all(&claude_ui).unwrap();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join(".claude/.credentials.json"), "{}").unwrap();
    std::fs::write(claude_b.join(".credentials.json"), "{}").unwrap();

    let claude_env = scan_paths::ScanEnv::new(&home)
        .with_wsl_homes([wsl_home.clone()])
        .with_env_override(
            "CLAUDE_CONFIG_DIR",
            vec![claude_a.clone(), claude_b.clone()],
        );
    assert_eq!(
        claude_usage::first_existing_credentials_in(&claude_env),
        claude_b.join(".credentials.json")
    );
    assert_eq!(
        hook::default_settings_path_in(&claude_env),
        claude_a.join("settings.json")
    );

    let claude_ui_env = claude_env.with_ui_config(ScanPathConfig {
        overrides: roots("claude", &[claude_ui.to_str().unwrap()]),
    });
    assert_eq!(
        claude_usage::first_existing_credentials_in(&claude_ui_env),
        home.join(".claude/.credentials.json")
    );
    assert_eq!(
        hook::default_settings_path_in(&claude_ui_env),
        claude_ui.join("settings.json")
    );

    let claude_default = scan_paths::ScanEnv::new(&home).with_wsl_homes([wsl_home.clone()]);
    assert_eq!(
        hook::default_settings_path_in(&claude_default),
        home.join(".claude/settings.json")
    );

    // 4. OpenCode: UI override > OPENCODE_DATA_DIR > XDG_DATA_HOME/opencode > default.
    let xdg_root = dir.path().join("xdg");
    let xdg_opencode = xdg_root.join("opencode");
    let oc_ui = dir.path().join("oc-ui");
    std::fs::create_dir_all(&xdg_opencode).unwrap();
    std::fs::create_dir_all(&oc_ui).unwrap();
    std::fs::write(xdg_opencode.join("auth.json"), "{}").unwrap();
    std::fs::write(oc_ui.join("auth.json"), "{}").unwrap();

    let oc_xdg = scan_paths::ScanEnv::new(&home).with_xdg_data_home(&xdg_root);
    assert_eq!(
        opencode::auth_path_in(&oc_xdg),
        xdg_opencode.join("auth.json")
    );
    let oc_ui_env = oc_xdg.with_ui_config(ScanPathConfig {
        overrides: roots("opencode", &[oc_ui.to_str().unwrap()]),
    });
    assert_eq!(opencode::auth_path_in(&oc_ui_env), oc_ui.join("auth.json"));
}
