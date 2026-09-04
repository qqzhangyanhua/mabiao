import { describe, expect, it } from "vitest";
import { FAKE_POSTER } from "./fakePosterData";
import {
  concreteFont,
  layoutCastConcretePoster,
  wrapText,
  type TextMeasure,
} from "./castConcretePaint";
import type { PosterViewModel } from "./posterTypes";

const measure: TextMeasure = (font, text) => {
  const match = /(\d+)px/.exec(font);
  const px = match ? Number(match[1]) : 16;
  return [...text].length * px * 0.56;
};

function poster(overrides: Partial<PosterViewModel>): PosterViewModel {
  return { ...FAKE_POSTER, ...overrides };
}

describe("layoutCastConcretePoster", () => {
  it("keeps comments, bars, sources, and stats", () => {
    const layout = layoutCastConcretePoster(FAKE_POSTER, measure);
    expect(layout.comments.length).toBeGreaterThan(0);
    expect(layout.y.bars).not.toBeNull();
    expect(layout.sourceLine).toContain("Claude 52%");
    expect(layout.y.stats).not.toBeNull();
    expect(layout.height).toBeGreaterThan(500);
  });

  it("omits optional sections when empty", () => {
    const layout = layoutCastConcretePoster(
      poster({ comments: [], days: [], sources: [], stats: [] }),
      measure,
    );
    expect(layout.comments).toEqual([]);
    expect(layout.y.bars).toBeNull();
    expect(layout.sourceLine).toBeNull();
    expect(layout.y.stats).toBeNull();
  });
});

describe("concrete wrapText", () => {
  it("keeps the kicker on one line", () => {
    expect(wrapText(measure, concreteFont(700, 48), "码表 · 周报", 600)).toEqual(["码表 · 周报"]);
  });
});
