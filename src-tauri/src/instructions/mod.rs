mod checkup;
pub mod claude;
mod claude_memory;
pub mod codex;
mod conflict;
pub mod copilot;
pub mod cursor;
pub mod cursor_agent;
mod cursor_memories;
pub mod dsh;
pub mod factory;
mod file;
pub mod gemini;
pub mod grok;
mod insight;
pub mod kimi;
pub mod opencode;
pub mod pi;
pub mod qwen;

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::domain::{GlobalInstructionDto, InstructionUsageSummary};

pub fn scan(
    home: &Path,
    project_root: Option<&Path>,
    usage: &InstructionUsageSummary,
) -> GlobalInstructionDto {
    let mut sources = vec![
        claude::scan(home),
        codex::scan(home),
        gemini::scan(home),
        cursor::scan(),
        pi::scan(home),
        opencode::scan(home),
        kimi::scan(),
        dsh::scan(home),
        grok::scan(home),
        qwen::scan(home),
        factory::scan(home),
        cursor_agent::scan(),
        copilot::scan(home),
    ];
    mark_editable(home, &mut sources);
    let mut findings = checkup::collect(&sources);
    if let Some(finding) = cursor_memories::detect(home) {
        findings.push(finding);
    }
    let claude_memories = claude_memory::collect(home);
    if let Some(finding) = claude_memory::finding(&claude_memories) {
        findings.push(finding);
    }
    checkup::sort(&mut findings);
    let (selected_project, hints) = conflict::collect(&sources, project_root);
    let (investments, imbalances) = insight::collect(&sources, usage);
    GlobalInstructionDto {
        sources,
        findings,
        selected_project,
        projects: Vec::new(),
        hints,
        investments,
        imbalances,
        claude_memories,
    }
}

pub fn scan_for_projects(
    home: &Path,
    requested: Option<&str>,
    recent: &[String],
    usage: &InstructionUsageSummary,
) -> GlobalInstructionDto {
    let comparable: Vec<String> = recent
        .iter()
        .filter(|path| Path::new(path.as_str()).is_dir())
        .cloned()
        .collect();
    let selected = match requested {
        Some(path) if comparable.iter().any(|item| item == path) => Some(path.to_string()),
        _ => comparable.first().cloned(),
    };
    let mut dto = scan(home, selected.as_deref().map(Path::new), usage);
    dto.projects = comparable;
    dto
}

/// 解析「在外部打开」的目标：已存在的文件或目录原样打开；文件尚未创建则打开父目录。
pub fn resolve_open_path(abs_path: &str) -> Result<PathBuf, String> {
    if abs_path.trim().is_empty() {
        return Err("没有可打开的路径".into());
    }
    let path = PathBuf::from(abs_path);
    if path.exists() {
        return Ok(path);
    }
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(parent.to_path_buf()),
        _ => Err("没有可打开的路径".into()),
    }
}

pub fn open_in_external_editor(abs_path: &str) -> Result<(), String> {
    let target = resolve_open_path(abs_path)?;
    let status = open_command(&target)
        .status()
        .map_err(|e| format!("无法在外部打开：{e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("无法在外部打开该全局指令".into())
    }
}

fn mark_editable(home: &Path, sources: &mut [crate::domain::GlobalInstructionSourceRow]) {
    for row in sources {
        for file in &mut row.files {
            file.editable = file.kind == crate::domain::InstructionEntryKind::File
                && !file.abs_path.is_empty()
                && crate::user_files::is_allowed(home, Path::new(&file.abs_path));
        }
    }
}

fn open_command(target: &Path) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(target);
        cmd
    }
    #[cfg(target_os = "linux")]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(target);
        cmd
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", &target.display().to_string()]);
        cmd
    }
}
