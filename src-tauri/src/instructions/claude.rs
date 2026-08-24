use std::path::Path;

use crate::domain::{GlobalInstructionSourceRow, InstructionEvidence, InstructionLoadStatus};

use super::file;

pub fn scan(home: &Path) -> GlobalInstructionSourceRow {
    let claude_dir = home.join(".claude");
    let mut files = vec![file::read_file(
        &claude_dir.join("CLAUDE.md"),
        "~/.claude/CLAUDE.md",
        InstructionLoadStatus::Loaded,
        InstructionEvidence::Verified,
        None,
    )];

    let rules_dir = claude_dir.join("rules");
    let rule_files = file::list_files(&rules_dir);
    if !rule_files.is_empty() {
        if let Some(dir) = file::read_directory(
            &rules_dir,
            "~/.claude/rules/",
            InstructionLoadStatus::Loaded,
            InstructionEvidence::Verified,
            None,
        ) {
            files.push(dir);
        }
        for (name, path) in rule_files {
            files.push(file::read_file(
                &path,
                &format!("~/.claude/rules/{name}"),
                InstructionLoadStatus::Loaded,
                InstructionEvidence::Verified,
                None,
            ));
        }
    }

    GlobalInstructionSourceRow {
        source: "claude".into(),
        application: "Claude".into(),
        files,
    }
}
