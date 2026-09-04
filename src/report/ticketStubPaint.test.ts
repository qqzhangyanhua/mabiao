import { describe, expect, it } from "vitest";
import { FAKE_POSTER } from "./fakePosterData";
import {
  layoutTicketStubPoster,
  ticketFont,
  ticketSerial,
  wrapText,
  type TextMeasure,
} from "./ticketStubPaint";
import type { PosterViewModel } from "./posterTypes";

const measure: TextMeasure = (font, text) => {
  const match = /(\d+)px/.exec(font);
  const px = match ? Number(match[1]) : 16;
  return [...text].length * px * 0.56;
};

function poster(overrides: Partial<PosterViewModel>): PosterViewModel {
  return { ...FAKE_POSTER, ...overrides };
}

describe("ticketSerial", () => {
  it("builds a stub number from the period label", () => {
    expect(ticketSerial("2026年8月24日 – 8月30日")).toBe("No. 0824-0830");
    expect(ticketSerial("2026年12月30日 – 2027年1月5日")).toBe("No. 1230-0105");
  });

  it("returns null when the range has no two calendar days", () => {
    expect(ticketSerial("")).toBeNull();
    expect(ticketSerial("本周")).toBeNull();
  });
});

describe("layoutTicketStubPoster", () => {
  it("keeps the seven slots and a serial from the dummy range", () => {
    const layout = layoutTicketStubPoster(FAKE_POSTER, measure);
    expect(layout.serial).toBe("No. 0824-0830");
    expect(layout.comments.length).toBeGreaterThan(0);
    expect(layout.y.chart).not.toBeNull();
    expect(layout.y.stats).not.toBeNull();
    expect(layout.height).toBe(1053);
    expect(layout.chartH).toBeGreaterThan(92);
  });

  it("omits optional blocks when those slots are empty", () => {
    const layout = layoutTicketStubPoster(
      poster({ comments: [], days: [], sources: [], stats: [], rangeLabel: "" }),
      measure,
    );
    expect(layout.serial).toBeNull();
    expect(layout.comments).toEqual([]);
    expect(layout.y.chart).toBeNull();
    expect(layout.y.sources).toBeNull();
    expect(layout.y.stats).toBeNull();
  });
});

describe("ticket wrapText", () => {
  it("keeps a short kicker on one line", () => {
    expect(wrapText(measure, ticketFont(600, 15), "码表 · 周报", 400)).toEqual(["码表 · 周报"]);
  });
});
