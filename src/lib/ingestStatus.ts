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

const CATALOG_PROGRESS_PREFIX = "对话目录同步中";
const CATALOG_ISSUES_PREFIX = "对话目录保留";

export function catalogProgressStatus(progress: ConversationCatalogProgressDto): string {
  return `${CATALOG_PROGRESS_PREFIX} ${progress.done}/${progress.total}`;
}

export function catalogFinishedStatus(baseStatus: string, issueCount: number): string {
  if (issueCount <= 0) {
    return baseStatus;
  }
  const suffix = `${CATALOG_ISSUES_PREFIX} ${issueCount} 项问题`;
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

/** 用量阶段文案加上目前已知的目录阶段状态；还没收到过目录事件时就是用量文案本身。 */
export function composeStatus(
  usageStatus: string,
  progress: ConversationCatalogProgressDto | null,
): string {
  return progress ? catalogStatus(progress, usageStatus) : usageStatus;
}

/**
 * 当前状态栏文案是不是目录阶段自己写的（或还是用量阶段的结果）。
 * 不是的话说明别处写了更新的状态（重建、保存、报错……），目录事件不应覆盖它。
 */
export function isCatalogManagedStatus(current: string, usageStatus: string): boolean {
  if (current.startsWith(CATALOG_PROGRESS_PREFIX) || current.startsWith(CATALOG_ISSUES_PREFIX)) {
    return true;
  }
  if (usageStatus === "") {
    return false;
  }
  return current === usageStatus || current.startsWith(`${usageStatus}，${CATALOG_ISSUES_PREFIX}`);
}
