import type { OfficialQuotaFreshness } from "../types";
import { formatClock, relativeTime } from "./format";

export const OFFICIAL_QUOTA_FRESHNESS_STATUS: Record<OfficialQuotaFreshness, string> = {
  official: "官方",
  stale: "已过期",
  unavailable: "暂无",
};

export function officialQuotaAgeLabel(
  capturedAt: string | null,
  now = Date.now(),
): string | null {
  if (!capturedAt) {
    return null;
  }
  if (Number.isNaN(Date.parse(capturedAt))) {
    return null;
  }
  return relativeTime(capturedAt, now);
}

export function officialQuotaFreshnessTitle(
  freshness: OfficialQuotaFreshness,
  capturedAt: string | null,
  staleAfterMinutes: number,
): string {
  const clock = capturedAt ? formatClock(capturedAt) : null;
  if (freshness === "unavailable") {
    return "尚未取到官方额度";
  }
  if (freshness === "official") {
    return clock
      ? `官方快照，${clock} 更新。${staleAfterMinutes} 分钟内视为新鲜`
      : `官方快照，${staleAfterMinutes} 分钟内视为新鲜`;
  }
  return clock
    ? `本地缓存超过 ${staleAfterMinutes} 分钟未更新（${clock}）。已过期指缓存超时，不是额度用完`
    : `本地缓存超过 ${staleAfterMinutes} 分钟未更新。已过期指缓存超时，不是额度用完`;
}

export function officialQuotaRefreshHint(staleAfterMinutes: number): string {
  return `每 ${staleAfterMinutes} 分钟自动刷新 · 超过 ${staleAfterMinutes} 分钟未更新标为过期（指缓存）`;
}

export function officialQuotaSettingsRefreshNote(staleAfterMinutes: number): string {
  return `打开总览或点「刷新额度」时取数；总览打开期间每 ${staleAfterMinutes} 分钟自动再刷。超过 ${staleAfterMinutes} 分钟未更新标为过期，仍显示上次数字。`;
}
