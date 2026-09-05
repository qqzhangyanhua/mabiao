use std::fs;
use std::path::{Path, PathBuf};

use crate::test_support::conversation_boundary::{
    inspect_conversation_boundary, ConversationBoundaryRule,
};

#[test]
fn inspect_accepts_compliant_sample() {
    let source = "\
use super::toolbox::{ParsedConversation, FileIndexCursor};
use super::event_index;
use super::persist;
";
    let violations = inspect_conversation_boundary([("read.rs", source)]);
    assert!(violations.is_empty(), "合规样本不应报违规：{violations:?}");
}

#[test]
fn inspect_reports_super_wildcard_line() {
    let source = "\
use std::path::Path;

use super::*;

fn load() {}
";
    let violations = inspect_conversation_boundary([("read.rs", source)]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].file, "read.rs");
    assert_eq!(violations[0].line, 3);
    assert_eq!(
        violations[0].rule,
        ConversationBoundaryRule::NoSuperWildcard
    );
    assert_eq!(violations[0].symbol, "use super::*");
    assert_eq!(
        violations[0].to_string(),
        "read.rs:3: no_super_wildcard (use super::*)"
    );
}

#[test]
fn inspect_allows_sibling_globs_and_line_comments() {
    let source = "\
use super::toolbox::*;
// use super::*;
use super::{persist, read};
";
    let violations = inspect_conversation_boundary([("codex.rs", source)]);
    assert!(
        violations.is_empty(),
        "兄弟模块 glob 与注释不应报规则一：{violations:?}"
    );
}

#[test]
fn conversation_directory_has_no_super_wildcard() {
    let files = conversation_module_sources();
    assert!(
        !files.is_empty(),
        "应读到 src/conversation 下的 Rust 源文件"
    );
    assert!(
        files.iter().any(|(name, _)| name == "read.rs"),
        "扫描范围必须包含 conversation/read.rs，实际：{:?}",
        files
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        files.iter().all(
            |(name, _)| Path::new(name).extension().is_some_and(|ext| ext == "rs")
                && Path::new(name)
                    .components()
                    .all(|component| matches!(component, std::path::Component::Normal(_)))
        ),
        "扫描范围只应包含 conversation 目录内的相对路径：{:?}",
        files
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
    );

    let violations = inspect_conversation_boundary(files);
    assert!(
        violations.is_empty(),
        "conversation 模块边界违规：\n{}",
        violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn conversation_module_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/conversation");
    let mut files = Vec::new();
    collect_rust_sources(&root, &root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn collect_rust_sources(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
    let mut entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", dir.display()))
        .map(|entry| entry.expect("读取 conversation 目录项").path())
        .collect::<Vec<PathBuf>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_rust_sources(root, &path, files);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .expect("源文件应位于 conversation 目录内")
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", path.display()));
        files.push((relative, source));
    }
}
