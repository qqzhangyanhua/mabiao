use super::usage::Source;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceDiagnostic {
    pub source: String,
    pub application: String,
    pub detected: bool,
    pub root_path: String,
    pub cached_files: u64,
    pub record_count: u64,
    pub total_tokens: i64,
    pub coverage: String,
    /// 源文件已被工具自身清理，但仍计入统计的历史记录数（见 ADR 0004）。
    pub archived_record_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestIssue {
    pub source: String,
    pub path: String,
    pub message: String,
    #[serde(default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub line: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceIngestReport {
    pub source: String,
    pub detected: bool,
    pub files_seen: u64,
    pub files_parsed: u64,
    pub files_skipped: u64,
    pub files_failed: u64,
    pub records_written: u64,
    pub records_removed: u64,
    /// 本轮因源文件消失而归档（非物理删除，见 ADR 0004）的记录数。
    pub records_archived: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestReport {
    pub files_seen: u64,
    pub files_parsed: u64,
    pub files_skipped: u64,
    pub files_failed: u64,
    pub records_written: u64,
    pub records_removed: u64,
    pub records_archived: u64,
    pub partial_success: bool,
    pub issues: Vec<IngestIssue>,
    #[serde(default)]
    pub conversation_issues: Vec<IngestIssue>,
    pub sources: Vec<SourceIngestReport>,
    /// 本轮摄取动过的 UTC 日期（`YYYY-MM-DD`）。只用来把预聚合表的重建收窄到这些天，
    /// 不返回给前端。空集合配合 `rollup_full_rebuild = false` 表示无事可做。
    #[serde(skip)]
    pub touched_days: std::collections::BTreeSet<String>,
    /// 罕见的整源清理（删掉未知来源的记录）无法按天定位，只能整表重来。
    #[serde(skip)]
    pub rollup_full_rebuild: bool,
}

impl Default for IngestReport {
    fn default() -> Self {
        Self {
            files_seen: 0,
            files_parsed: 0,
            files_skipped: 0,
            files_failed: 0,
            records_written: 0,
            records_removed: 0,
            records_archived: 0,
            partial_success: false,
            issues: Vec::new(),
            conversation_issues: Vec::new(),
            sources: Source::ALL
                .iter()
                .map(|source| SourceIngestReport {
                    source: source.as_str().to_string(),
                    detected: false,
                    files_seen: 0,
                    files_parsed: 0,
                    files_skipped: 0,
                    files_failed: 0,
                    records_written: 0,
                    records_removed: 0,
                    records_archived: 0,
                })
                .collect(),
            touched_days: std::collections::BTreeSet::new(),
            rollup_full_rebuild: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterOptions {
    pub sources: Vec<String>,
    pub models: Vec<String>,
    pub projects: Vec<String>,
    pub providers: Vec<String>,
}
