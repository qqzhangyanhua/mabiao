import { describe, expect, it } from "vitest";
import type { EventKind, SessionEvent } from "../api/types";
import { filterEvents } from "./events";

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

describe("filterEvents", () => {
  const events = [
    event("message", 0),
    event("tool_call", 1),
    event("tool_result", 2),
    event("plan", 3),
    event("error", 4),
  ];
  const ids = (list: SessionEvent[]) => list.map((e) => e.sequence);

  it("keeps everything for all", () => {
    expect(ids(filterEvents(events, "all"))).toEqual([0, 1, 2, 3, 4]);
  });
  it("splits conversation, tool traffic and the rest without overlap", () => {
    expect(ids(filterEvents(events, "messages"))).toEqual([0, 3]);
    expect(ids(filterEvents(events, "tools"))).toEqual([1, 2]);
    expect(ids(filterEvents(events, "other"))).toEqual([4]);
  });
});
