use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};

use crate::domain::{
    GlobalInstructionFile, InstructionEntryKind, InstructionEvidence, InstructionLoadStatus,
};

pub fn read_file(
    path: &Path,
    display_path: &str,
    load_status: InstructionLoadStatus,
    evidence: InstructionEvidence,
    note: Option<String>,
) -> GlobalInstructionFile {
    match fs::metadata(path) {
        Ok(meta) => {
            let content = fs::read_to_string(path).ok();
            let error = if content.is_none() {
                Some(format!("读取 {display_path} 失败"))
            } else {
                None
            };
            GlobalInstructionFile {
                kind: InstructionEntryKind::File,
                display_path: display_path.to_string(),
                abs_path: path.to_string_lossy().into_owned(),
                byte_size: meta.len(),
                modified_at: mtime_rfc3339(&meta),
                load_status,
                evidence,
                content: content.unwrap_or_default(),
                error,
                note,
                action: None,
                editable: false,
            }
        }
        Err(_) => missing(
            path,
            display_path,
            InstructionLoadStatus::NotCreated,
            evidence,
            note,
        ),
    }
}

fn missing(
    path: &Path,
    display_path: &str,
    load_status: InstructionLoadStatus,
    evidence: InstructionEvidence,
    note: Option<String>,
) -> GlobalInstructionFile {
    GlobalInstructionFile {
        kind: InstructionEntryKind::File,
        display_path: display_path.to_string(),
        abs_path: path.to_string_lossy().into_owned(),
        byte_size: 0,
        modified_at: None,
        load_status,
        evidence,
        content: String::new(),
        error: None,
        note,
        action: None,
        editable: false,
    }
}

pub fn no_mechanism(note: &str) -> GlobalInstructionFile {
    GlobalInstructionFile {
        kind: InstructionEntryKind::File,
        display_path: "无用户级全局指令机制".into(),
        abs_path: String::new(),
        byte_size: 0,
        modified_at: None,
        load_status: InstructionLoadStatus::NotCreated,
        evidence: InstructionEvidence::NoMechanism,
        content: String::new(),
        error: None,
        note: Some(note.to_string()),
        action: None,
        editable: false,
    }
}

pub fn read_directory(
    path: &Path,
    display_path: &str,
    load_status: InstructionLoadStatus,
    evidence: InstructionEvidence,
    note: Option<String>,
) -> Option<GlobalInstructionFile> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_dir() {
        return None;
    }
    Some(GlobalInstructionFile {
        kind: InstructionEntryKind::Directory,
        display_path: display_path.to_string(),
        abs_path: path.to_string_lossy().into_owned(),
        byte_size: 0,
        modified_at: mtime_rfc3339(&meta),
        load_status,
        evidence,
        content: String::new(),
        error: None,
        note,
        action: None,
        editable: false,
    })
}

pub fn list_files(dir: &Path) -> Vec<(String, std::path::PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut extras = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        extras.push((name, path));
    }
    extras.sort_by(|a, b| a.0.cmp(&b.0));
    extras
}

fn mtime_rfc3339(meta: &fs::Metadata) -> Option<String> {
    let modified = meta.modified().ok()?;
    Some(DateTime::<Utc>::from(modified).to_rfc3339())
}
