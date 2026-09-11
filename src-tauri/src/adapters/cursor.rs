use std::collections::BTreeMap;
use std::path::Path;

use crate::domain::{
    CodeVolumeBranchRow, CodeVolumeCommit, CodeVolumeDailyPoint, CodeVolumeSummary,
};
use crate::ingest::open_readonly;

#[derive(Debug, Clone, Default)]
pub struct CursorCommitRow {
    pub commit_hash: String,
    pub branch: String,
    pub scored_at_ms: i64,
    pub commit_message: String,
    pub lines_added: i64,
    pub lines_deleted: i64,
    pub composer_lines_added: i64,
    pub composer_lines_deleted: i64,
    pub human_lines_added: i64,
    pub human_lines_deleted: i64,
    pub tab_lines_added: i64,
    pub tab_lines_deleted: i64,
    pub ai_percentage: Option<f64>,
}

pub fn parse_cursor_commits(rows: &[CursorCommitRow]) -> Vec<CodeVolumeCommit> {
    rows.iter()
        .map(|row| CodeVolumeCommit {
            commit_hash: row.commit_hash.clone(),
            branch: row.branch.clone(),
            scored_at: chrono::DateTime::from_timestamp_millis(row.scored_at_ms)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
            commit_message: row.commit_message.clone(),
            lines_added: row.lines_added,
            lines_deleted: row.lines_deleted,
            composer_lines_added: row.composer_lines_added,
            composer_lines_deleted: row.composer_lines_deleted,
            human_lines_added: row.human_lines_added,
            human_lines_deleted: row.human_lines_deleted,
            tab_lines_added: row.tab_lines_added,
            tab_lines_deleted: row.tab_lines_deleted,
            ai_percentage: row.ai_percentage,
        })
        .collect()
}

pub fn summarize_code_volume(commits: &[CodeVolumeCommit]) -> CodeVolumeSummary {
    let commit_count = commits.len() as i64;
    let lines_added = commits.iter().map(|c| c.lines_added).sum();
    let lines_deleted = commits.iter().map(|c| c.lines_deleted).sum();
    let composer_lines_added = commits.iter().map(|c| c.composer_lines_added).sum();
    let composer_lines_deleted = commits.iter().map(|c| c.composer_lines_deleted).sum();
    let human_lines_added = commits.iter().map(|c| c.human_lines_added).sum();
    let human_lines_deleted = commits.iter().map(|c| c.human_lines_deleted).sum();
    let tab_lines_added = commits.iter().map(|c| c.tab_lines_added).sum();
    let tab_lines_deleted = commits.iter().map(|c| c.tab_lines_deleted).sum();
    let ai_percentage = if lines_added > 0 {
        Some((composer_lines_added as f64 / lines_added as f64) * 100.0)
    } else {
        let scored: Vec<f64> = commits.iter().filter_map(|c| c.ai_percentage).collect();
        if scored.is_empty() {
            None
        } else {
            Some(scored.iter().sum::<f64>() / scored.len() as f64)
        }
    };

    let mut daily: BTreeMap<String, CodeVolumeDailyPoint> = BTreeMap::new();
    let mut branches: BTreeMap<String, (i64, i64, i64)> = BTreeMap::new();
    for commit in commits {
        let bucket = local_day(&commit.scored_at);
        if !bucket.is_empty() {
            let point = daily.entry(bucket.clone()).or_insert(CodeVolumeDailyPoint {
                bucket,
                lines_added: 0,
                lines_deleted: 0,
                composer_lines_added: 0,
                tab_lines_added: 0,
                human_lines_added: 0,
            });
            point.lines_added += commit.lines_added;
            point.lines_deleted += commit.lines_deleted;
            point.composer_lines_added += commit.composer_lines_added;
            point.tab_lines_added += commit.tab_lines_added;
            point.human_lines_added += commit.human_lines_added;
        }

        let branch = if commit.branch.is_empty() {
            "未知分支".to_string()
        } else {
            commit.branch.clone()
        };
        let entry = branches.entry(branch).or_insert((0, 0, 0));
        entry.0 += 1;
        entry.1 += commit.lines_added;
        entry.2 += commit.composer_lines_added;
    }

    let mut by_branch: Vec<CodeVolumeBranchRow> = branches
        .into_iter()
        .map(
            |(name, (commit_count, lines_added, composer_lines_added))| CodeVolumeBranchRow {
                name,
                commit_count,
                lines_added,
                composer_lines_added,
            },
        )
        .collect();
    by_branch.sort_by(|a, b| {
        b.commit_count
            .cmp(&a.commit_count)
            .then_with(|| b.lines_added.cmp(&a.lines_added))
            .then_with(|| a.name.cmp(&b.name))
    });
    by_branch.truncate(12);

    let mut listed = commits.to_vec();
    listed.sort_by(|a, b| {
        b.scored_at
            .cmp(&a.scored_at)
            .then_with(|| a.commit_hash.cmp(&b.commit_hash))
    });

    CodeVolumeSummary {
        commit_count,
        lines_added,
        lines_deleted,
        net_lines: lines_added - lines_deleted,
        composer_lines_added,
        composer_lines_deleted,
        human_lines_added,
        human_lines_deleted,
        tab_lines_added,
        tab_lines_deleted,
        ai_percentage,
        total_cost: None,
        cost_unpriced: false,
        cost_per_thousand_ai_lines: None,
        daily: daily.into_values().collect(),
        by_branch,
        commits: listed,
    }
}

fn local_day(occurred_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(occurred_at)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|_| occurred_at.get(..10).unwrap_or(occurred_at).to_string())
}

pub fn load_code_volume(home: &Path) -> Result<CodeVolumeSummary, String> {
    let db_path = home.join(".cursor/ai-tracking/ai-code-tracking.db");
    if !db_path.exists() {
        return Ok(CodeVolumeSummary::empty());
    }
    let source_db = open_readonly(&db_path)?;
    let mut stmt = source_db
        .prepare(
            r#"
            SELECT commitHash, branchName, scoredAt,
                   COALESCE(commitMessage, ''),
                   COALESCE(linesAdded, 0),
                   COALESCE(linesDeleted, 0),
                   COALESCE(composerLinesAdded, 0),
                   COALESCE(composerLinesDeleted, 0),
                   COALESCE(humanLinesAdded, 0),
                   COALESCE(humanLinesDeleted, 0),
                   COALESCE(tabLinesAdded, 0),
                   COALESCE(tabLinesDeleted, 0),
                   v2AiPercentage
            FROM scored_commits
            WHERE linesAdded IS NOT NULL OR v2AiPercentage IS NOT NULL
            "#,
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let percentage: Option<String> = row.get(12)?;
            Ok(CursorCommitRow {
                commit_hash: row.get(0)?,
                branch: row.get(1)?,
                scored_at_ms: row.get(2)?,
                commit_message: row.get(3)?,
                lines_added: row.get(4)?,
                lines_deleted: row.get(5)?,
                composer_lines_added: row.get(6)?,
                composer_lines_deleted: row.get(7)?,
                human_lines_added: row.get(8)?,
                human_lines_deleted: row.get(9)?,
                tab_lines_added: row.get(10)?,
                tab_lines_deleted: row.get(11)?,
                ai_percentage: percentage.and_then(|value| value.parse().ok()),
            })
        })
        .map_err(|e| e.to_string())?;
    let commits: Result<Vec<_>, _> = rows.collect();
    let parsed = parse_cursor_commits(&commits.map_err(|e| e.to_string())?);
    Ok(summarize_code_volume(&parsed))
}

/// 把「全部时间、全部来源」的消耗记录费用叠加到代码量摘要上，算出粗略的 ROI 交叉指标。
/// 两个输入统计口径不同（费用覆盖所有 AI CLI，AI 生成行只来自 Cursor 记录），调用方需保证
/// 两者都是不加筛选的全量口径，否则时间窗口对不上、比值没有意义。
pub fn with_cost_roi(
    mut summary: CodeVolumeSummary,
    total_cost: Option<f64>,
    cost_unpriced: bool,
) -> CodeVolumeSummary {
    summary.total_cost = total_cost;
    summary.cost_unpriced = cost_unpriced;
    summary.cost_per_thousand_ai_lines = match total_cost {
        Some(cost) if summary.composer_lines_added > 0 => {
            Some(cost / (summary.composer_lines_added as f64 / 1000.0))
        }
        _ => None,
    };
    summary
}
