import { describe, expect, it } from "vitest";
import { FAKE_POSTER } from "./fakePosterData";
import { layoutNewsprintPoster, newsprintFont, wrapText, type TextMeasure } from "./newsprintPaint";
import type { PosterViewModel } from "./posterTypes";

const measure: TextMeasure = (font, text) => {
  const match = /(\d+)px/.exec(font);
  const px = match ? Number(match[1]) : 16;
  return [...text].length * px * 0.58;
};

function poster(overrides: Partial<PosterViewModel>): PosterViewModel {
  return { ...FAKE_POSTER, ...overrides };
}

describe("newsprint wrapText", () => {
  it("keeps a short headline on one line", () => {
    expect(wrapText(measure, newsprintFont(900, 34), "本周 token $18.60", 600)).toEqual([
      "本周 token $18.60",
    ]);
  });
});

describe("layoutNewsprintPoster", () => {
  it("uses cost in the headline and lays out charts plus three stats", () => {
    const layout = layoutNewsprintPoster(FAKE_POSTER, measure);
    expect(layout.headline).toBe("本周 token $18.60");
    expect(layout.body.length).toBeGreaterThan(0);
    expect(layout.y.charts).not.toBeNull();
    expect(layout.statCols).toHaveLength(3);
    expect(layout.statCols.map((col) => col.label)).toEqual([
      "最忙的一天",
      "模型 Top 3",
      "最贵的一次",
    ]);
    expect(layout.height).toBeGreaterThan(700);
  });

  it("falls back to token total when there is no cost", () => {
    const layout = layoutNewsprintPoster(poster({ totalCostLabel: null }), measure);
    expect(layout.headline).toBe("12.4M 本周 token");
  });

  it("omits charts and stats when those slots are empty", () => {
    const layout = layoutNewsprintPoster(
      poster({ comments: [], days: [], sources: [], stats: [] }),
      measure,
    );
    expect(layout.body).toEqual([]);
    expect(layout.y.charts).toBeNull();
    expect(layout.y.stats).toBeNull();
    expect(layout.statCols).toEqual([]);
    expect(layout.height).toBeGreaterThan(200);
  });
});
