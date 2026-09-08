//! 项目树内指令文件的共用走盘。`conflict` 与 Cursor 上下文清单必须用同一套
//! 相对路径，避免「全局指令重叠」和「可能生效」各扫各的。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{TimeZone, Utc};

/// 与 `conflict::ROOT_NAMES` 里 Cursor 会吃的那几份对齐。
pub const CURSOR_PROJECT_INSTRUCTION_NAMES: &[&str] =
    &["AGENTS.md", "AGENTS.override.md", ".cursorrules"];

pub const CURSOR_RULES_DIR: &str = ".cursor/rules";
pub const CURSOR_SKILLS_DIR: &str = ".cursor/skills";
pub const CURSOR_MCP_REL: &str = ".cursor/mcp.json";
pub const CURSOR_USER_MCP_REL: &str = ".cursor/mcp.json";
pub const CURSOR_USER_SKILLS_DIR: &str = ".cursor/skills";

pub fn existing_file(root: &Path, rel: &Path) -> Option<PathBuf> {
    let path = root.join(rel);
    path.is_file().then_some(path)
}

pub fn walk_files(root: &Path, rel: &Path, depth: u8, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(&mut out, &root.join(rel), depth, &keep);
    out.sort();
    out
}

fn walk(out: &mut Vec<PathBuf>, dir: &Path, depth: u8, keep: &impl Fn(&str) -> bool) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(out, &path, depth.saturating_sub(1), keep);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if keep(name) {
            out.push(path);
        }
    }
}

pub fn is_cursor_rule_file(name: &str) -> bool {
    name.ends_with(".md") || name.ends_with(".mdc") || name.ends_with(".instructions.md")
}

pub fn is_skill_file(name: &str) -> bool {
    name.eq_ignore_ascii_case("SKILL.md")
}

pub fn file_stat(path: &Path) -> Option<(u64, Option<String>)> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    Some((meta.len(), mtime_rfc3339(&meta)))
}

pub fn mtime_rfc3339(meta: &fs::Metadata) -> Option<String> {
    let modified = meta.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    Utc.timestamp_opt(duration.as_secs() as i64, duration.subsec_nanos())
        .single()
        .map(|time| time.to_rfc3339())
}
