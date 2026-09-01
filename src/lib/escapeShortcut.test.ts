import { describe, expect, it } from "vitest";
import { consumeEscape, consumeRefreshShortcut } from "./escapeShortcut";

function keyEvent(key: string) {
  let prevented = 0;
  let stopped = 0;
  return {
    key,
    prevented: () => prevented,
    stopped: () => stopped,
    preventDefault() {
      prevented += 1;
    },
    stopPropagation() {
      stopped += 1;
    },
  };
}

describe("consumeEscape", () => {
  it("consumes Escape so later listeners do not also clear filters", () => {
    const event = keyEvent("Escape");
    expect(consumeEscape(event)).toBe(true);
    expect(event.prevented()).toBe(1);
    expect(event.stopped()).toBe(1);
  });

  it("leaves other keys for global shortcuts", () => {
    const event = keyEvent("r");
    expect(consumeEscape(event)).toBe(false);
    expect(event.prevented()).toBe(0);
    expect(event.stopped()).toBe(0);
  });
});

describe("consumeRefreshShortcut", () => {
  it("consumes r and R so detail view does not trigger ingest", () => {
    for (const key of ["r", "R"]) {
      const event = keyEvent(key);
      expect(consumeRefreshShortcut(event)).toBe(true);
      expect(event.prevented()).toBe(1);
      expect(event.stopped()).toBe(1);
    }
  });

  it("leaves unrelated keys alone", () => {
    const event = keyEvent("Escape");
    expect(consumeRefreshShortcut(event)).toBe(false);
    expect(event.prevented()).toBe(0);
    expect(event.stopped()).toBe(0);
  });
});
