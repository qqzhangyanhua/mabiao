import type { EventKind, SessionEvent } from "../api/types";

export type EventFilter = "all" | "messages" | "tools" | "other";

export const EVENT_FILTER_LABEL: Record<EventFilter, string> = {
  all: "全部",
  messages: "对话",
  tools: "工具调用",
  other: "其他",
};

export const EVENT_KIND_LABEL: Record<EventKind, string> = {
  message: "消息",
  plan: "计划",
  tool_call: "工具调用",
  tool_result: "工具结果",
  model_change: "切换模型",
  error: "错误",
  system_status: "系统状态",
  unadapted: "未适配",
};

function filterOf(kind: EventKind): Exclude<EventFilter, "all"> {
  switch (kind) {
    case "message":
    case "plan":
      return "messages";
    case "tool_call":
    case "tool_result":
      return "tools";
    default:
      return "other";
  }
}

export function filterEvents(events: SessionEvent[], filter: EventFilter): SessionEvent[] {
  return filter === "all" ? events : events.filter((e) => filterOf(e.kind) === filter);
}
