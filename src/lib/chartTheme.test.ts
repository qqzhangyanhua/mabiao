import { describe, expect, it } from "vitest";
import { donutOption, donutTooltipPosition } from "./chartTheme";

const wideTooltip = {
  contentSize: [220, 48],
  viewSize: [160, 160],
};

describe("donutTooltipPosition", () => {
  it("keeps the box to the right of a left-side hover", () => {
    const [x, y] = donutTooltipPosition([20, 40], null, null, null, wideTooltip);
    expect(x).toBe(32);
    expect(y).toBeGreaterThanOrEqual(0);
  });

  it("does not flip left when the box is wider than the chart", () => {
    const [x] = donutTooltipPosition([30, 80], null, null, null, wideTooltip);
    expect(x).toBeGreaterThan(30);
  });

  it("clamps vertically inside the chart", () => {
    const [, yTop] = donutTooltipPosition([40, 4], null, null, null, wideTooltip);
    expect(yTop).toBe(0);
    const [, yBottom] = donutTooltipPosition([40, 158], null, null, null, wideTooltip);
    expect(yBottom + 48).toBeLessThanOrEqual(160);
  });
});

describe("donutOption tooltip", () => {
  it("appends to body and prefers the right side so panel overflow cannot clip names", () => {
    const option = donutOption([
      { name: "cursor-grok-4.6-high-fast", value: 971_668_192, color: "#8b6cff" },
    ]);
    expect(option.tooltip).toMatchObject({
      trigger: "item",
      appendTo: "body",
      confine: false,
      position: donutTooltipPosition,
    });
  });

  it("formats the full model name and share", () => {
    const option = donutOption([{ name: "claude-4.5-sonnet", value: 10, color: "#3b82f6" }]);
    const tooltip = option.tooltip;
    if (!tooltip || Array.isArray(tooltip) || typeof tooltip.formatter !== "function") {
      throw new Error("expected a tooltip formatter");
    }
    const format = tooltip.formatter as (
      params: { name: string; value: number; percent: number },
      ticket: string,
    ) => string;
    expect(
      format({ name: "cursor-grok-4.6-high-fast", value: 971_668_192, percent: 19.3 }, ""),
    ).toBe("cursor-grok-4.6-high-fast<br/>971.67M (19.3%)");
  });
});
