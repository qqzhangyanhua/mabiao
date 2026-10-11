import { describe, expect, it } from "vitest";
import { nearestTickId, percentFromClientX, timelineSpan, timelineTicks } from "./timeline";

const events = [
  { event_id: "a", kind: "message" as const, occurred_at: "2026-10-10T02:00:00Z" },
  { event_id: "b", kind: "tool_call" as const, occurred_at: "2026-10-10T02:30:00Z" },
  { event_id: "c", kind: "message" as const, occurred_at: "2026-10-10T03:00:00Z" },
];

describe("timelineSpan", () => {
  it("uses session bounds when they are wider than the events", () => {
    expect(timelineSpan(events, "2026-10-10T01:00:00Z", "2026-10-10T04:00:00Z")).toEqual({
      start: Date.parse("2026-10-10T01:00:00Z"),
      end: Date.parse("2026-10-10T04:00:00Z"),
    });
  });

  it("returns null when nothing has a time", () => {
    expect(timelineSpan([{ occurred_at: null }])).toBeNull();
  });
});

describe("timelineTicks", () => {
  it("places events by elapsed time", () => {
    const span = { start: Date.parse("2026-10-10T02:00:00Z"), end: Date.parse("2026-10-10T04:00:00Z") };
    const ticks = timelineTicks(events, span);
    expect(ticks[0]?.left).toBe(0);
    expect(ticks[1]?.left).toBe(25);
    expect(ticks[2]?.left).toBe(50);
  });

  it("spaces evenly when there is no clock", () => {
    const ticks = timelineTicks(
      events.map((event) => ({ ...event, occurred_at: null })),
      null,
    );
    expect(ticks.map((tick) => tick.left)).toEqual([0, 50, 100]);
  });
});

describe("nearestTickId / percentFromClientX", () => {
  it("picks the closest tick and clamps the pointer", () => {
    const ticks = timelineTicks(events, {
      start: Date.parse("2026-10-10T02:00:00Z"),
      end: Date.parse("2026-10-10T04:00:00Z"),
    });
    expect(nearestTickId(ticks, 20)).toBe("b");
    expect(percentFromClientX(0, 200, 0)).toBe(0);
    expect(percentFromClientX(300, 200, 0)).toBe(100);
    expect(percentFromClientX(50, 200, 0)).toBe(25);
  });
});
