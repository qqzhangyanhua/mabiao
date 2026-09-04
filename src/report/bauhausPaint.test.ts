import { describe, expect, it } from "vitest";
import { FAKE_POSTER } from "./fakePosterData";
import { bauhausFont, layoutBauhausPoster, wrapText, type TextMeasure } from "./bauhausPaint";
import type { PosterViewModel } from "./posterTypes";

const measure: TextMeasure = (font, text) => {
  const match = /(\d+)px/.exec(font);
  const px = match ? Number(match[1]) : 16;
  return [...text].length * px * 0.62;
};

function poster(overrides: Partial<PosterViewModel>): PosterViewModel {
  return { ...FAKE_POSTER, ...overrides };
}

describe("wrapText", () => {
  it("returns no lines for empty text", () => {
    expect(wrapText(measure, bauhausFont(800, 20), "", 120)).toEqual([]);
  });

  it("keeps short text on one line", () => {
    expect(wrapText(measure, bauhausFont(800, 20), "周三", 200)).toEqual(["周三"]);
  });

  it("splits a long value on separators instead of mid-glyph", () => {
    const text = "claude-opus-4.1 · gpt-5 · grok-4";
    const lines = wrapText(measure, bauhausFont(800, 20), text, 160);
    expect(lines.length).toBeGreaterThan(1);
    expect(lines.some((line) => /rok-4/.test(line) && !line.includes("grok-4"))).toBe(false);
    expect(lines.join("")).toBe(text);
  });
});

describe("layoutBauhausPoster", () => {
  it("lays out the seven slots for the capture dummy", () => {
    const layout = layoutBauhausPoster(FAKE_POSTER, measure);
    expect(layout.height).toBeGreaterThan(800);
    expect(layout.cost).not.toBeNull();
    expect(layout.comments).toHaveLength(3);
    expect(layout.y.daysTitle).not.toBeNull();
    expect(layout.y.sourcesTitle).not.toBeNull();
    expect(layout.y.stats).not.toBeNull();
    expect(layout.statRows.length).toBeGreaterThan(0);
    expect(layout.y.strips).toBeLessThan(layout.height);
  });

  it("omits empty optional sections and still has a footer", () => {
    const layout = layoutBauhausPoster(
      poster({
        totalCostLabel: null,
        comments: [],
        days: [],
        sources: [],
        stats: [],
      }),
      measure,
    );
    expect(layout.cost).toBeNull();
    expect(layout.comments).toEqual([]);
    expect(layout.y.insight).toBeNull();
    expect(layout.y.daysTitle).toBeNull();
    expect(layout.y.sourcesTitle).toBeNull();
    expect(layout.y.stats).toBeNull();
    expect(layout.statRows).toEqual([]);
    expect(layout.height).toBeGreaterThan(200);
  });

  it("puts three stats into two columns with the third on the next row", () => {
    const layout = layoutBauhausPoster(FAKE_POSTER, measure);
    expect(layout.statRows).toHaveLength(2);
    expect(layout.statRows[0]?.left?.label).toBe("最忙的一天");
    expect(layout.statRows[0]?.right?.label).toBe("模型 Top 3");
    expect(layout.statRows[1]?.left?.label).toBe("最贵的一次");
    expect(layout.statRows[1]?.right).toBeNull();
  });
});
