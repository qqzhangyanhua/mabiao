import { describe, expect, it } from "vitest";
import { niceCeil, sliceColor, sourceTone, sparkPoints } from "./chart";

describe("sourceTone", () => {
  it("maps known sources onto the existing tone tokens", () => {
    expect(sourceTone("codex")).toBe("var(--tone-codex)");
    expect(sourceTone("claude")).toBe("var(--tone-claude)");
    expect(sourceTone("cursor_agent")).toBe("var(--tone-cursor)");
    expect(sourceTone("factory")).toBe("var(--tone-codex)");
  });

  it("falls back to the cursor tone for unknown sources", () => {
    expect(sourceTone("brand_new")).toBe("var(--tone-cursor)");
  });
});

describe("sliceColor", () => {
  it("prefers the source tone, then the chart palette", () => {
    expect(sliceColor("grok", 0)).toBe("var(--tone-grok)");
    expect(sliceColor("unknown-model", 2)).toBe("var(--chart-3)");
  });
});

describe("niceCeil", () => {
  it("rounds up to a 1/2/2.5/5 step of the same magnitude", () => {
    expect(niceCeil(0)).toBe(0);
    expect(niceCeil(142_900_000)).toBe(200_000_000);
    expect(niceCeil(180_000)).toBe(200_000);
    expect(niceCeil(2_100)).toBe(2_500);
    expect(niceCeil(47.27)).toBe(50);
    expect(niceCeil(100)).toBe(100);
  });
});

describe("sparkPoints", () => {
  it("spans the box from left to right and flips y", () => {
    expect(sparkPoints([0, 10], 100, 20, 0)).toBe("0.0,20.0 100.0,0.0");
  });

  it("draws a flat line in the middle-bottom when all values are equal", () => {
    expect(sparkPoints([5, 5], 10, 10, 0)).toBe("0.0,10.0 10.0,10.0");
  });

  it("returns nothing for no data", () => {
    expect(sparkPoints([], 10, 10)).toBe("");
  });
});
