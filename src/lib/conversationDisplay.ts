import type { ConversationQuery, ConversationSessionRow, Filter } from "../types";
import { sourceLabel, formatClock, projectLabel, relativeTime } from "./format";

const CAPABILITY_LABELS: Record<string, string> = {
  messages: "基础正文",
  events: "完整事件",
  usage: "用量明细",
};

export function capabilityLabel(capability: string): string {
  return CAPABILITY_LABELS[capability] ?? capability;
}

export function conversationSourceLabel(source: string): string {
  return source === "cursor_agent" ? "Cursor / Cursor Agent" : sourceLabel(source);
}

export function conversationSourceOptions(usageSources: string[]): string[] {
  const sources = new Set(usageSources);
  sources.add("cursor_agent");
  return [...sources].sort();
}

/** 用量页来源筛选里的 `cursor` 是账号用量；对话目录对应 `cursor_agent`。 */
export function conversationSourcesFromUsageFilter(sources: string[]): string[] {
  if (sources.length === 0) {
    return [];
  }
  return [...new Set(sources.map((source) => (source === "cursor" ? "cursor_agent" : source)))];
}

export function conversationQueryFromFilter(
  filter: Filter,
  overrides: {
    projects?: string[];
    page?: number;
    page_size?: number;
  } = {},
): ConversationQuery {
  return {
    search: null,
    page: overrides.page ?? 1,
    page_size: overrides.page_size,
    sources: conversationSourcesFromUsageFilter(filter.sources),
    projects: overrides.projects ?? filter.projects,
    models: filter.models,
    providers: filter.providers,
    from: filter.from,
    to: filter.to,
    tool_names: [],
    tool_failed: false,
  };
}

export function conversationStatusLabel(status: string): string {
  return status === "experimental" ? "实验性" : status;
}

export function conversationFileUnavailableLabel(source: string): string {
  return source === "cursor_agent" ? "缺少 transcript" : "原文件已删除";
}

export function conversationSessionTime(
  session: Pick<ConversationSessionRow, "ended_at" | "started_at">,
): string {
  return session.ended_at || session.started_at;
}

export function conversationRangeLabel(
  session: Pick<ConversationSessionRow, "ended_at" | "started_at">,
): string {
  const started = session.started_at || session.ended_at;
  const ended = session.ended_at || session.started_at;
  if (!started) {
    return "—";
  }
  return `${relativeTime(started)} → ${relativeTime(ended)}`;
}

export function conversationRangeTitle(
  session: Pick<ConversationSessionRow, "ended_at" | "started_at">,
): string {
  const started = session.started_at || session.ended_at;
  const ended = session.ended_at || session.started_at;
  if (!started) {
    return "";
  }
  return `${formatClock(started)} → ${formatClock(ended)}`;
}

export function conversationDetailSummary(session: ConversationSessionRow): string {
  const time = conversationSessionTime(session);
  const parts = [
    conversationSourceLabel(session.source),
    projectLabel(session.project),
    session.model || "未标注",
  ];
  if (time) {
    parts.push(relativeTime(time));
  }
  return parts.join(" · ");
}
