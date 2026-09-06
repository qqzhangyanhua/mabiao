use crate::domain::WorkNotesGate;

pub const CONFIRM_AFTER: i64 = 60;
pub const REJECT_AFTER: i64 = 150;

pub fn assess(session_count: i64) -> (WorkNotesGate, String) {
    if session_count > REJECT_AFTER {
        (
            WorkNotesGate::Rejected,
            format!("会话数 {session_count} 超过 {REJECT_AFTER}，请收窄区间后再试"),
        )
    } else if session_count > CONFIRM_AFTER {
        (
            WorkNotesGate::Confirm,
            format!(
                "会话数 {session_count} 超过 {CONFIRM_AFTER}，生成会花较长时间并消耗额度，确认后才会开始"
            ),
        )
    } else {
        (WorkNotesGate::Ok, String::new())
    }
}

pub fn enforce(session_count: i64, confirmed: bool) -> Result<(), String> {
    match assess(session_count) {
        (WorkNotesGate::Rejected, message) => Err(message),
        (WorkNotesGate::Confirm, message) if !confirmed => Err(message),
        _ => Ok(()),
    }
}
