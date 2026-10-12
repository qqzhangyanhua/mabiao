import type { EventKind, SessionEvent } from "../api/types";

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

export type SessionToolStep = { call: SessionEvent; result: SessionEvent | null };

export type SessionChatPart =
  | { type: "prose"; event: SessionEvent }
  | { type: "tools"; steps: SessionToolStep[] }
  | { type: "notice"; event: SessionEvent };

export type SessionChatTurn = {
  id: string;
  role: "user" | "assistant";
  occurredAt: string | null;
  parts: SessionChatPart[];
};

function isUserMessage(event: SessionEvent): boolean {
  return event.kind === "message" && event.actor !== "assistant" && event.actor !== "tool";
}

function toolRun(turn: SessionChatTurn): Extract<SessionChatPart, { type: "tools" }> {
  const last = turn.parts[turn.parts.length - 1];
  if (last?.type === "tools") return last;
  const part: Extract<SessionChatPart, { type: "tools" }> = { type: "tools", steps: [] };
  turn.parts.push(part);
  return part;
}

/** 用户一句一轮；助手正文和夹在中间的工具调用合成一轮。 */
export function groupSessionTurns(events: readonly SessionEvent[]): SessionChatTurn[] {
  const turns: SessionChatTurn[] = [];
  let assistant: SessionChatTurn | null = null;
  const close = () => {
    if (assistant && assistant.parts.length > 0) turns.push(assistant);
    assistant = null;
  };
  for (const event of events) {
    if (isUserMessage(event)) {
      close();
      turns.push({
        id: event.event_id,
        role: "user",
        occurredAt: event.occurred_at,
        parts: [{ type: "prose", event }],
      });
      continue;
    }
    if (!assistant) {
      assistant = { id: event.event_id, role: "assistant", occurredAt: event.occurred_at, parts: [] };
    }
    if (!assistant.occurredAt && event.occurred_at) assistant.occurredAt = event.occurred_at;
    if (event.kind === "tool_call" || event.kind === "tool_result") {
      const run = toolRun(assistant);
      const step = run.steps[run.steps.length - 1];
      if (event.kind === "tool_result" && step?.call.kind === "tool_call" && step.result === null) {
        step.result = event;
      } else {
        run.steps.push({ call: event, result: null });
      }
    } else if (event.kind === "message" || event.kind === "plan") {
      assistant.parts.push({ type: "prose", event });
    } else {
      assistant.parts.push({ type: "notice", event });
    }
  }
  close();
  return turns;
}

export function sessionStepCount(turn: SessionChatTurn): number {
  return turn.parts.filter((part) => part.type === "tools").length;
}
