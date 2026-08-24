use crate::domain::{
    GlobalInstructionFile, GlobalInstructionSourceRow, InstructionEvidence, InstructionLoadStatus,
    Source,
};

/// CLI 文档写「与编辑器同一套规则系统」，但只点名项目级 `.cursor/rules` 与
/// `AGENTS.md`；账号级 User Rules 写明用于编辑器 Agent (Chat)，未单独写明注入 CLI。
/// 按同源推测为同一套账号级偏好，不得标已验证，也不给出可创建的本地路径。
/// 依据：https://cursor.com/docs/cli/using 与 https://cursor.com/docs/rules （2026-08 查阅）。
pub fn scan() -> GlobalInstructionSourceRow {
    GlobalInstructionSourceRow {
        source: Source::CursorAgent.as_str().into(),
        application: Source::CursorAgent.application_name().into(),
        files: vec![GlobalInstructionFile {
            kind: crate::domain::InstructionEntryKind::File,
            display_path: "用户级全局指令（未证实）".into(),
            abs_path: String::new(),
            byte_size: 0,
            modified_at: None,
            load_status: InstructionLoadStatus::LocallyInvisible,
            evidence: InstructionEvidence::Inferred,
            content: String::new(),
            error: None,
            note: Some(
                "官方只写明项目级 .cursor/rules 与 AGENTS.md；账号级偏好是否注入 CLI 未写明，故标推测。"
                    .into(),
            ),
            action: None,
            editable: false,
        }],
    }
}
