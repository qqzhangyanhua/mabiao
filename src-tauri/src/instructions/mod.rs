mod checkup;
pub mod claude;
mod claude_memory;
pub mod codex;
mod conflict;
pub mod copilot;
pub mod cursor;
pub mod cursor_agent;
pub(crate) mod cursor_disk;
mod cursor_memories;
pub mod dsh;
pub mod factory;
mod file;
pub mod gemini;
pub mod grok;
pub(crate) mod grok_disk;
mod insight;
pub mod kimi;
pub mod opencode;
pub mod pi;
mod project_walk;
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

/// 只打开写入白名单或各 Source 已知的全局指令位置。webview 传来的任意路径一律拒绝。
pub fn is_allowed_to_open(home: &Path, path: &Path) -> bool {
    crate::user_files::is_allowed(home, path) || is_known_instruction_location(home, path)
}

fn is_known_instruction_location(home: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(home) else {
        return false;
    };
    let parts: Vec<_> = rel.iter().filter_map(|p| p.to_str()).collect();
    match parts.as_slice() {
        [".claude", "CLAUDE.md"] => true,
        [".claude", "rules"] => true,
        [".claude", "rules", name] if crate::user_files::is_plain_name(name) => true,
        [".codex", "AGENTS.md"] => true,
        [".codex", "AGENTS.override.md"] => true,
        [".codex", "rules", name] if crate::user_files::is_plain_name(name) => true,
        [".gemini", "GEMINI.md"] => true,
        [".grok", name] if grok::HOME_INSTRUCTION_NAMES.contains(name) => true,
        [".grok", "rules"] => true,
        [".grok", "rules", name]
            if crate::user_files::is_plain_name(name) && name.ends_with(".md") =>
        {
            true
        }
        [".pi", "agent", "AGENTS.md"] => true,
        [".pi", "agent", "AGENTS.override.md"] => true,
        [".config", "opencode", "AGENTS.md"] => true,
        [".dsh", "AGENTS.md"] => true,
        [".qwen", "QWEN.md"] => true,
        [".factory", "AGENTS.md"] => true,
        [".copilot", "copilot-instructions.md"] => true,
        [".copilot", "instructions", name]
            if crate::user_files::is_plain_name(name) && name.ends_with(".instructions.md") =>
        {
            true
        }
        _ => false,
    }
}

/// Windows `cmd /C start` 会再解析一遍参数；路径里的引号或换行能拆出第二条命令。
pub fn windows_start_path_is_safe(path: &Path) -> bool {
    let display = path.display().to_string();
    !display.contains('"')
        && !display.contains('\n')
        && !display.contains('\r')
        && !display.contains('\0')
}

pub fn open_in_external_editor(home: &Path, abs_path: &str) -> Result<(), String> {
    let requested = Path::new(abs_path);
    if !is_allowed_to_open(home, requested) {
        return Err("该路径不在可打开的全局指令名单中".into());
    }
    let target = resolve_open_path(abs_path)?;
    if !windows_start_path_is_safe(&target) {
        return Err("路径含有不安全字符".into());
    }
    let status = open_command(&target)?
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

fn open_command(target: &Path) -> Result<Command, String> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(target);
        Ok(cmd)
    }
    #[cfg(target_os = "linux")]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(target);
        Ok(cmd)
    }
    #[cfg(target_os = "windows")]
    {
        windows_start_command(target)
    }
}

#[cfg(target_os = "windows")]
fn windows_start_command(target: &Path) -> Result<Command, String> {
    use std::os::windows::process::CommandExt;
    if !windows_start_path_is_safe(target) {
        return Err("路径含有不安全字符".into());
    }
    let display = target.display().to_string();
    let mut cmd = Command::new("cmd");
    cmd.arg("/C");
    // raw_arg 让空标题和带引号的路径原样进 cmd，文件名里的 `&`/`|` 不会变成第二条命令。
    cmd.raw_arg("start \"\"");
    cmd.raw_arg(format!("\"{display}\""));
    Ok(cmd)
}
