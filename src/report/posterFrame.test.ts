import { describe, expect, it } from "vitest";
import { FAKE_POSTER } from "./fakePosterData";
import { layoutBauhausPoster } from "./bauhausPaint";
import { layoutCastConcretePoster } from "./castConcreteLayout";
import { layoutFuseBeadPoster } from "./fuseBeadLayout";
import { layoutInkWashPoster } from "./inkWashLayout";
import { layoutNewsprintPoster } from "./newsprintLayout";
import { layoutTicketStubPoster } from "./ticketStubLayout";
import {
  POSTER_FRAME_HEIGHT,
  POSTER_FRAME_WIDTH,
  framePosterLayout,
  offsetPackedY,
  sizePosterCanvas,
  splitFrameExtra,
} from "./posterFrame";

const measure = (font: string, text: string) => {
  const match = /(\d+)px/.exec(font);
  const px = match ? Number(match[1]) : 16;
  return [...text].length * px * 0.6;
};

describe("poster frame", () => {
  it("keeps the light-glass 720×1053 card", () => {
    expect(POSTER_FRAME_WIDTH).toBe(720);
    expect(POSTER_FRAME_HEIGHT).toBe(1053);
  });

  it("pads a shorter layout to the shared height without moving slots", () => {
    const framed = framePosterLayout({ height: 670, y: { kicker: 36 } });
    expect(framed.height).toBe(POSTER_FRAME_HEIGHT);
    expect(framed.y.kicker).toBe(36);
  });

  it("sizes the canvas bitmap to the shared frame", () => {
    const canvas = {
      width: 0,
      height: 0,
      style: { width: "", height: "" },
    } as HTMLCanvasElement;
    sizePosterCanvas(canvas, 2);
    expect(canvas.width).toBe(1440);
    expect(canvas.height).toBe(2106);
    expect(canvas.style.width).toBe("720px");
    expect(canvas.style.height).toBe("1053px");
  });

  it("keeps every canvas style's content within the light-glass frame", () => {
    const heights = [
      layoutBauhausPoster(FAKE_POSTER, measure).height,
      layoutNewsprintPoster(FAKE_POSTER, measure).height,
      layoutInkWashPoster(FAKE_POSTER, measure).height,
      layoutTicketStubPoster(FAKE_POSTER, measure).height,
      layoutFuseBeadPoster(FAKE_POSTER).height,
      layoutCastConcretePoster(FAKE_POSTER, measure).height,
    ];
    for (const height of heights) {
      expect(height).toBeGreaterThan(500);
      expect(height).toBeLessThanOrEqual(POSTER_FRAME_HEIGHT);
    }
  });

  it("splits leftover height into a capped chart boost and equal gaps", () => {
    const { chartExtra, gaps } = splitFrameExtra(753, 4, 120);
    expect(chartExtra + gaps.reduce((sum, gap) => sum + gap, 0)).toBe(POSTER_FRAME_HEIGHT - 753);
    expect(chartExtra).toBe(120);
    expect(gaps).toHaveLength(4);
    expect(offsetPackedY([{ y: 10, text: "a" }], 8)).toEqual([{ y: 18, text: "a" }]);
  });

  it("fills the short canvas styles so the last slot sits in the lower third", () => {
    const newsprint = layoutNewsprintPoster(FAKE_POSTER, measure);
    const ink = layoutInkWashPoster(FAKE_POSTER, measure);
    const ticket = layoutTicketStubPoster(FAKE_POSTER, measure);
    const concrete = layoutCastConcretePoster(FAKE_POSTER, measure);
    const lower = POSTER_FRAME_HEIGHT * (2 / 3);
    expect(newsprint.height).toBe(POSTER_FRAME_HEIGHT);
    expect(newsprint.y.footer).toBeGreaterThan(lower);
    expect(newsprint.chartH).toBeGreaterThan(248);
    expect(ink.height).toBe(POSTER_FRAME_HEIGHT);
    expect(ink.y.stats).toBeGreaterThan(lower);
    expect(ink.barH).toBeGreaterThan(148);
    expect(ticket.height).toBe(POSTER_FRAME_HEIGHT);
    expect(ticket.y.footer).toBeGreaterThan(lower);
    expect(ticket.chartH).toBeGreaterThan(92);
    expect(concrete.height).toBe(POSTER_FRAME_HEIGHT);
    expect(concrete.y.stats).toBeGreaterThan(lower);
    expect(concrete.barH).toBeGreaterThan(80);
  });
});
