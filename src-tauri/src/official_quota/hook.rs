use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::domain::OfficialQuotaHookDto;
use crate::official_quota::QuotaEnv;
use crate::scan_paths::ScanEnv;

pub fn default_settings_path() -> PathBuf {
    default_settings_path_in(&ScanEnv::capture_for_home(&crate::ingest::default_home()))
}

pub(crate) fn default_settings_path_in(env: &ScanEnv) -> PathBuf {
    env.claude_hook_settings_path()
}

pub fn preview_in(env: &QuotaEnv, command: &str) -> OfficialQuotaHookDto {
    preview(&env.claude_hook_settings_path(), command)
}

pub fn apply_in(env: &QuotaEnv, command: &str) -> Result<OfficialQuotaHookDto, String> {
    apply(&env.claude_hook_settings_path(), command)
}

pub fn hook_command() -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("mabiao"));
    format!("\"{}\" statusline", exe.display())
}

pub fn preview(settings_path: &Path, command: &str) -> OfficialQuotaHookDto {
    inspect(settings_path, command)
}

pub fn apply(settings_path: &Path, command: &str) -> Result<OfficialQuotaHookDto, String> {
    let preview = inspect(settings_path, command);
    if preview.conflict {
        return Ok(preview);
    }
    if preview.already_configured {
        return Ok(preview);
    }
    let mut root = if settings_path.exists() {
        let text =
            fs::read_to_string(settings_path).map_err(|e| format!("读取 Claude 设置失败：{e}"))?;
        if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&text).map_err(|e| format!("Claude 设置不是合法 JSON：{e}"))?
        }
    } else {
        json!({})
    };
    let Some(object) = root.as_object_mut() else {
        return Err("Claude 设置根节点必须是对象".to_string());
    };
    object.insert(
        "statusLine".to_string(),
        json!({
            "type": "command",
            "command": command
        }),
    );
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        settings_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?
        ),
    )
    .map_err(|e| format!("写入 Claude 设置失败：{e}"))?;
    Ok(inspect(settings_path, command))
}

fn inspect(settings_path: &Path, command: &str) -> OfficialQuotaHookDto {
    let snippet = serde_json::to_string_pretty(&json!({
        "statusLine": {
            "type": "command",
            "command": command
        }
    }))
    .unwrap_or_else(|_| command.to_string());
    let existing = read_status_line(settings_path);
    let already_configured = existing.as_deref() == Some(command);
    let conflict = matches!(&existing, Some(value) if value != command);
    OfficialQuotaHookDto {
        settings_path: settings_path.display().to_string(),
        command: command.to_string(),
        snippet,
        already_configured,
        conflict,
        conflict_command: if conflict { existing } else { None },
    }
}

fn read_status_line(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value
        .pointer("/statusLine/command")
        .and_then(Value::as_str)
        .map(str::to_string)
}
