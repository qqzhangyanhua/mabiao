use std::path::{Path, PathBuf};

use crate::domain::{
    InstructionCheckupFinding, InstructionCheckupKind, InstructionCheckupSeverity,
};
use crate::source_sqlite;

const PENDING_KEY: &str = "cursor/pendingMemories";

struct Residue {
    count: usize,
    first_date: Option<String>,
    last_date: Option<String>,
}

pub fn detect(home: &Path) -> Option<InstructionCheckupFinding> {
    let path = candidate_paths(home)
        .into_iter()
        .find(|item| item.is_file())?;
    let residue = read_residue(&path)?;
    Some(finding(residue))
}

fn candidate_paths(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb"),
        home.join(".config/Cursor/User/globalStorage/state.vscdb"),
        home.join("AppData/Roaming/Cursor/User/globalStorage/state.vscdb"),
    ]
}

fn read_residue(path: &Path) -> Option<Residue> {
    let conn = source_sqlite::open_wal_readonly(path).ok()?;
    let raw = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            [PENDING_KEY],
            |row| source_sqlite::row_text_lossy(row, 0),
        )
        .ok()?;
    parse_residue(&raw)
}

fn parse_residue(raw: &str) -> Option<Residue> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let items: Vec<&serde_json::Value> = value
        .as_array()?
        .iter()
        .filter(|item| item.is_object())
        .collect();
    if items.is_empty() {
        return None;
    }
    let mut dates: Vec<String> = items
        .iter()
        .filter_map(|item| item.get("timestamp").and_then(|stamp| stamp.as_i64()))
        .filter_map(timestamp_date)
        .collect();
    dates.sort();
    dates.dedup();
    Some(Residue {
        count: items.len(),
        first_date: dates.first().cloned(),
        last_date: dates.last().cloned(),
    })
}

fn timestamp_date(stamp: i64) -> Option<String> {
    let millis = if stamp > 10_000_000_000 {
        stamp
    } else {
        stamp.saturating_mul(1_000)
    };
    chrono::DateTime::from_timestamp_millis(millis).map(|dt| dt.date_naive().to_string())
}

fn finding(residue: Residue) -> InstructionCheckupFinding {
    InstructionCheckupFinding {
        kind: InstructionCheckupKind::OrphanMemories,
        severity: InstructionCheckupSeverity::Medium,
        source: "cursor".into(),
        application: "Cursor".into(),
        display_path: PENDING_KEY.into(),
        problem: problem(&residue),
        consequence: "Memories 功能已从 Cursor 移除，管理入口不存在，这批数据既看不到也删不掉。"
            .into(),
    }
}

fn problem(residue: &Residue) -> String {
    match (&residue.first_date, &residue.last_date) {
        (Some(from), Some(to)) if from != to => format!(
            "检测到 Cursor 残留 {} 条 memories，时间范围 {from} 至 {to}。",
            residue.count
        ),
        (Some(day), Some(_)) => {
            format!(
                "检测到 Cursor 残留 {} 条 memories，时间范围 {day}。",
                residue.count
            )
        }
        _ => format!("检测到 Cursor 残留 {} 条 memories。", residue.count),
    }
}
