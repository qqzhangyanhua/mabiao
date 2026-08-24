use crate::domain::{
    GlobalInstructionFile, GlobalInstructionSourceRow, InstructionEvidence, InstructionLoadStatus,
};

pub fn scan() -> GlobalInstructionSourceRow {
    GlobalInstructionSourceRow {
        source: "cursor".into(),
        application: "Cursor".into(),
        files: vec![GlobalInstructionFile {
            kind: crate::domain::InstructionEntryKind::File,
            display_path: "Cursor 账号级偏好".into(),
            abs_path: String::new(),
            byte_size: 0,
            modified_at: None,
            load_status: InstructionLoadStatus::LocallyInvisible,
            evidence: InstructionEvidence::Verified,
            content: String::new(),
            error: None,
            note: Some("存在于 Cursor 账号服务端，本机磁盘看不到内容。".into()),
            action: Some("cursor_settings".into()),
            editable: false,
        }],
    }
}

pub fn open_settings() -> Result<(), String> {
    let status = std::process::Command::new("open")
        .arg("cursor://settings")
        .status()
        .map_err(|e| format!("无法打开 Cursor 设置：{e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("无法打开 Cursor 设置".into())
    }
}
