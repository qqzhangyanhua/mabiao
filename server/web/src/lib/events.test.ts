import { describe, expect, it } from "vitest";
import type { EventKind, SessionEvent } from "../api/types";
import { groupSessionTurns } from "./events";

function event(kind: EventKind, n: number): SessionEvent {
  return {
    event_id: `e${n}`,
    sequence: n,
    source_file: "/f",
    source_sequence: n,
    kind,
    occurred_at: null,
    actor: null,
    name: null,
    text: null,
  };
}

describe("groupSessionTurns", () => {
  it("folds a user line and the following tool run into two turns", () => {
    const turns = groupSessionTurns([
      { ...event("message", 0), actor: "user", text: "hi" },
      { ...event("tool_call", 1), name: "shell" },
      event("tool_result", 2),
    ]);
    expect(turns.map((turn) => turn.role)).toEqual(["user", "assistant"]);
    expect(turns[1]?.parts[0]?.type).toBe("tools");
  });
});
