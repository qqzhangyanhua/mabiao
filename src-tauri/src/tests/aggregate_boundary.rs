use std::fs;
use std::path::{Path, PathBuf};

/// 生产源码不得引用 `aggregate`。模块声明本身不算引用。
#[test]
fn production_source_does_not_reference_aggregate() {
    let files = production_rust_sources();
    assert!(!files.is_empty(), "应读到 src/ 下的生产 Rust 源文件");
    assert!(
        files.iter().any(|(name, _)| name == "lib.rs"),
        "扫描范围必须包含 lib.rs，实际：{:?}",
        file_names(&files)
    );
    assert!(
        files.iter().any(|(name, _)| name == "query/series.rs"),
        "扫描范围必须包含 query/series.rs，实际：{:?}",
        file_names(&files)
    );
    assert!(
        files
            .iter()
            .all(|(name, _)| !name.starts_with("tests/") && !name.starts_with("test_support/")),
        "扫描范围不得包含测试模块或测试辅助：{:?}",
        file_names(&files)
    );

    let hits = inspect_aggregate_refs(&files);
    assert!(
        hits.is_empty(),
        "生产源码不得引用 aggregate（无豁免清单）：\n{}",
        hits.join("\n")
    );
}

#[test]
fn inspect_flags_path_reference_to_aggregate() {
    let hits = inspect_aggregate_refs(&[(
        "query/series.rs",
        "Ok(crate::aggregate::attach_cursor_trend(\n",
    )]);
    assert_eq!(hits, vec!["query/series.rs:1"]);
}

#[test]
fn inspect_flags_grouped_use_of_aggregate() {
    let hits =
        inspect_aggregate_refs(&[("query/analytics.rs", "use crate::{aggregate, query};\n")]);
    assert_eq!(hits, vec!["query/analytics.rs:1"]);
}

#[test]
fn inspect_allows_mod_declaration_and_comments() {
    let source = "\
#[cfg(test)]
pub mod aggregate;
// crate::aggregate::trend 是预言机，测试才引用
";
    assert!(
        inspect_aggregate_refs(&[("lib.rs", source)]).is_empty(),
        "模块声明与注释不应算引用"
    );
}

fn inspect_aggregate_refs<N, S>(files: &[(N, S)]) -> Vec<String>
where
    N: AsRef<str>,
    S: AsRef<str>,
{
    let mut hits = Vec::new();
    for (file, source) in files {
        for (index, line) in source.as_ref().lines().enumerate() {
            if line_references_aggregate(line) {
                hits.push(format!("{}:{}", file.as_ref(), index + 1));
            }
        }
    }
    hits
}

fn line_references_aggregate(line: &str) -> bool {
    let code = strip_line_comment(line);
    if is_aggregate_mod_declaration(code) {
        return false;
    }
    code.contains("::aggregate") || code.contains("aggregate::") || is_use_importing_aggregate(code)
}

fn is_use_importing_aggregate(code: &str) -> bool {
    let trimmed = code.trim_start();
    let rest = trimmed
        .strip_prefix("pub(crate) use ")
        .or_else(|| trimmed.strip_prefix("pub(super) use "))
        .or_else(|| trimmed.strip_prefix("pub use "))
        .or_else(|| trimmed.strip_prefix("use "))
        .unwrap_or("");
    !rest.is_empty() && contains_ident(rest, "aggregate")
}

fn contains_ident(code: &str, ident: &str) -> bool {
    let bytes = code.as_bytes();
    let mut start = 0;
    while let Some(offset) = code[start..].find(ident) {
        let index = start + offset;
        let end = index + ident.len();
        let prev_ok = index == 0 || !is_ident_char(bytes[index - 1]);
        let next_ok = end >= bytes.len() || !is_ident_char(bytes[end]);
        if prev_ok && next_ok {
            return true;
        }
        start = index + 1;
    }
    false
}

fn is_ident_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(index) => &line[..index],
        None => line,
    }
}

fn is_aggregate_mod_declaration(code: &str) -> bool {
    let trimmed = code.trim().trim_end_matches(';').trim();
    let name = trimmed
        .strip_prefix("pub(crate) ")
        .or_else(|| trimmed.strip_prefix("pub(super) "))
        .or_else(|| trimmed.strip_prefix("pub "))
        .unwrap_or(trimmed);
    name == "mod aggregate"
}

fn production_rust_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_production_rust(&root, &root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn collect_production_rust(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
    let mut entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", dir.display()))
        .map(|entry| entry.expect("读取源码目录项").path())
        .collect::<Vec<PathBuf>>();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if name == "tests" || name == "test_support" {
                continue;
            }
            collect_production_rust(root, &path, files);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .expect("源文件应位于 src 目录内")
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", path.display()));
        files.push((relative, source));
    }
}

fn file_names(files: &[(String, String)]) -> Vec<&str> {
    files.iter().map(|(name, _)| name.as_str()).collect()
}
