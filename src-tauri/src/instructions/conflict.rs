use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::{
    GlobalInstructionFile, GlobalInstructionSourceRow, InstructionEntryKind, InstructionLoadStatus,
    InstructionOverlapHint,
};

const ROOT_NAMES: &[&str] = &[
    "AGENTS.md",
    "AGENTS.override.md",
    "CLAUDE.md",
    "CLAUDE.local.md",
    "GEMINI.md",
    "QWEN.md",
    "COPILOT.md",
    ".cursorrules",
];

const STOPWORDS: &[&str] = &[
    "also", "always", "avoid", "been", "both", "code", "does", "each", "file", "files", "from",
    "have", "into", "just", "like", "make", "more", "must", "need", "never", "only", "other",
    "please", "prefer", "same", "should", "such", "text", "than", "that", "them", "then", "this",
    "used", "using", "very", "when", "will", "with", "your",
];

pub fn collect(
    sources: &[GlobalInstructionSourceRow],
    project_root: Option<&Path>,
) -> (Option<String>, Vec<InstructionOverlapHint>) {
    let Some(root) = project_root.filter(|path| path.is_dir()) else {
        return (None, Vec::new());
    };
    let selected = Some(root.to_string_lossy().into_owned());
    let project_files = project_rule_files(root);
    if project_files.is_empty() {
        return (selected, Vec::new());
    }

    let mut hints = Vec::new();
    for row in sources {
        for file in &row.files {
            if !is_loaded_text(file) {
                continue;
            }
            let global_keys = keywords(&file.content);
            if global_keys.is_empty() {
                continue;
            }
            for (project_path, project_text) in &project_files {
                for keyword in global_keys.intersection(&keywords(project_text)) {
                    hints.push(InstructionOverlapHint {
                        keyword: keyword.clone(),
                        global_application: row.application.clone(),
                        global_display_path: file.display_path.clone(),
                        global_snippet: snippet(&file.content, keyword),
                        project_display_path: project_path.clone(),
                        project_snippet: snippet(project_text, keyword),
                    });
                }
            }
        }
    }
    hints.sort_by(|a, b| {
        a.keyword
            .cmp(&b.keyword)
            .then_with(|| a.global_display_path.cmp(&b.global_display_path))
            .then_with(|| a.project_display_path.cmp(&b.project_display_path))
    });
    (selected, hints)
}

fn is_loaded_text(file: &GlobalInstructionFile) -> bool {
    file.kind == InstructionEntryKind::File
        && file.load_status == InstructionLoadStatus::Loaded
        && !file.content.is_empty()
}

fn project_rule_files(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    for name in ROOT_NAMES {
        push_file(&mut files, root, Path::new(name));
    }
    collect_dir(&mut files, root, Path::new(".cursor/rules"), 3);
    push_file(&mut files, root, Path::new(".claude/CLAUDE.md"));
    collect_dir(&mut files, root, Path::new(".claude/rules"), 1);
    push_file(
        &mut files,
        root,
        Path::new(".github/copilot-instructions.md"),
    );
    collect_dir(&mut files, root, Path::new(".github/instructions"), 1);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files.dedup_by(|a, b| a.0 == b.0);
    files
}

fn push_file(out: &mut Vec<(String, String)>, root: &Path, rel: &Path) {
    let path = root.join(rel);
    let Ok(content) = fs::read_to_string(&path) else {
        return;
    };
    if content.is_empty() {
        return;
    }
    out.push((rel.to_string_lossy().replace('\\', "/"), content));
}

fn collect_dir(out: &mut Vec<(String, String)>, root: &Path, rel: &Path, depth: u8) {
    walk(out, root, &root.join(rel), depth);
}

fn walk(out: &mut Vec<(String, String)>, root: &Path, dir: &Path, depth: u8) {
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
            walk(out, root, &path, depth - 1);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !(name.ends_with(".md") || name.ends_with(".mdc") || name.ends_with(".instructions.md"))
        {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if content.is_empty() {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, content));
    }
}

fn keywords(text: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let mut current = String::new();
    for ch in text.chars() {
        if is_token_char(ch) {
            current.push(ch);
        } else {
            take_keyword(&mut keys, &current);
            current.clear();
        }
    }
    take_keyword(&mut keys, &current);
    keys
}

fn is_token_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.')
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}'
    )
}

fn take_keyword(keys: &mut BTreeSet<String>, raw: &str) {
    let trimmed = raw.trim_matches(|ch: char| matches!(ch, '.' | '-' | '_'));
    if trimmed.is_empty() || trimmed.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
        return;
    }
    let char_count = trimmed.chars().count();
    if trimmed.chars().any(is_cjk) {
        if char_count < 3 {
            return;
        }
        keys.insert(trimmed.to_string());
        return;
    }
    if char_count < 4 {
        return;
    }
    let key = trimmed.to_ascii_lowercase();
    if STOPWORDS.contains(&key.as_str()) {
        return;
    }
    keys.insert(key);
}

fn snippet(text: &str, keyword: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let Some(pos) = lower.find(keyword) else {
        return String::new();
    };
    let start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = text[pos..]
        .find('\n')
        .map(|i| pos + i)
        .unwrap_or(text.len());
    let line = text[start..end].trim();
    if line.chars().count() <= 120 {
        return line.to_string();
    }
    let keyword_len = keyword.len();
    let from = floor_char_boundary(text, pos.saturating_sub(40));
    let to = ceil_char_boundary(text, (pos + keyword_len + 40).min(text.len()));
    let mut piece = text[from..to].trim().to_string();
    if from > 0 {
        piece.insert(0, '…');
    }
    if to < text.len() {
        piece.push('…');
    }
    piece
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}
