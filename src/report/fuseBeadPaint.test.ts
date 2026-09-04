import { describe, expect, it } from "vitest";
import { FAKE_POSTER } from "./fakePosterData";
import { BEAD, FONT_COMMENT, layoutFuseBeadPoster, snap, wrapBeadText } from "./fuseBeadLayout";
import type { PosterSourceSlice, PosterViewModel } from "./posterTypes";

function poster(overrides: Partial<PosterViewModel>): PosterViewModel {
  return { ...FAKE_POSTER, ...overrides };
}

function sources(count: number): PosterSourceSlice[] {
  return Array.from({ length: count }, (_, index) => ({
    label: `S${index + 1}`,
    pct: 1,
    color: "#888888",
  }));
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

  it("grows the source card as more sources appear", () => {
    const three = layoutFuseBeadPoster(poster({ sources: sources(3) }));
    const six = layoutFuseBeadPoster(poster({ sources: sources(6) }));
    expect(six.sourceH).toBeGreaterThan(three.sourceH);
  });

  it("does not reserve four-row height for one or two sources", () => {
    const one = layoutFuseBeadPoster(poster({ sources: sources(1) }));
    const two = layoutFuseBeadPoster(poster({ sources: sources(2) }));
    const four = layoutFuseBeadPoster(poster({ sources: sources(4) }));
    expect(one.sourceH).toBeLessThan(four.sourceH);
    expect(two.sourceH).toBeLessThan(four.sourceH);
  });

  it("fits every source inside the card, even at the 14-source cap", () => {
    const count = 14;
    const layout = layoutFuseBeadPoster(poster({ sources: sources(count) }));
    // 行距下限是两颗豆子，避免条目叠在一起；卡片必须盖住标题区 + 每一行。
    expect(layout.sourceRowH).toBeGreaterThanOrEqual(BEAD * 2);
    expect(layout.sourceH).toBeGreaterThanOrEqual(layout.sourceHeadH + count * layout.sourceRowH);
  });

  it("reserves footer space for top_session even when busiest_day is missing", () => {
    const full = layoutFuseBeadPoster(FAKE_POSTER);
    const shifted = layoutFuseBeadPoster(
      poster({ stats: FAKE_POSTER.stats.filter((stat) => stat.kind !== "busiest_day") }),
    );
    expect(shifted.height - shifted.y.footer).toBe(full.height - full.y.footer);
  });

  it("does not reserve footer space when top_session is missing", () => {
    const layout = layoutFuseBeadPoster(
      poster({ stats: FAKE_POSTER.stats.filter((stat) => stat.kind !== "top_session") }),
    );
    expect(layout.height - layout.y.footer).toBeLessThan(snap(80));
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
