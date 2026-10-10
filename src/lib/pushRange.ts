import type {
  AutoPushDto,
  Filter,
  PushHistoryEntry,
  PushOutcome,
  PushRange,
  PushSessionIssue,
  PushSessionKey,
} from "../types";

export type PushPreset = "filter" | "yesterday" | "7d" | "30d";

const DAY_MS = 24 * 3600 * 1000;

/** 后端按字符串比较 RFC 3339，所以统一成不带毫秒的 UTC 写法，和库里存的格式一致。 */
function iso(date: Date): string {
  return date.toISOString().replace(/\.\d{3}Z$/, "Z");
}

export function filterHasRange(filter: Filter): boolean {
  return filter.from !== null || filter.to !== null;
}

export function pushPresetOptions(filter: Filter): { value: PushPreset; label: string }[] {
  const options: { value: PushPreset; label: string }[] = [
    { value: "yesterday", label: "昨天" },
    { value: "7d", label: "近 7 天" },
    { value: "30d", label: "近 30 天" },
  ];
  return filterHasRange(filter) ? [{ value: "filter", label: "顶栏范围" }, ...options] : options;
}

export function defaultPushPreset(filter: Filter): PushPreset {
  return filterHasRange(filter) ? "filter" : "7d";
}

/** 区间永远是「本机时区的整天」或「到现在的滚动窗口」；来源沿用顶栏筛选。 */
export function pushRangeFor(preset: PushPreset, filter: Filter, now: Date): PushRange {
  const sources = [...filter.sources];
  switch (preset) {
    case "filter":
      return { from: filter.from, to: filter.to, sources };
    case "yesterday": {
      const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
      const start = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
      return { from: iso(start), to: iso(new Date(today.getTime() - 1000)), sources };
    }
    case "7d":
      return { from: iso(new Date(now.getTime() - 7 * DAY_MS)), to: iso(now), sources };
    case "30d":
      return { from: iso(new Date(now.getTime() - 30 * DAY_MS)), to: iso(now), sources };
  }
}

export function issueKeys(issues: PushSessionIssue[]): PushSessionKey[] {
  return issues.map(({ source, session_id }) => ({ source, session_id }));
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 推完后的一句话总结。 */
export function pushOutcomeSummary(outcome: PushOutcome): string {
  const parts = [`已推送 ${outcome.sessions_succeeded} 场会话`];
  if (outcome.failed.length > 0) {
    parts.push(`${outcome.failed.length} 场失败`);
  }
  if (outcome.skipped.length > 0) {
    parts.push(`${outcome.skipped.length} 场跳过`);
  }
  if (outcome.usage_error) {
    parts.push("消耗记录失败");
  } else {
    parts.push(`消耗记录新增 ${outcome.usage_inserted} 条，已存在 ${outcome.usage_duplicates} 条`);
  }
  return parts.join("，");
}

export function pushHistoryLine(entry: PushHistoryEntry): string {
  const parts = [entry.automatic ? "自动" : "手动", `成功 ${entry.sessions_succeeded}`];
  if (entry.sessions_failed > 0) {
    parts.push(`失败 ${entry.sessions_failed}`);
  }
  if (entry.sessions_skipped > 0) {
    parts.push(`跳过 ${entry.sessions_skipped}`);
  }
  parts.push(entry.usage_failed ? "消耗记录失败" : `消耗记录 +${entry.usage_inserted}`);
  return parts.join(" · ");
}

/** 设置页自动推送那行的说明：已推到哪天、最近一次尝试。从没推过返回 null。 */
export function autoPushProgressLine(auto: AutoPushDto): string | null {
  const parts: string[] = [];
  if (auto.pushed_through) {
    parts.push(`已推送到 ${auto.pushed_through}`);
  }
  if (auto.last_attempt_at) {
    parts.push(`最近一次尝试 ${new Date(auto.last_attempt_at).toLocaleString()}`);
  }
  return parts.length > 0 ? parts.join("，") : null;
}
