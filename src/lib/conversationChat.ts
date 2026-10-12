import type { ConversationEvent } from "../types";

export type ChatToolStep = {
  call: ConversationEvent;
  result: ConversationEvent | null;
};

export type ChatPart =
  | { type: "prose"; event: ConversationEvent }
  | { type: "tools"; steps: ChatToolStep[] }
  | { type: "notice"; event: ConversationEvent };

export type ChatTurn = {
  id: string;
  role: "user" | "assistant";
  occurredAt: string | null;
  eventIds: string[];
  parts: ChatPart[];
};

function isUserMessage(event: ConversationEvent): boolean {
  return event.kind === "message" && event.actor !== "assistant" && event.actor !== "tool";
}

function isTool(event: ConversationEvent): boolean {
  return event.kind === "tool_call" || event.kind === "tool_result";
}

function beginTurn(role: ChatTurn["role"], event: ConversationEvent): ChatTurn {
  return {
    id: event.event_id,
    role,
    occurredAt: event.occurred_at,
    eventIds: [],
    parts: [],
  };
}

function remember(turn: ChatTurn, event: ConversationEvent) {
  turn.eventIds.push(event.event_id);
  if (!turn.occurredAt && event.occurred_at) {
    turn.occurredAt = event.occurred_at;
  }
}

function toolRun(turn: ChatTurn): Extract<ChatPart, { type: "tools" }> {
  const last = turn.parts[turn.parts.length - 1];
  if (last?.type === "tools") {
    return last;
  }
  const part: Extract<ChatPart, { type: "tools" }> = { type: "tools", steps: [] };
  turn.parts.push(part);
  return part;
}

function pushTool(turn: ChatTurn, event: ConversationEvent) {
  remember(turn, event);
  const run = toolRun(turn);
  const step = run.steps[run.steps.length - 1];
  if (event.kind === "tool_result" && step && step.call.kind === "tool_call" && step.result === null) {
    step.result = event;
    return;
  }
  run.steps.push({ call: event, result: null });
}

function pushOther(turn: ChatTurn, event: ConversationEvent) {
  remember(turn, event);
  if (event.kind === "message" || event.kind === "plan") {
    turn.parts.push({ type: "prose", event });
    return;
  }
  turn.parts.push({ type: "notice", event });
}

/** 把扁平事件收成对话轮次：用户一句一轮，助手正文和其间的工具调用合成一轮。 */
export function groupConversationTurns(events: readonly ConversationEvent[]): ChatTurn[] {
  const turns: ChatTurn[] = [];
  let assistant: ChatTurn | null = null;

  const closeAssistant = () => {
    if (assistant && assistant.parts.length > 0) {
      turns.push(assistant);
    }
    assistant = null;
  };

  for (const event of events) {
    if (event.kind === "unadapted") {
      continue;
    }
    if (isUserMessage(event)) {
      closeAssistant();
      const turn = beginTurn("user", event);
      remember(turn, event);
      turn.parts.push({ type: "prose", event });
      turns.push(turn);
      continue;
    }
    if (!assistant) {
      assistant = beginTurn("assistant", event);
    }
    if (isTool(event)) {
      pushTool(assistant, event);
    } else {
      pushOther(assistant, event);
    }
  }
  closeAssistant();
  return turns;
}

export function chatStepCount(turn: ChatTurn): number {
  return turn.parts.filter((part) => part.type === "tools").length;
}
