import { describe, expect, it } from "vitest";
import type { ConversationEvent } from "../types";
import {
  compactionBoundaryLabel,
  groupTimelineEvents,
  isCompactionBoundary,
  unadaptedGroupLabel,
} from "./conversationEventDisplay";

function event(
  kind: ConversationEvent["kind"],
  id: string,
  name: string | null = null,
): ConversationEvent {
  return {
    event_id: id,
    sequence: 0,
    source_file: "session.jsonl",
    source_sequence: 0,
    kind,
    occurred_at: null,
    actor: null,
    name,
    text: null,
    details: {},
    attachments: [],
    capability_status: kind === "unadapted" ? "unadapted" : "complete",
    content_status: "complete",
  };
}

describe("groupTimelineEvents", () => {
  it("keeps adapted events in order and folds every unadapted event into one trailing group", () => {
    const groups = groupTimelineEvents([
      event("message", "m1"),
      event("unadapted", "u1", "hook_execution"),
      event("unadapted", "u2", "hook_execution"),
      event("tool_call", "t1"),
      event("unadapted", "u3", "future_update"),
    ]);
    expect(groups).toEqual([
      { type: "event", event: event("message", "m1") },
      { type: "event", event: event("tool_call", "t1") },
      {
        type: "unadapted",
        events: [
          event("unadapted", "u1", "hook_execution"),
          event("unadapted", "u2", "hook_execution"),
          event("unadapted", "u3", "future_update"),
        ],
      },
    ]);
  });
});

describe("isCompactionBoundary", () => {
  it("recognizes Gemini compression and other compact system events", () => {
    expect(isCompactionBoundary(event("system_status", "c1", "compression"))).toBe(true);
    expect(isCompactionBoundary(event("system_status", "c2", "compaction_state"))).toBe(true);
    expect(isCompactionBoundary(event("system_status", "s1", "session_started"))).toBe(false);
    expect(isCompactionBoundary(event("message", "m1"))).toBe(false);
  });
});

describe("compactionBoundaryLabel", () => {
  it("prefers event text and falls back to a generic boundary label", () => {
    const withText = event("system_status", "c1", "compression");
    withText.text = "上下文已压缩：120000 → 18000 tokens";
    expect(compactionBoundaryLabel(withText)).toBe("上下文已压缩：120000 → 18000 tokens");
    expect(compactionBoundaryLabel(event("system_status", "c2", "compression"))).toBe(
      "上下文已压缩",
    );
  });
});

describe("unadaptedGroupLabel", () => {
  it("summarizes count and distinct raw kinds", () => {
    expect(unadaptedGroupLabel([event("unadapted", "u1")])).toBe("1 条尚未适配");
    expect(
      unadaptedGroupLabel([
        event("unadapted", "u1", "hook_execution"),
        event("unadapted", "u2", "hook_execution"),
      ]),
    ).toBe("2 条尚未适配 · hook_execution");
    expect(
      unadaptedGroupLabel([
        event("unadapted", "a", "one"),
        event("unadapted", "b", "two"),
        event("unadapted", "c", "three"),
        event("unadapted", "d", "four"),
      ]),
    ).toBe("4 条尚未适配 · one / two / three 等");
  });
});
