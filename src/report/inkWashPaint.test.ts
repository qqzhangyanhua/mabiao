import { describe, expect, it } from "vitest";
import { FAKE_POSTER } from "./fakePosterData";
import { inkKai, layoutInkWashPoster, wrapText, type TextMeasure } from "./inkWashPaint";
import type { PosterViewModel } from "./posterTypes";

const measure: TextMeasure = (font, text) => {
  const match = /(\d+)px/.exec(font);
  const px = match ? Number(match[1]) : 16;
  return [...text].length * px * 0.58;
};

function poster(overrides: Partial<PosterViewModel>): PosterViewModel {
  return { ...FAKE_POSTER, ...overrides };
}

describe("layoutInkWashPoster", () => {
  it("keeps the seven slots and a source line", () => {
    const layout = layoutInkWashPoster(FAKE_POSTER, measure);
    expect(layout.comments.length).toBeGreaterThan(0);
    expect(layout.sourceLines[0]?.text).toContain("来源占比");
    expect(layout.sourceLines[0]?.text).toContain("Claude 52%");
    expect(layout.statLines.length).toBeGreaterThan(0);
    expect(layout.statLines[0]?.text).toContain("最忙的一天");
    expect(layout.height).toBe(1053);
    expect(layout.barH).toBeGreaterThan(148);
    expect(layout.numberSize).toBeGreaterThan(70);
  });

  it("omits source and stats lines when those slots are empty", () => {
    const layout = layoutInkWashPoster(
      poster({ comments: [], days: [], sources: [], stats: [] }),
      measure,
    );
    expect(layout.comments).toEqual([]);
    expect(layout.sourceLines).toEqual([]);
    expect(layout.statLines).toEqual([]);
    expect(layout.y.sources).toBeNull();
    expect(layout.y.stats).toBeNull();
  });
});

describe("ink wash wrapText", () => {
  it("keeps a short kicker on one line", () => {
    expect(wrapText(measure, inkKai(600, 22), "码表 · 周报", 600)).toEqual(["码表 · 周报"]);
  });
});
