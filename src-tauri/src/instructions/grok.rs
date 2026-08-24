use std::path::Path;

use crate::domain::{
    GlobalInstructionSourceRow, InstructionEvidence, InstructionLoadStatus, Source,
};

use super::file;

pub const HOME_INSTRUCTION_NAMES: &[&str] = &[
    "AGENTS.md",
    "Agents.md",
    "AGENT.md",
    "CLAUDE.md",
    "Claude.md",
    "CLAUDE.local.md",
];

/// 官方 Project Rules：先读 `~/.grok/` 下的全局指令文件，候选名为
/// AGENTS.md / Agents.md / AGENT.md / CLAUDE.md / Claude.md / CLAUDE.local.md；
/// 另外加载 `~/.grok/rules/*.md`。
/// 不把 config.toml、sessions 等非指令文件列进来。
/// 依据：https://docs.x.ai/build/features/project-rules （2026-08 查阅）。
pub fn scan(home: &Path) -> GlobalInstructionSourceRow {
    let dir = home.join(".grok");
    let mut files: Vec<_> = HOME_INSTRUCTION_NAMES
        .iter()
        .filter(|name| dir.join(name).is_file())
        .map(|name| {
            file::read_file(
                &dir.join(name),
                &format!("~/.grok/{name}"),
                InstructionLoadStatus::Loaded,
                InstructionEvidence::Verified,
                None,
            )
        })
        .collect();

    if files.is_empty() {
        files.push(file::read_file(
            &dir.join("AGENTS.md"),
            "~/.grok/AGENTS.md",
            InstructionLoadStatus::Loaded,
            InstructionEvidence::Verified,
            None,
        ));
    }

    let rules_dir = dir.join("rules");
    let rule_files: Vec<_> = file::list_files(&rules_dir)
        .into_iter()
        .filter(|(name, _)| name.ends_with(".md"))
        .collect();
    if !rule_files.is_empty() {
        if let Some(entry) = file::read_directory(
            &rules_dir,
            "~/.grok/rules/",
            InstructionLoadStatus::Loaded,
            InstructionEvidence::Verified,
            None,
        ) {
            files.push(entry);
        }
        for (name, path) in rule_files {
            files.push(file::read_file(
                &path,
                &format!("~/.grok/rules/{name}"),
                InstructionLoadStatus::Loaded,
                InstructionEvidence::Verified,
                None,
            ));
        }
    }

    GlobalInstructionSourceRow {
        source: Source::Grok.as_str().into(),
        application: Source::Grok.application_name().into(),
        files,
    }
}
