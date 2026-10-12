import { describe, expect, it } from "vitest";
import { chatStepCount, groupConversationTurns } from "./conversationChat";
import type { ConversationEvent } from "../types";

function event(
  id: string,
  kind: ConversationEvent["kind"],
  actor: ConversationEvent["actor"] = null,
  name: string | null = null,
): ConversationEvent {
  return {
    event_id: id,
    sequence: 0,
    source_file: "session.jsonl",
    source_sequence: 0,
    kind,
    occurred_at: null,
    actor,
    name,
    text: id,
    details: {},
    attachments: [],
    capability_status: "complete",
    content_status: "complete",
  };
}

describe("groupConversationTurns", () => {
  it("把用户消息和助手的正文、工具收成两轮", () => {
    const turns = groupConversationTurns([
      event("u", "message", "user"),
      event("a1", "message", "assistant"),
      event("c1", "tool_call", "assistant", "shell"),
      event("r1", "tool_result", "tool"),
      event("a2", "message", "assistant"),
      event("c2", "tool_call", "assistant", "read"),
      event("r2", "tool_result", "tool"),
    ]);

    expect(turns.map((turn) => turn.role)).toEqual(["user", "assistant"]);
    const assistant = turns[1];
    expect(assistant?.parts.map((part) => part.type)).toEqual(["prose", "tools", "prose", "tools"]);
    expect(assistant ? chatStepCount(assistant) : 0).toBe(2);
    const firstTools = assistant?.parts[1];
    expect(firstTools?.type === "tools" ? firstTools.steps[0]?.result?.event_id : null).toBe("r1");
    expect(assistant?.eventIds).toEqual(["a1", "c1", "r1", "a2", "c2", "r2"]);
  });

  it("连续工具并进同一步，错误另起一行", () => {
    const turns = groupConversationTurns([
      event("c1", "tool_call", "assistant"),
      event("c2", "tool_call", "assistant"),
      event("e", "error", "assistant"),
    ]);
    expect(turns).toHaveLength(1);
    expect(turns[0]?.parts.map((part) => part.type)).toEqual(["tools", "notice"]);
    const tools = turns[0]?.parts[0];
    expect(tools?.type === "tools" ? tools.steps : []).toHaveLength(2);
  });

  it("没有说话人的消息算用户", () => {
    const turns = groupConversationTurns([event("m", "message", null)]);
    expect(turns[0]?.role).toBe("user");
  });
});
