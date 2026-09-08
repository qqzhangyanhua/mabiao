use std::path::{Path, PathBuf};

use crate::domain::{
    GlobalInstructionSourceRow, InstructionEvidence, InstructionLoadStatus, Source,
};

use super::file;

pub const HOME_DIR: &str = ".grok";
pub const RULES_DIR: &str = "rules";

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
/// 不扫项目根 `AGENTS.md` / `.cursor/rules`：产品文档只写用户级 `~/.grok`。
/// 依据：https://docs.x.ai/build/features/project-rules （2026-08 查阅）。
pub fn existing_home_instruction_files(home: &Path) -> Vec<(String, PathBuf)> {
    let dir = home.join(HOME_DIR);
    HOME_INSTRUCTION_NAMES
        .iter()
        .filter(|name| dir.join(name).is_file())
        .map(|name| (format!("~/.grok/{name}"), dir.join(name)))
        .collect()
}

pub fn existing_rule_files(home: &Path) -> Vec<(String, PathBuf)> {
    file::list_files(&home.join(HOME_DIR).join(RULES_DIR))
        .into_iter()
        .filter(|(name, _)| name.ends_with(".md"))
        .map(|(name, path)| (format!("~/.grok/rules/{name}"), path))
        .collect()
}

pub fn scan(home: &Path) -> GlobalInstructionSourceRow {
    let dir = home.join(HOME_DIR);
    let mut files: Vec<_> = existing_home_instruction_files(home)
        .into_iter()
        .map(|(display, path)| {
            file::read_file(
                &path,
                &display,
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

    let rule_files = existing_rule_files(home);
    if !rule_files.is_empty() {
        if let Some(entry) = file::read_directory(
            &dir.join(RULES_DIR),
            "~/.grok/rules/",
            InstructionLoadStatus::Loaded,
            InstructionEvidence::Verified,
            None,
        ) {
            files.push(entry);
        }
        for (display, path) in rule_files {
            files.push(file::read_file(
                &path,
                &display,
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
