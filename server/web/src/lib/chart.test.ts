import { describe, expect, it } from "vitest";
import { BRAND_CHART_COLORS, DEFAULT_BRAND_CHART_COLOR } from "../generated/displayIdentity";
import { niceCeil, sliceColor, sourceTone, sparkPoints } from "./chart";

describe("sourceTone", () => {
  it("maps known sources to deterministic brand hex colors and aligns shared brands", () => {
    expect(sourceTone("claude")).toBe("#ea580c");
    expect(sourceTone("codex")).toBe("#6d5ef6");
    expect(sourceTone("cursor")).toBe("#64748b");
    expect(sourceTone("cursor_agent")).toBe(sourceTone("cursor"));
    expect(sourceTone("factory")).toBe(sourceTone("droid"));
    expect(sourceTone("agy")).toBe(sourceTone("gemini"));
    expect(sourceTone("antigravity")).toBe(sourceTone("agy"));
    expect(sourceTone("qoder_cn")).toBe(sourceTone("qoder"));
    expect(sourceTone("codebuddy_ide")).toBe(sourceTone("codebuddy"));
    expect(sourceTone("windsurf")).toBe(BRAND_CHART_COLORS.windsurf);
    expect(sourceTone("windsurf")).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("falls back to the neutral default hex for brandless or unknown sources", () => {
    expect(sourceTone("devin")).toBe(DEFAULT_BRAND_CHART_COLOR);
    expect(sourceTone("brand_new")).toBe(DEFAULT_BRAND_CHART_COLOR);
  });
});

describe("sliceColor", () => {
  it("prefers the brand hex color for known sources, then falls back to the chart palette", () => {
    expect(sliceColor("grok", 0)).toBe("#db2777");
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
