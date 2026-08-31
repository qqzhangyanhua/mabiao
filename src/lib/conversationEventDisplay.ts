import type {
  ConversationAttachment,
  ConversationEvent,
  ConversationEventActor,
  ConversationEventCapabilityStatus,
  ConversationEventKind,
} from "../types";

export const EVENT_LABELS: Record<ConversationEventKind, string> = {
  message: "消息",
  plan: "计划",
  tool_call: "工具调用",
  tool_result: "工具结果",
  model_change: "模型切换",
  error: "错误",
  system_status: "系统状态",
  unadapted: "尚未适配",
};

const ACTOR_LABELS: Record<ConversationEventActor, string> = {
  user: "用户",
  assistant: "助手",
  tool: "工具",
};

const CAPABILITY_STATUS_LABELS: Record<ConversationEventCapabilityStatus, string> = {
  complete: "完整",
  missing_timestamp: "时间缺失",
  unadapted: "尚未适配",
  unadapted_missing_timestamp: "尚未适配、时间缺失",
};

export function actorLabel(actor: ConversationEventActor): string {
  return ACTOR_LABELS[actor];
}

export function capabilityStatusLabel(status: ConversationEventCapabilityStatus): string {
  return CAPABILITY_STATUS_LABELS[status];
}

export function hasEventDetails(details: unknown): boolean {
  if (details == null) {
    return false;
  }
  if (Array.isArray(details)) {
    return details.length > 0;
  }
  if (typeof details === "object") {
    return Object.keys(details).length > 0;
  }
  return true;
}

export function prettyDetails(details: unknown): string {
  try {
    return JSON.stringify(details, null, 2) ?? String(details);
  } catch {
    return String(details);
  }
}

export function formatAttachmentBytes(bytes: number | null): string {
  if (bytes === null) {
    return "大小未知";
  }
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let index = 1; value >= 1024 && index < units.length; index += 1) {
    value /= 1024;
    unit = units[index];
  }
  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${unit}`;
}

export function attachmentStatusText(attachment: ConversationAttachment): string {
  if (attachment.status === "missing") {
    return "原附件已不存在";
  }
  if (attachment.status === "unsupported") {
    return "无法在应用内加载";
  }
  return attachment.status === "embedded" ? "已嵌入" : "可用";
}

export function attachmentSignature(attachment: ConversationAttachment): string {
  return `${attachment.kind}\u0000${attachment.status}\u0000${attachment.original_path}\u0000${attachment.size_bytes ?? ""}`;
}

export function attachmentRequestKey(attachment: ConversationAttachment): string {
  return `${attachment.id}\u0000${attachmentSignature(attachment)}`;
}

export type ConversationTimelineGroup =
  | { type: "event"; event: ConversationEvent }
  | { type: "unadapted"; events: ConversationEvent[] };

export function groupTimelineEvents(
  events: ConversationEvent[],
): ConversationTimelineGroup[] {
  const groups: ConversationTimelineGroup[] = [];
  const unadapted: ConversationEvent[] = [];
  for (const event of events) {
    if (event.kind === "unadapted") {
      unadapted.push(event);
      continue;
    }
    groups.push({ type: "event", event });
  }
  if (unadapted.length > 0) {
    groups.push({ type: "unadapted", events: unadapted });
  }
  return groups;
}

export function unadaptedGroupLabel(events: ConversationEvent[]): string {
  const names = [
    ...new Set(events.map((event) => event.name).filter((name): name is string => Boolean(name))),
  ];
  const countLabel = `${events.length} 条尚未适配`;
  if (names.length === 0) {
    return countLabel;
  }
  const shown = names.slice(0, 3).join(" / ");
  return names.length > 3 ? `${countLabel} · ${shown} 等` : `${countLabel} · ${shown}`;
}
