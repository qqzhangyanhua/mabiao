use std::path::{Path, PathBuf};

use crate::domain::Source;
use crate::ingest;
use crate::wsl;

fn write_dir(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
}

#[test]
fn wsl_share_roots_list_distro_user_homes_and_skip_missing_shares() {
    let temp = tempfile::tempdir().unwrap();
    let share = temp.path().join("wsl.localhost");
    write_dir(&share.join("Ubuntu/home/alice"));
    write_dir(&share.join("Ubuntu/home/bob"));
    write_dir(&share.join("Debian/home/carol"));
    write_dir(&share.join("docker-desktop/home"));
    std::fs::write(share.join("Ubuntu/home/alice/.keep"), b"").unwrap();

    let missing = temp.path().join("wsl$");
    let homes = wsl::homes_from_share_roots(&[share.clone(), missing]);
    let mut names: Vec<String> = homes
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["alice", "bob", "carol"]);
    assert!(homes.iter().all(|path| path.ends_with("home/alice")
        || path.ends_with("home/bob")
        || path.ends_with("home/carol")));
}

#[test]
fn source_scan_dirs_append_wsl_homes_unless_path_is_overridden() {
    let temp = tempfile::tempdir().unwrap();
    let windows_home = temp.path().join("win-home");
    let wsl_home = temp.path().join("wsl-home");
    write_dir(&windows_home);
    write_dir(&wsl_home.join(".codex/sessions"));

    wsl::with_extra_scan_homes(vec![wsl_home.clone()], || {
        let dirs = ingest::source_scan_dirs_with(
            &ingest::PathOverrides::new(),
            &windows_home,
            Source::Codex,
        );
        assert!(dirs.contains(&windows_home.join(".codex/sessions")));
        assert!(dirs.contains(&wsl_home.join(".codex/sessions")));
        assert!(dirs.contains(&wsl_home.join(".codex/archived_sessions")));

        let claude = ingest::source_scan_dirs_with(
            &ingest::PathOverrides::new(),
            &windows_home,
            Source::Claude,
        );
        assert!(claude.contains(&wsl_home.join(".claude/projects")));
        assert!(claude.contains(&wsl_home.join(".config/claude/projects")));

        let overridden =
            ingest::PathOverrides::from([("CODEX_HOME", vec![windows_home.join("custom-codex")])]);
        assert_eq!(
            ingest::source_scan_dirs_with(&overridden, &windows_home, Source::Codex),
            vec![
                windows_home.join("custom-codex/sessions"),
                windows_home.join("custom-codex/archived_sessions"),
            ]
        );
    });
}

#[test]
fn extra_scan_homes_are_empty_without_injected_wsl_layout() {
    assert!(wsl::extra_scan_homes().is_empty());
}

#[test]
fn cursor_agent_conversation_roots_also_cover_wsl_homes() {
    let temp = tempfile::tempdir().unwrap();
    let windows_home = temp.path().join("win-home");
    let wsl_home = temp.path().join("wsl-home");
    write_dir(&windows_home);
    write_dir(&wsl_home.join(".cursor/projects"));

    wsl::with_extra_scan_homes(vec![wsl_home.clone()], || {
        let roots = crate::conversation::catalog_roots(
            &ingest::PathOverrides::new(),
            &windows_home,
            Source::CursorAgent,
        );
        assert_eq!(
            roots,
            vec![
                windows_home.join(".cursor/projects"),
                wsl_home.join(".cursor/projects"),
            ]
        );
    });
}

#[test]
fn wsl_unc_share_candidates_are_windows_only() {
    let roots = wsl::unc_share_candidates();
    if cfg!(windows) {
        assert!(roots
            .iter()
            .any(|path| path == &PathBuf::from(r"\\wsl.localhost")));
        assert!(roots.iter().any(|path| path == &PathBuf::from(r"\\wsl$")));
    } else {
        assert!(roots.is_empty());
    }
}
