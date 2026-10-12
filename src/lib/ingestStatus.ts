import type { ConversationCatalogProgressDto, IngestReport } from "../types";

export function usageStageStatus(label: string, report: IngestReport): string {
  const issue = report.files_failed > 0 ? `，失败 ${report.files_failed}` : "";
  const removed = report.records_removed > 0 ? `，清理 ${report.records_removed}` : "";
  const archived = report.records_archived > 0 ? `，归档 ${report.records_archived}` : "";
  return `${label}${report.partial_success ? "部分完成" : "完成"}：解析 ${report.files_parsed}，跳过 ${report.files_skipped}，写入 ${report.records_written}${archived}${removed}${issue}`;
}

export function ingestFailureStatus(label: string, reason: string): string {
  return `${label}失败：${reason}`;
}

export function isCatalogSyncing(progress: ConversationCatalogProgressDto | null): boolean {
  return progress !== null && !progress.finished;
}

export function catalogProgressStatus(progress: ConversationCatalogProgressDto): string {
  return `对话目录同步中 ${progress.done}/${progress.total}`;
}

export function catalogFinishedStatus(baseStatus: string, issueCount: number): string {
  if (issueCount <= 0) {
    return baseStatus;
  }
  const suffix = `对话目录保留 ${issueCount} 项问题`;
  return baseStatus ? `${baseStatus}，${suffix}` : suffix;
}

export function catalogStatus(
  progress: ConversationCatalogProgressDto,
  baseStatus: string,
): string {
  return progress.finished
    ? catalogFinishedStatus(baseStatus, progress.issue_count)
    : catalogProgressStatus(progress);
}
