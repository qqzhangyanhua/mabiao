import { describe, expect, it } from "vitest";
import { FAKE_POSTER } from "./fakePosterData";
import { BEAD, FONT_COMMENT, layoutFuseBeadPoster, snap, wrapBeadText } from "./fuseBeadLayout";
import type { PosterViewModel } from "./posterTypes";

function poster(overrides: Partial<PosterViewModel>): PosterViewModel {
  return { ...FAKE_POSTER, ...overrides };
}

describe("layoutFuseBeadPoster", () => {
  it("snaps section tops to the bead grid and keeps room for seven slots", () => {
    const layout = layoutFuseBeadPoster(FAKE_POSTER);
    expect(layout.y.title % BEAD).toBe(0);
    expect(layout.y.hero % BEAD).toBe(0);
    expect(layout.height).toBeGreaterThan(layout.y.footer);
    expect(layout.height % BEAD).toBe(0);
    expect(layout.y.bars).toBeGreaterThan(layout.y.comments);
  });

  it("shrinks when comments and stats are empty", () => {
    const full = layoutFuseBeadPoster(FAKE_POSTER);
    const empty = layoutFuseBeadPoster(poster({ comments: [], sources: [], stats: [] }));
    expect(empty.height).toBeLessThan(full.height);
  });
});

describe("snap", () => {
  it("rounds to the bead pitch", () => {
    expect(snap(0)).toBe(0);
    expect(snap(BEAD + 1)).toBe(BEAD);
  });
});

describe("wrapBeadText", () => {
  it("keeps ascii words together", () => {
    const lines = wrapBeadText("你这周烧掉了 12.4M token。", FONT_COMMENT, 420);
    expect(lines.some((line) => line.includes("token"))).toBe(true);
    expect(lines.join("")).toBe("你这周烧掉了 12.4M token。");
  });
});
