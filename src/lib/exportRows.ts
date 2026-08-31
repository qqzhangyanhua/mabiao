import { projectLabel } from "./format";
import { formatBucket } from "./chartTheme";
import { cacheTokens, trendTableRows } from "./trendStats";
import type {
  ApplicationAnalyticsDto,
  CodeVolumeCommit,
  CodeVolumeSummary,
  CursorAccountEventRow,
  CursorAccountUsageDto,
  CursorSessionDetailDto,
  CursorSessionSummaryDto,
  SeriesPoint,
} from "../types";

export type ExportTable = {
  headers: string[];
  rows: (string | number)[][];
};

function costCell(cost: number | null): string | number {
  return cost ?? "";
}

function ratioCell(value: number | null): string | number {
  return value ?? "";
}

export function trendSeriesTable(points: SeriesPoint[]): ExportTable {
  return {
    headers: ["时间", "总量", "输入", "输出", "缓存", "推理", "费用", "占总量%", "环比%"],
    rows: trendTableRows(points).map((row) => [
      formatBucket(row.point.bucket),
      row.point.total_tokens,
      row.point.input_tokens,
      row.point.output_tokens,
      cacheTokens(row.point),
      row.point.reasoning_tokens,
      costCell(row.point.cost),
      Number(row.shareOfTotal.toFixed(2)),
      row.periodDelta == null ? "" : Number(row.periodDelta.toFixed(2)),
    ]),
  };
}

export function applicationEfficiencyTable(data: ApplicationAnalyticsDto): ExportTable {
  return {
    headers: ["来源", "总 Token", "会话数", "平均会话 Token", "缓存命中率", "推理占比"],
    rows: data.by_application.map((row) => [
      row.application,
      row.metrics.total_tokens,
      row.metrics.session_count,
      ratioCell(row.metrics.average_session_tokens),
      ratioCell(row.metrics.cache_hit_rate),
      ratioCell(row.metrics.reasoning_share),
    ]),
  };
}

export function applicationProjectMatrixTable(data: ApplicationAnalyticsDto): ExportTable {
  const headers = [
    "项目",
    ...data.by_application.map((application) => application.application),
    "总计",
  ];
  return {
    headers,
    rows: data.projects.map((row) => [
      projectLabel(row.project),
      ...data.by_application.map((application) => row.values[application.source] ?? 0),
      row.total_tokens,
    ]),
  };
}

export function codeVolumeTable(data: CodeVolumeSummary): ExportTable {
  const unpriced = data.cost_unpriced ? "是" : "";
  return {
    headers: ["指标", "数值", "未定价"],
    rows: [
      ["提交数", data.commit_count, ""],
      ["新增行", data.lines_added, ""],
      ["删除行", data.lines_deleted, ""],
      ["净增行", data.net_lines, ""],
      ["AI 生成行", data.composer_lines_added, ""],
      ["Tab 行", data.tab_lines_added, ""],
      ["人工编写行", data.human_lines_added, ""],
      ["AI 占比", ratioCell(data.ai_percentage), ""],
      ["全部来源累计费用", costCell(data.total_cost), unpriced],
      ["每千行 AI 代码成本", costCell(data.cost_per_thousand_ai_lines), unpriced],
    ],
  };
}

export function cursorAccountDailyTable(data: CursorAccountUsageDto): ExportTable {
  return {
    headers: ["日期", "总量", "输入", "输出"],
    rows: data.daily.map((point) => [
      point.bucket,
      point.total_tokens,
      point.input_tokens,
      point.output_tokens,
    ]),
  };
}

export function cursorAccountModelTable(data: CursorAccountUsageDto): ExportTable {
  return {
    headers: ["模型", "Token", "占比"],
    rows: data.by_model.map((row) => [row.name, row.total_tokens, row.share]),
  };
}

export function cursorSessionProjectTable(data: CursorSessionSummaryDto): ExportTable {
  return {
    headers: ["项目", "会话数", "轮次数"],
    rows: data.by_project.map((row) => [projectLabel(row.name), row.session_count, row.turn_count]),
  };
}

export function cursorSessionToolTable(data: CursorSessionSummaryDto): ExportTable {
  return {
    headers: ["工具", "调用次数"],
    rows: data.top_tools.map((row) => [row.name, row.call_count]),
  };
}

export function cursorSessionToolGroupTable(data: CursorSessionSummaryDto): ExportTable {
  return {
    headers: ["分类", "调用次数"],
    rows: data.tool_groups.map((row) => [row.name, row.call_count]),
  };
}

export function cursorSessionDetailToolTable(data: CursorSessionDetailDto): ExportTable {
  return {
    headers: ["工具", "调用次数"],
    rows: data.tools.map((row) => [row.name, row.call_count]),
  };
}

export function cursorSessionPathTable(data: CursorSessionDetailDto): ExportTable {
  return {
    headers: ["类型", "路径"],
    rows: [
      ...data.read_paths.map((path) => ["读", path]),
      ...data.write_paths.map((path) => ["写", path]),
    ],
  };
}

export function cursorSessionHashFileTable(data: CursorSessionDetailDto): ExportTable {
  return {
    headers: ["路径", "扩展名", "来源"],
    rows: data.hash_files.map((row) => [row.path, row.extension, row.source]),
  };
}

export function cursorAccountEventTable(rows: CursorAccountEventRow[]): ExportTable {
  return {
    headers: ["时间", "模型", "输入", "输出", "缓存读", "缓存写", "总量", "无头"],
    rows: rows.map((row) => [
      row.occurred_at,
      row.model,
      row.input_tokens,
      row.output_tokens,
      row.cache_read_tokens,
      row.cache_creation_tokens,
      row.total_tokens,
      row.is_headless ? "是" : "",
    ]),
  };
}

export function codeVolumeCommitTable(commits: CodeVolumeCommit[]): ExportTable {
  return {
    headers: ["提交", "分支", "说明", "新增", "删除", "AI 行", "Tab 行", "时间"],
    rows: commits.map((row) => [
      row.commit_hash,
      row.branch,
      row.commit_message,
      row.lines_added,
      row.lines_deleted,
      row.composer_lines_added,
      row.tab_lines_added,
      row.scored_at,
    ]),
  };
}
